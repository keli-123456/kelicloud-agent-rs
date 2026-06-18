use crate::ktp::{
    append_frame_bytes, decode_frame, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN,
};
use chacha20poly1305::aead::{Aead, AeadInPlace, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::error::Error;
use std::fmt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const KTP_CRYPTO_MAGIC: &[u8; 4] = b"KTE1";
pub const KTP_CRYPTO_VERSION: u8 = 1;
pub const KTP_CRYPTO_HEADER_LEN: usize = 24;
const KTP_CODEC_COMPACT_MIN_PREFIX: usize = 64 * 1024;

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
    read_offset: usize,
    max_payload_len: usize,
    max_buffer_len: usize,
}

impl KtpStreamCodec {
    pub fn new(max_payload_len: usize, max_buffer_len: usize) -> Self {
        Self {
            buffer: Vec::new(),
            read_offset: 0,
            max_payload_len: max_payload_len.min(KTP_MAX_PAYLOAD_LEN),
            max_buffer_len,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), KtpStreamCodecError> {
        let attempted = self.unread_len().saturating_add(chunk.len());
        if attempted > self.max_buffer_len {
            return Err(KtpStreamCodecError::BufferLimit {
                attempted,
                limit: self.max_buffer_len,
            });
        }
        self.compact_if_needed_for_push(chunk.len());
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<KtpFrame>, KtpStreamCodecError> {
        let unread = self.unread();
        if unread.len() < KTP_HEADER_LEN {
            return Ok(None);
        }

        if let Err(error) = decode_frame(&unread[..KTP_HEADER_LEN], self.max_payload_len) {
            if !matches!(error, KtpError::TruncatedPayload) {
                return Err(error.into());
            }
        }

        let payload_len = u32::from_be_bytes(
            unread[16..20]
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
        if unread.len() < frame_len {
            return Ok(None);
        }

        let frame = decode_frame(&unread[..frame_len], self.max_payload_len)?;
        self.consume(frame_len);
        Ok(Some(frame))
    }

    fn unread(&self) -> &[u8] {
        &self.buffer[self.read_offset..]
    }

    fn unread_len(&self) -> usize {
        self.buffer.len().saturating_sub(self.read_offset)
    }

    fn compact_if_needed_for_push(&mut self, incoming_len: usize) {
        if self.read_offset > 0
            && self.buffer.len().saturating_add(incoming_len) > self.max_buffer_len
        {
            self.compact();
        }
    }

    fn consume(&mut self, len: usize) {
        self.read_offset += len;
        self.compact_after_consume();
    }

    fn compact_after_consume(&mut self) {
        if self.read_offset == 0 {
            return;
        }
        if self.read_offset == self.buffer.len() {
            self.buffer.clear();
            self.read_offset = 0;
            return;
        }
        if self.read_offset >= KTP_CODEC_COMPACT_MIN_PREFIX
            && self.read_offset.saturating_mul(2) >= self.buffer.len()
        {
            self.compact();
        }
    }

    fn compact(&mut self) {
        if self.read_offset == 0 {
            return;
        }
        self.buffer.drain(..self.read_offset);
        self.read_offset = 0;
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
        let mut record = Vec::new();
        self.seal_frame_into(frame, &mut record)?;
        Ok(record)
    }

    pub fn seal_frame_into(
        &mut self,
        frame: &KtpFrame,
        record: &mut Vec<u8>,
    ) -> Result<(), KtpCryptoError> {
        record.clear();
        self.append_sealed_frame(frame, record)
    }

    pub fn append_sealed_frame(
        &mut self,
        frame: &KtpFrame,
        record: &mut Vec<u8>,
    ) -> Result<(), KtpCryptoError> {
        let sequence = self.sequence;
        let record_start = record.len();
        let payload_start = record_start + KTP_CRYPTO_HEADER_LEN;
        record.resize(payload_start, 0);
        append_frame_bytes(frame, record).map_err(KtpCryptoError::ktp)?;
        let ciphertext_len = record.len() - payload_start + 16;
        let header = crypto_header_bytes(self.direction, sequence, ciphertext_len);
        record[record_start..payload_start].copy_from_slice(&header);
        let tag = self
            .cipher
            .encrypt_in_place_detached(
                &nonce(self.direction, sequence),
                &header,
                &mut record[payload_start..],
            )
            .map_err(|_| KtpCryptoError::auth_failed())?;
        record.extend_from_slice(&tag);
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
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
    read_offset: usize,
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
            read_offset: 0,
            open: KtpCryptoOpen::new(key, direction, max_payload_len),
            max_buffer_len,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), KtpCryptoError> {
        let attempted = self.unread_len().saturating_add(chunk.len());
        if attempted > self.max_buffer_len {
            return Err(KtpCryptoError::buffer_limit(attempted, self.max_buffer_len));
        }
        self.compact_if_needed_for_push(chunk.len());
        self.buffer.extend_from_slice(chunk);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<KtpFrame>, KtpCryptoError> {
        let unread = self.unread();
        if unread.len() < KTP_CRYPTO_HEADER_LEN {
            return Ok(None);
        }
        let header = parse_crypto_header(unread, self.open.direction)?;
        let record_len = KTP_CRYPTO_HEADER_LEN + header.ciphertext_len;
        if record_len > self.max_buffer_len {
            return Err(KtpCryptoError::buffer_limit(
                record_len,
                self.max_buffer_len,
            ));
        }
        if unread.len() < record_len {
            return Ok(None);
        }
        let record_start = self.read_offset;
        let record_end = record_start + record_len;
        let frame = self
            .open
            .open_record(&self.buffer[record_start..record_end])?;
        self.consume(record_len);
        Ok(Some(frame))
    }

    fn next_frames_into(
        &mut self,
        max_frames: usize,
        frames: &mut Vec<KtpFrame>,
    ) -> Result<usize, KtpCryptoError> {
        frames.clear();
        while frames.len() < max_frames {
            let Some(frame) = self.next_frame()? else {
                break;
            };
            frames.push(frame);
        }
        Ok(frames.len())
    }

    fn unread(&self) -> &[u8] {
        &self.buffer[self.read_offset..]
    }

    fn unread_len(&self) -> usize {
        self.buffer.len().saturating_sub(self.read_offset)
    }

    fn compact_if_needed_for_push(&mut self, incoming_len: usize) {
        if self.read_offset > 0
            && self.buffer.len().saturating_add(incoming_len) > self.max_buffer_len
        {
            self.compact();
        }
    }

    fn consume(&mut self, len: usize) {
        self.read_offset += len;
        self.compact_after_consume();
    }

    fn compact_after_consume(&mut self) {
        if self.read_offset == 0 {
            return;
        }
        if self.read_offset == self.buffer.len() {
            self.buffer.clear();
            self.read_offset = 0;
            return;
        }
        if self.read_offset >= KTP_CODEC_COMPACT_MIN_PREFIX
            && self.read_offset.saturating_mul(2) >= self.buffer.len()
        {
            self.compact();
        }
    }

    fn compact(&mut self) {
        if self.read_offset == 0 {
            return;
        }
        self.buffer.drain(..self.read_offset);
        self.read_offset = 0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CryptoHeader {
    sequence: u64,
    ciphertext_len: usize,
}

fn crypto_header_bytes(
    direction: KtpCryptoDirection,
    sequence: u64,
    ciphertext_len: usize,
) -> [u8; KTP_CRYPTO_HEADER_LEN] {
    let mut header = [0u8; KTP_CRYPTO_HEADER_LEN];
    header[0..4].copy_from_slice(KTP_CRYPTO_MAGIC);
    header[4] = KTP_CRYPTO_VERSION;
    header[5] = direction.id();
    header[6..14].copy_from_slice(&sequence.to_be_bytes());
    header[14..18].copy_from_slice(&(ciphertext_len as u32).to_be_bytes());
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
    write_buffer: Vec<u8>,
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
        let _ = stream.set_nodelay(true);
        Self {
            stream,
            seal: KtpCryptoSeal::new(key.clone(), seal_direction),
            codec: KtpCryptoRecordCodec::new(key, open_direction, max_payload_len, max_buffer_len),
            read_buffer: vec![0u8; 16 * 1024],
            write_buffer: Vec::with_capacity(
                KTP_CRYPTO_HEADER_LEN + KTP_HEADER_LEN + 16 * 1024 + 16,
            ),
        }
    }

    pub fn tcp_nodelay(&self) -> Result<bool, KtpTcpTransportError> {
        self.stream.nodelay().map_err(KtpTcpTransportError::Io)
    }

    pub async fn send_frame(&mut self, frame: &KtpFrame) -> Result<(), KtpTcpTransportError> {
        self.seal
            .seal_frame_into(frame, &mut self.write_buffer)
            .map_err(KtpTcpTransportError::Crypto)?;
        self.stream
            .write_all(&self.write_buffer)
            .await
            .map_err(KtpTcpTransportError::Io)
    }

    pub async fn send_frames(&mut self, frames: &[KtpFrame]) -> Result<(), KtpTcpTransportError> {
        if frames.is_empty() {
            return Ok(());
        }
        self.write_buffer.clear();
        for frame in frames {
            self.seal
                .append_sealed_frame(frame, &mut self.write_buffer)
                .map_err(KtpTcpTransportError::Crypto)?;
        }
        self.stream
            .write_all(&self.write_buffer)
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

    pub async fn next_frames(
        &mut self,
        max_frames: usize,
    ) -> Result<Vec<KtpFrame>, KtpTcpTransportError> {
        let mut frames = Vec::new();
        self.next_frames_into(max_frames, &mut frames).await?;
        Ok(frames)
    }

    pub async fn next_frames_into(
        &mut self,
        max_frames: usize,
        frames: &mut Vec<KtpFrame>,
    ) -> Result<usize, KtpTcpTransportError> {
        frames.clear();
        if max_frames == 0 {
            return Ok(0);
        }

        loop {
            let frame_count = self
                .codec
                .next_frames_into(max_frames, frames)
                .map_err(KtpTcpTransportError::Crypto)?;
            if frame_count > 0 {
                return Ok(frame_count);
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
    pub batches_left_to_right: u64,
    pub batches_right_to_left: u64,
    pub max_batch_frames_left_to_right: u64,
    pub max_batch_frames_right_to_left: u64,
}

impl KtpEncryptedTcpRelayStats {
    fn record_left_to_right_batch(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.frames_left_to_right += frames as u64;
        self.batches_left_to_right += 1;
        self.max_batch_frames_left_to_right =
            self.max_batch_frames_left_to_right.max(frames as u64);
    }

    fn record_right_to_left_batch(&mut self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.frames_right_to_left += frames as u64;
        self.batches_right_to_left += 1;
        self.max_batch_frames_right_to_left =
            self.max_batch_frames_right_to_left.max(frames as u64);
    }
}

pub struct KtpEncryptedTcpFrameRelay {
    left: KtpEncryptedTcpStream,
    right: KtpEncryptedTcpStream,
    left_batch: Vec<KtpFrame>,
    right_batch: Vec<KtpFrame>,
    stats: KtpEncryptedTcpRelayStats,
}

const KTP_RELAY_BATCH_FRAMES: usize = 64;

impl KtpEncryptedTcpFrameRelay {
    pub fn new(left: KtpEncryptedTcpStream, right: KtpEncryptedTcpStream) -> Self {
        Self {
            left,
            right,
            left_batch: Vec::with_capacity(KTP_RELAY_BATCH_FRAMES),
            right_batch: Vec::with_capacity(KTP_RELAY_BATCH_FRAMES),
            stats: KtpEncryptedTcpRelayStats::default(),
        }
    }

    pub fn stats(&self) -> KtpEncryptedTcpRelayStats {
        self.stats
    }

    pub async fn relay_next_left_to_right(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        let frame = self.left.next_frame().await?;
        self.right.send_frame(&frame).await?;
        self.stats.record_left_to_right_batch(1);
        Ok(frame)
    }

    pub async fn relay_next_right_to_left(&mut self) -> Result<KtpFrame, KtpTcpTransportError> {
        let frame = self.right.next_frame().await?;
        self.left.send_frame(&frame).await?;
        self.stats.record_right_to_left_batch(1);
        Ok(frame)
    }

    async fn relay_next_left_to_right_batch(
        &mut self,
        max_frames: usize,
    ) -> Result<usize, KtpTcpTransportError> {
        let frame_count = self
            .left
            .next_frames_into(max_frames, &mut self.left_batch)
            .await?;
        self.right.send_frames(&self.left_batch).await?;
        self.stats.record_left_to_right_batch(frame_count);
        Ok(frame_count)
    }

    async fn relay_next_right_to_left_batch(
        &mut self,
        max_frames: usize,
    ) -> Result<usize, KtpTcpTransportError> {
        let frame_count = self
            .right
            .next_frames_into(max_frames, &mut self.right_batch)
            .await?;
        self.left.send_frames(&self.right_batch).await?;
        self.stats.record_right_to_left_batch(frame_count);
        Ok(frame_count)
    }

    pub async fn relay_bidirectional_rounds(
        &mut self,
        rounds: usize,
    ) -> Result<KtpEncryptedTcpRelayStats, KtpTcpTransportError> {
        for _ in 0..rounds {
            self.relay_next_left_to_right_batch(KTP_RELAY_BATCH_FRAMES)
                .await?;
            self.relay_next_right_to_left_batch(KTP_RELAY_BATCH_FRAMES)
                .await?;
        }
        Ok(self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ktp::{encode_frame, FrameLeg, FrameType};

    #[test]
    fn stream_codec_advances_cursor_without_draining_every_frame() {
        let first = session_data(1, b"first");
        let second = session_data(2, b"second");
        let mut bytes = encode_frame(&first).expect("encode first");
        bytes.extend_from_slice(&encode_frame(&second).expect("encode second"));
        let mut codec = KtpStreamCodec::new(KTP_MAX_PAYLOAD_LEN, 1024 * 1024);

        codec.push(&bytes).expect("push combined frames");
        let initial_buffer_len = codec.buffer.len();

        assert_eq!(codec.next_frame().expect("decode first"), Some(first));
        assert!(codec.read_offset > 0);
        assert_eq!(codec.buffer.len(), initial_buffer_len);

        assert_eq!(codec.next_frame().expect("decode second"), Some(second));
        assert_eq!(codec.read_offset, 0);
        assert_eq!(codec.buffer.len(), 0);
    }

    #[test]
    fn crypto_record_codec_advances_cursor_without_draining_every_record() {
        let key = KtpCryptoKey::from_bytes([9u8; 32]);
        let first = session_data(11, b"first");
        let second = session_data(12, b"second");
        let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::ClientToRelay);
        let mut records = seal.seal_frame(&first).expect("seal first");
        records.extend_from_slice(&seal.seal_frame(&second).expect("seal second"));
        let mut codec = KtpCryptoRecordCodec::new(
            key,
            KtpCryptoDirection::ClientToRelay,
            KTP_MAX_PAYLOAD_LEN,
            1024 * 1024,
        );

        codec.push(&records).expect("push combined records");
        let initial_buffer_len = codec.buffer.len();

        assert_eq!(codec.next_frame().expect("decode first"), Some(first));
        assert!(codec.read_offset > 0);
        assert_eq!(codec.buffer.len(), initial_buffer_len);

        assert_eq!(codec.next_frame().expect("decode second"), Some(second));
        assert_eq!(codec.read_offset, 0);
        assert_eq!(codec.buffer.len(), 0);
    }

    fn session_data(session_id: u64, payload: &[u8]) -> KtpFrame {
        KtpFrame {
            frame_type: FrameType::SessionData,
            leg: FrameLeg::Ingress,
            flags: 0,
            session_id,
            payload: payload.to_vec(),
        }
    }
}
