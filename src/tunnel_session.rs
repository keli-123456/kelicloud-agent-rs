use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelSessionOpenPayload {
    pub rule_id: u64,
    pub listen_host: String,
    pub listen_port: u16,
    pub source_addr: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelSessionErrorPayload {
    pub rule_id: u64,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TunnelSessionPayloadError {
    Truncated(&'static str),
    StringTooLong(&'static str),
    InvalidUtf8(&'static str),
    TrailingBytes(usize),
}

impl fmt::Display for TunnelSessionPayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(field) => write!(f, "truncated tunnel session payload field: {field}"),
            Self::StringTooLong(field) => {
                write!(f, "tunnel session payload string too long: {field}")
            }
            Self::InvalidUtf8(field) => write!(f, "invalid utf-8 tunnel session field: {field}"),
            Self::TrailingBytes(count) => {
                write!(f, "tunnel session payload has {count} trailing bytes")
            }
        }
    }
}

impl Error for TunnelSessionPayloadError {}

pub fn encode_session_open_payload(
    payload: &TunnelSessionOpenPayload,
) -> Result<Vec<u8>, TunnelSessionPayloadError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&payload.rule_id.to_be_bytes());
    write_string(&mut bytes, "listen_host", &payload.listen_host)?;
    bytes.extend_from_slice(&payload.listen_port.to_be_bytes());
    write_string(&mut bytes, "source_addr", &payload.source_addr)?;
    Ok(bytes)
}

pub fn decode_session_open_payload(
    bytes: &[u8],
) -> Result<TunnelSessionOpenPayload, TunnelSessionPayloadError> {
    let mut cursor = PayloadCursor::new(bytes);
    let payload = TunnelSessionOpenPayload {
        rule_id: cursor.read_u64("rule_id")?,
        listen_host: cursor.read_string("listen_host")?,
        listen_port: cursor.read_u16("listen_port")?,
        source_addr: cursor.read_string("source_addr")?,
    };
    cursor.expect_end()?;
    Ok(payload)
}

pub fn encode_session_accept_payload(rule_id: u64) -> Vec<u8> {
    rule_id.to_be_bytes().to_vec()
}

pub fn decode_session_accept_payload(bytes: &[u8]) -> Result<u64, TunnelSessionPayloadError> {
    let mut cursor = PayloadCursor::new(bytes);
    let rule_id = cursor.read_u64("rule_id")?;
    cursor.expect_end()?;
    Ok(rule_id)
}

pub fn encode_session_error_payload(
    payload: &TunnelSessionErrorPayload,
) -> Result<Vec<u8>, TunnelSessionPayloadError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&payload.rule_id.to_be_bytes());
    write_string(&mut bytes, "code", &payload.code)?;
    write_string(&mut bytes, "message", &payload.message)?;
    Ok(bytes)
}

pub fn decode_session_error_payload(
    bytes: &[u8],
) -> Result<TunnelSessionErrorPayload, TunnelSessionPayloadError> {
    let mut cursor = PayloadCursor::new(bytes);
    let payload = TunnelSessionErrorPayload {
        rule_id: cursor.read_u64("rule_id")?,
        code: cursor.read_string("code")?,
        message: cursor.read_string("message")?,
    };
    cursor.expect_end()?;
    Ok(payload)
}

fn write_string(
    bytes: &mut Vec<u8>,
    field: &'static str,
    value: &str,
) -> Result<(), TunnelSessionPayloadError> {
    let len =
        u16::try_from(value.len()).map_err(|_| TunnelSessionPayloadError::StringTooLong(field))?;
    bytes.extend_from_slice(&len.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_u64(&mut self, field: &'static str) -> Result<u64, TunnelSessionPayloadError> {
        let end = self.offset + 8;
        if self.bytes.len() < end {
            return Err(TunnelSessionPayloadError::Truncated(field));
        }
        let value = u64::from_be_bytes(
            self.bytes[self.offset..end]
                .try_into()
                .expect("u64 slice length checked"),
        );
        self.offset = end;
        Ok(value)
    }

    fn read_u16(&mut self, field: &'static str) -> Result<u16, TunnelSessionPayloadError> {
        let end = self.offset + 2;
        if self.bytes.len() < end {
            return Err(TunnelSessionPayloadError::Truncated(field));
        }
        let value = u16::from_be_bytes(
            self.bytes[self.offset..end]
                .try_into()
                .expect("u16 slice length checked"),
        );
        self.offset = end;
        Ok(value)
    }

    fn read_string(&mut self, field: &'static str) -> Result<String, TunnelSessionPayloadError> {
        let len = self.read_u16(field)? as usize;
        let end = self.offset + len;
        if self.bytes.len() < end {
            return Err(TunnelSessionPayloadError::Truncated(field));
        }
        let value = std::str::from_utf8(&self.bytes[self.offset..end])
            .map_err(|_| TunnelSessionPayloadError::InvalidUtf8(field))?
            .to_string();
        self.offset = end;
        Ok(value)
    }

    fn expect_end(&self) -> Result<(), TunnelSessionPayloadError> {
        let trailing = self.bytes.len().saturating_sub(self.offset);
        if trailing != 0 {
            return Err(TunnelSessionPayloadError::TrailingBytes(trailing));
        }
        Ok(())
    }
}
