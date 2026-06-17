use crate::ktp::{decode_frame, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN};
use std::error::Error;
use std::fmt;

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
