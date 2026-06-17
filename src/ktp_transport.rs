use crate::ktp::{
    decode_frame, encode_frame, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN,
};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::error::Error;
use std::fmt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const KTP_CRYPTO_MAGIC: &[u8; 4] = b"KTE1";
pub const KTP_CRYPTO_VERSION: u8 = 1;
pub const KTP_CRYPTO_HEADER_LEN: usize = 24;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KtpStreamCodecError {
    Ktp(KtpError),
    BufferLimit { attempted: usize, limit: usize },
}

impl fmt::Display for KtpStreamCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ktp(error) => write!(f, "{error}"),
            Self::BufferLimit { attempted, limit } => {
                write!(
                    f,
                    "KTP stream buffer limit exceeded: attempted {attempted}, limit {limit}"
                )
            }
        }
    }
}

impl Error for KtpStreamCodecError {}

impl From<KtpError> for KtpStreamCodecError {
    fn from(error: KtpError) -> Self {
        Self::Ktp(error)
    }
}

#[derive(Clone, Debug)]
pub struct KtpStreamCodec {
    buffer: Vec<u8>,
    max_payload_len: usize,
    max_buffer_len: usize,
}

impl KtpStreamCodec {
    pub fn new(max_payload_len: usize, max_buffer_len: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_payload_len: max_payload_len.min(KTP_MAX_PAYLOAD_LEN),
            max_buffer_len,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), KtpStreamCodecError> {
        let attempted = self.buffer.len().saturating_add(chunk.len());
        if attempted > self.max_buffer_len {
            return Err(KtpStreamCodecError::BufferLimit {
                attempted,
                limit: self.max_buffer_len,
            });
        }
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<KtpFrame>, KtpStreamCodecError> {
        if self.buffer.len() < KTP_HEADER_LEN {
            return Ok(None);
        }

        if let Err(error) = decode_frame(&self.buffer[..KTP_HEADER_LEN], self.max_payload_len) {
            if !matches!(error, KtpError::TruncatedPayload) {
                return Err(error.into());
            }
        }

        let payload_len = u32::from_be_bytes(
            self.buffer[16..20]
                .try_into()
                .expect("KTP payload length slice is present"),
        ) as usize;
        if payload_len > self.max_payload_len {
            return Err(KtpError::PayloadTooLarge(payload_len).into());
        }

        let frame_len = KTP_HEADER_LEN + payload_len;
        if frame_len > self.max_buffer_len {
            return Err(KtpStreamCodecError::BufferLimit {
                attempted: frame_len,
                limit: self.max_buffer_len,
            });
        }
        if self.buffer.len() < frame_len {
            return Ok(None);
        }

        let frame = decode_frame(&self.buffer[..frame_len], self.max_payload_len)?;
        self.buffer.drain(..frame_len);
        Ok(Some(frame))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KtpCryptoDirection {
    ClientToRelay,
    RelayToClient,
}

impl KtpCryptoDirection {
    fn id(self) -> u8 {
        match self {
            Self::ClientToRelay => 1,
            Self::RelayToClient => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KtpCryptoKey([u8; 32]);

impl KtpCryptoKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KtpCryptoError {
    code: &'static str,
    message: String,
}

impl KtpCryptoError {
    pub fn code(&self) -> &'static str {
        self.code
    }

    fn auth_failed() -> Self {
        Self {
            code: "auth_failed",
            message: "KTP crypto record authentication failed".to_string(),
        }
    }

    fn malformed_record(message: impl Into<String>) -> Self {
        Self {
            code: "malformed_record",
            message: message.into(),
        }
    }

    fn buffer_limit(attempted: usize, limit: usize) -> Self {
        Self {
            code: "buffer_limit",
            message: format!(
                "KTP crypto buffer limit exceeded: attempted {attempted}, limit {limit}"
            ),
        }
    }

    fn ktp(error: KtpError) -> Self {
        Self {
            code: "ktp_error",
            message: error.to_string(),
        }
    }

    fn sequence_mismatch(expected: u64, actual: u64) -> Self {
        Self {
            code: "sequence_mismatch",
            message: format!("expected crypto sequence {expected}, got {actual}"),
        }
    }
}

impl fmt::Display for KtpCryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for KtpCryptoError {}

#[derive(Clone)]
pub struct KtpCryptoSeal {
    cipher: ChaCha20Poly1305,
    direction: KtpCryptoDirection,
    sequence: u64,
}

impl KtpCryptoSeal {
    pub fn new(key: KtpCryptoKey, direction: KtpCryptoDirection) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(&key.0)),
            direction,
            sequence: 0,
        }
    }

    pub fn seal_frame(&mut self, frame: &KtpFrame) -> Result<Vec<u8>, KtpCryptoError> {
        let plaintext = encode_frame(frame).map_err(KtpCryptoError::ktp)?;
        let sequence = self.sequence;
        let mut header = crypto_header(self.direction, sequence, plaintext.len() + 16);
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce(self.direction, sequence),
                Payload {
                    msg: &plaintext,
                    aad: &header,
                },
            )
            .map_err(|_| KtpCryptoError::auth_failed())?;
        let ciphertext_len = ciphertext.len() as u32;
        header[14..18].copy_from_slice(&ciphertext_len.to_be_bytes());
        self.sequence = self.sequence.wrapping_add(1);

        let mut record = header;
        record.extend_from_slice(&ciphertext);
        Ok(record)
    }
}

#[derive(Clone)]
pub struct KtpCryptoOpen {
    cipher: ChaCha20Poly1305,
    direction: KtpCryptoDirection,
    max_payload_len: usize,
    next_sequence: u64,
}

impl KtpCryptoOpen {
    pub fn new(key: KtpCryptoKey, direction: KtpCryptoDirection, max_payload_len: usize) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(Key::from_slice(&key.0)),
            direction,
            max_payload_len: max_payload_len.min(KTP_MAX_PAYLOAD_LEN),
            next_sequence: 0,
        }
    }

    pub fn open_record(&mut self, record: &[u8]) -> Result<KtpFrame, KtpCryptoError> {
        let header = parse_crypto_header(record, self.direction)?;
        if header.sequence != self.next_sequence {
            return Err(KtpCryptoError::sequence_mismatch(
                self.next_sequence,
                header.sequence,
            ));
        }
        let frame_len = KTP_CRYPTO_HEADER_LEN + header.ciphertext_len;
        if record.len() < frame_len {
            return Err(KtpCryptoError::malformed_record("truncated crypto record"));
        }
        let aad = &record[..KTP_CRYPTO_HEADER_LEN];
        let ciphertext = &record[KTP_CRYPTO_HEADER_LEN..frame_len];
        let plaintext = self
            .cipher
            .decrypt(
                &nonce(self.direction, header.sequence),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| KtpCryptoError::auth_failed())?;
        let frame = decode_frame(&plaintext, self.max_payload_len).map_err(KtpCryptoError::ktp)?;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        Ok(frame)
    }
}

