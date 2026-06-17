use std::error::Error;
use std::fmt;

pub const KTP_MAGIC: &[u8; 4] = b"KTP1";
pub const KTP_VERSION: u8 = 1;
pub const KTP_HEADER_LEN: usize = 24;
pub const KTP_MAX_PAYLOAD_LEN: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FrameType {
    Hello = 0x01,
    HelloAck = 0x02,
    Ready = 0x03,
    SessionOpen = 0x10,
    SessionAccept = 0x11,
    SessionData = 0x12,
    SessionWindow = 0x13,
    SessionClose = 0x14,
    SessionError = 0x15,
    Ping = 0x20,
    Pong = 0x21,
    Stats = 0x30,
}

impl FrameType {
    fn from_u8(value: u8) -> Result<Self, KtpError> {
        match value {
            0x01 => Ok(Self::Hello),
            0x02 => Ok(Self::HelloAck),
            0x03 => Ok(Self::Ready),
            0x10 => Ok(Self::SessionOpen),
            0x11 => Ok(Self::SessionAccept),
            0x12 => Ok(Self::SessionData),
            0x13 => Ok(Self::SessionWindow),
            0x14 => Ok(Self::SessionClose),
            0x15 => Ok(Self::SessionError),
            0x20 => Ok(Self::Ping),
            0x21 => Ok(Self::Pong),
            0x30 => Ok(Self::Stats),
            other => Err(KtpError::UnknownFrameType(other)),
        }
    }

    fn is_connection_level(self) -> bool {
        matches!(
            self,
            Self::Hello | Self::HelloAck | Self::Ready | Self::Ping | Self::Pong | Self::Stats
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FrameLeg {
    Connection = 0,
    Ingress = 1,
    Egress = 2,
}

impl FrameLeg {
    fn from_u8(value: u8) -> Result<Self, KtpError> {
        match value {
            0 => Ok(Self::Connection),
            1 => Ok(Self::Ingress),
            2 => Ok(Self::Egress),
            other => Err(KtpError::InvalidLeg(other)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KtpFrame {
    pub frame_type: FrameType,
    pub leg: FrameLeg,
    pub flags: u8,
    pub session_id: u64,
    pub payload: Vec<u8>,
}

impl KtpFrame {
    pub fn connection(frame_type: FrameType, payload: Vec<u8>) -> Self {
        Self {
            frame_type,
            leg: FrameLeg::Connection,
            flags: 0,
            session_id: 0,
            payload,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KtpError {
    WrongMagic,
    UnsupportedVersion(u8),
    UnknownFrameType(u8),
    InvalidLeg(u8),
    InvalidSessionId,
    TruncatedHeader,
    TruncatedPayload,
    PayloadTooLarge(usize),
    ReservedNonZero(u32),
}

impl fmt::Display for KtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongMagic => write!(f, "wrong KTP magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported KTP version {version}")
            }
            Self::UnknownFrameType(frame_type) => {
                write!(f, "unknown KTP frame type {frame_type:#04x}")
            }
            Self::InvalidLeg(leg) => write!(f, "invalid KTP frame leg {leg}"),
            Self::InvalidSessionId => write!(f, "invalid KTP session id"),
            Self::TruncatedHeader => write!(f, "truncated KTP header"),
            Self::TruncatedPayload => write!(f, "truncated KTP payload"),
            Self::PayloadTooLarge(size) => write!(f, "KTP payload too large: {size} bytes"),
            Self::ReservedNonZero(value) => {
                write!(f, "KTP reserved header field is non-zero: {value}")
            }
        }
    }
}

impl Error for KtpError {}

pub fn encode_frame(frame: &KtpFrame) -> Result<Vec<u8>, KtpError> {
    let mut bytes = Vec::with_capacity(KTP_HEADER_LEN + frame.payload.len());
    encode_frame_into(frame, &mut bytes)?;
    Ok(bytes)
}

pub fn encode_frame_into(frame: &KtpFrame, bytes: &mut Vec<u8>) -> Result<(), KtpError> {
    bytes.clear();
    append_frame_bytes(frame, bytes)
}

pub(crate) fn append_frame_bytes(frame: &KtpFrame, bytes: &mut Vec<u8>) -> Result<(), KtpError> {
    validate_frame(frame)?;

    let payload_len = frame.payload.len();
    if payload_len > KTP_MAX_PAYLOAD_LEN {
        return Err(KtpError::PayloadTooLarge(payload_len));
    }

    bytes.extend_from_slice(KTP_MAGIC);
    bytes.push(KTP_VERSION);
    bytes.push(frame.frame_type as u8);
    bytes.push(frame.leg as u8);
    bytes.push(frame.flags);
    bytes.extend_from_slice(&frame.session_id.to_be_bytes());
    bytes.extend_from_slice(&(payload_len as u32).to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&frame.payload);
    Ok(())
}

pub fn decode_frame(bytes: &[u8], max_payload_len: usize) -> Result<KtpFrame, KtpError> {
    let effective_max_payload_len = max_payload_len.min(KTP_MAX_PAYLOAD_LEN);

    if bytes.len() < KTP_HEADER_LEN {
        return Err(KtpError::TruncatedHeader);
    }

    if &bytes[0..4] != KTP_MAGIC {
        return Err(KtpError::WrongMagic);
    }

    let version = bytes[4];
    if version != KTP_VERSION {
        return Err(KtpError::UnsupportedVersion(version));
    }

    let frame_type = FrameType::from_u8(bytes[5])?;
    let leg = FrameLeg::from_u8(bytes[6])?;
    let flags = bytes[7];
    let session_id = u64::from_be_bytes(bytes[8..16].try_into().expect("valid session id slice"));
    let payload_len = u32::from_be_bytes(
        bytes[16..20]
            .try_into()
            .expect("valid payload length slice"),
    ) as usize;
    let reserved = u32::from_be_bytes(bytes[20..24].try_into().expect("valid reserved slice"));

    if reserved != 0 {
        return Err(KtpError::ReservedNonZero(reserved));
    }

    if payload_len > effective_max_payload_len {
        return Err(KtpError::PayloadTooLarge(payload_len));
    }

    if bytes.len() < KTP_HEADER_LEN + payload_len {
        return Err(KtpError::TruncatedPayload);
    }

    let frame = KtpFrame {
        frame_type,
        leg,
        flags,
        session_id,
        payload: bytes[KTP_HEADER_LEN..KTP_HEADER_LEN + payload_len].to_vec(),
    };
    validate_frame(&frame)?;

    Ok(frame)
}

fn validate_frame(frame: &KtpFrame) -> Result<(), KtpError> {
    if frame.frame_type.is_connection_level() {
        if frame.leg != FrameLeg::Connection {
            return Err(KtpError::InvalidLeg(frame.leg as u8));
        }
        if frame.session_id != 0 {
            return Err(KtpError::InvalidSessionId);
        }
    } else {
        if frame.leg == FrameLeg::Connection {
            return Err(KtpError::InvalidLeg(frame.leg as u8));
        }
        if frame.session_id == 0 {
            return Err(KtpError::InvalidSessionId);
        }
    }

    Ok(())
}