#[derive(Clone)]
pub struct KtpCryptoRecordCodec {
    buffer: Vec<u8>,
    open: KtpCryptoOpen,
    max_buffer_len: usize,
}

impl KtpCryptoRecordCodec {
    pub fn new(
        key: KtpCryptoKey,
        direction: KtpCryptoDirection,
        max_payload_len: usize,
        max_buffer_len: usize,
    ) -> Self {
        Self {
            buffer: Vec::new(),
            open: KtpCryptoOpen::new(key, direction, max_payload_len),
            max_buffer_len,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), KtpCryptoError> {
        let attempted = self.buffer.len().saturating_add(chunk.len());
        if attempted > self.max_buffer_len {
            return Err(KtpCryptoError::buffer_limit(attempted, self.max_buffer_len));
        }
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<KtpFrame>, KtpCryptoError> {
        if self.buffer.len() < KTP_CRYPTO_HEADER_LEN {
            return Ok(None);
        }
        let header = parse_crypto_header(&self.buffer, self.open.direction)?;
        let record_len = KTP_CRYPTO_HEADER_LEN + header.ciphertext_len;
        if record_len > self.max_buffer_len {
            return Err(KtpCryptoError::buffer_limit(
                record_len,
                self.max_buffer_len,
            ));
        }
        if self.buffer.len() < record_len {
            return Ok(None);
        }
        let frame = self.open.open_record(&self.buffer[..record_len])?;
        self.buffer.drain(..record_len);
        Ok(Some(frame))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CryptoHeader {
    sequence: u64,
    ciphertext_len: usize,
}

fn crypto_header(direction: KtpCryptoDirection, sequence: u64, ciphertext_len: usize) -> Vec<u8> {
    let mut header = Vec::with_capacity(KTP_CRYPTO_HEADER_LEN);
    header.extend_from_slice(KTP_CRYPTO_MAGIC);
    header.push(KTP_CRYPTO_VERSION);
    header.push(direction.id());
    header.extend_from_slice(&sequence.to_be_bytes());
    header.extend_from_slice(&(ciphertext_len as u32).to_be_bytes());
    header.extend_from_slice(&[0u8; 6]);
    header
}

fn parse_crypto_header(
    record: &[u8],
    expected_direction: KtpCryptoDirection,
) -> Result<CryptoHeader, KtpCryptoError> {
    if record.len() < KTP_CRYPTO_HEADER_LEN {
        return Err(KtpCryptoError::malformed_record("truncated crypto header"));
    }
    if &record[0..4] != KTP_CRYPTO_MAGIC {
        return Err(KtpCryptoError::malformed_record("wrong crypto magic"));
    }
    if record[4] != KTP_CRYPTO_VERSION {
        return Err(KtpCryptoError::malformed_record(
            "unsupported crypto version",
        ));
    }
    if record[5] != expected_direction.id() {
        return Err(KtpCryptoError::malformed_record("wrong crypto direction"));
    }
    if record[18..24].iter().any(|value| *value != 0) {
        return Err(KtpCryptoError::malformed_record(
            "non-zero crypto reserved bytes",
        ));
    }

    let sequence = u64::from_be_bytes(
        record[6..14]
            .try_into()
            .expect("crypto sequence slice is present"),
    );
    let ciphertext_len = u32::from_be_bytes(
        record[14..18]
            .try_into()
            .expect("crypto length slice is present"),
    ) as usize;
    Ok(CryptoHeader {
        sequence,
        ciphertext_len,
    })
}

fn nonce(direction: KtpCryptoDirection, sequence: u64) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[0] = direction.id();
    bytes[4..12].copy_from_slice(&sequence.to_be_bytes());
    *Nonce::from_slice(&bytes)
}

#[derive(Debug)]
pub enum KtpTcpTransportError {
    Io(std::io::Error),
    Crypto(KtpCryptoError),
    Closed,
}

impl fmt::Display for KtpTcpTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Crypto(error) => write!(f, "{error}"),
            Self::Closed => write!(f, "KTP encrypted TCP stream closed"),
        }
    }
}

impl Error for KtpTcpTransportError {}

pub struct KtpEncryptedTcpStream {
    stream: TcpStream,
    seal: KtpCryptoSeal,
    codec: KtpCryptoRecordCodec,
    read_buffer: Vec<u8>,
}

impl KtpEncryptedTcpStream {
    pub fn from_stream(
        stream: TcpStream,
        key: KtpCryptoKey,
        seal_direction: KtpCryptoDirection,
        open_direction: KtpCryptoDirection,
        max_payload_len: usize,
        max_buffer_len: usize,
    ) -> Self {
        Self {
            stream,
            seal: KtpCryptoSeal::new(key.clone(), seal_direction),
            codec: KtpCryptoRecordCodec::new(key, open_direction, max_payload_len, max_buffer_len),
            read_buffer: vec![0u8; 16 * 1024],
        }
    }

    pub async fn send_frame(&mut self, frame: &KtpFrame) -> Result<(), KtpTcpTransportError> {
        let record = self
            .seal
            .seal_frame(frame)
            .map_err(KtpTcpTransportError::Crypto)?;
        self.stream
            .write_all(&record)
            .await
            .map_err(KtpTcpTransportError::Io)
    }

    pub async fn next_frame(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        loop {
            if let Some(frame) = self
                .codec
                .next_frame()
                .map_err(KtpTcpTransportError::Crypto)?
            {
                return Ok(frame);
            }
            let read = self
                .stream
                .read(&mut self.read_buffer)
                .await
                .map_err(KtpTcpTransportError::Io)?;
            if read == 0 {
                return Err(KtpTcpTransportError::Closed);
            }
            self.codec
                .push(&self.read_buffer[..read])
                .map_err(KtpTcpTransportError::Crypto)?;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KtpEncryptedTcpRelayStats {
    pub frames_left_to_right: u64,
    pub frames_right_to_left: u64,
}

pub struct KtpEncryptedTcpFrameRelay {
    left: KtpEncryptedTcpStream,
    right: KtpEncryptedTcpStream,
    stats: KtpEncryptedTcpRelayStats,
}

impl KtpEncryptedTcpFrameRelay {
    pub fn new(left: KtpEncryptedTcpStream, right: KtpEncryptedTcpStream) -> Self {
        Self {
            left,
            right,
            stats: KtpEncryptedTcpRelayStats::default(),
        }
    }

    pub fn stats(&self) -> KtpEncryptedTcpRelayStats {
        self.stats
    }

    pub async fn relay_next_left_to_right(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        let frame = self.left.next_frame().await?;
        self.right.send_frame(&frame).await?;
        self.stats.frames_left_to_right += 1;
        Ok(frame)
    }

    pub async fn relay_next_right_to_left(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        let frame = self.right.next_frame().await?;
        self.left.send_frame(&frame).await?;
        self.stats.frames_right_to_left += 1;
        Ok(frame)
    }

    pub async fn relay_bidirectional_rounds(
        &mut self,
        rounds: usize,
    ) -> Result<KtpEncryptedTcpRelayStats, KtpTcpTransportError> {
        for _ in 0..rounds {
            self.relay_next_left_to_right().await?;
            self.relay_next_right_to_left().await?;
        }
        Ok(self.stats)
    }
}
