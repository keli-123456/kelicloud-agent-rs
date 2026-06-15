use kelicloud_agent_rs::ktp::{
    decode_frame, encode_frame, FrameLeg, FrameType, KtpError, KtpFrame, KTP_HEADER_LEN,
    KTP_MAX_PAYLOAD_LEN, KTP_VERSION,
};

#[test]
fn encodes_and_decodes_hello_connection_frame() {
    let frame = KtpFrame::connection(FrameType::Hello, b"agent".to_vec());

    let bytes = encode_frame(&frame).expect("encode hello");

    assert_eq!(&bytes[0..4], b"KTP1");
    assert_eq!(bytes[4], KTP_VERSION);
    assert_eq!(bytes.len(), KTP_HEADER_LEN + 5);

    let decoded = decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN).expect("decode hello");

    assert_eq!(decoded.frame_type, FrameType::Hello);
    assert_eq!(decoded.leg, FrameLeg::Connection);
    assert_eq!(decoded.flags, 0);
    assert_eq!(decoded.session_id, 0);
    assert_eq!(decoded.payload, b"agent");
}

#[test]
fn preserves_session_data_payload_leg_flags_and_session_id() {
    let frame = KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 0b1010_0001,
        session_id: 42,
        payload: b"hello session".to_vec(),
    };

    let bytes = encode_frame(&frame).expect("encode session data");
    let decoded = decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN).expect("decode session data");

    assert_eq!(decoded.frame_type, FrameType::SessionData);
    assert_eq!(decoded.leg, FrameLeg::Ingress);
    assert_eq!(decoded.flags, 0b1010_0001);
    assert_eq!(decoded.session_id, 42);
    assert_eq!(decoded.payload, b"hello session");
}

#[test]
fn rejects_wrong_magic() {
    let mut bytes = encoded_hello();
    bytes[0..4].copy_from_slice(b"NOPE");

    assert_eq!(
        decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN),
        Err(KtpError::WrongMagic)
    );
}

#[test]
fn rejects_unsupported_version() {
    let mut bytes = encoded_hello();
    bytes[4] = KTP_VERSION + 1;

    assert_eq!(
        decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN),
        Err(KtpError::UnsupportedVersion(KTP_VERSION + 1))
    );
}

#[test]
fn rejects_invalid_connection_leg_for_session_frame() {
    let mut bytes = encoded_session_data();
    bytes[6] = FrameLeg::Connection as u8;

    assert_eq!(
        decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN),
        Err(KtpError::InvalidLeg(FrameLeg::Connection as u8))
    );
}

#[test]
fn rejects_truncated_header_and_truncated_payload() {
    let bytes = encoded_hello();

    assert_eq!(
        decode_frame(&bytes[..KTP_HEADER_LEN - 1], KTP_MAX_PAYLOAD_LEN),
        Err(KtpError::TruncatedHeader)
    );

    assert_eq!(
        decode_frame(&bytes[..bytes.len() - 1], KTP_MAX_PAYLOAD_LEN),
        Err(KtpError::TruncatedPayload)
    );
}

#[test]
fn rejects_payload_above_max_limit() {
    let bytes = encoded_hello();

    assert_eq!(decode_frame(&bytes, 4), Err(KtpError::PayloadTooLarge(5)));
}

#[test]
fn rejects_non_zero_reserved_header_field() {
    let mut bytes =
        encode_frame(&KtpFrame::connection(FrameType::Ping, Vec::new())).expect("encode ping");
    bytes[20..24].copy_from_slice(&1u32.to_be_bytes());

    assert_eq!(
        decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN).unwrap_err(),
        KtpError::ReservedNonZero(1)
    );
}

#[test]
fn decode_rejects_payload_above_protocol_max_even_when_caller_limit_is_higher() {
    let payload_len = KTP_MAX_PAYLOAD_LEN + 1;
    let mut bytes = Vec::with_capacity(KTP_HEADER_LEN + payload_len);
    bytes.extend_from_slice(b"KTP1");
    bytes.push(KTP_VERSION);
    bytes.push(FrameType::Ping as u8);
    bytes.push(FrameLeg::Connection as u8);
    bytes.push(0);
    bytes.extend_from_slice(&0u64.to_be_bytes());
    bytes.extend_from_slice(&(payload_len as u32).to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&vec![0; payload_len]);

    assert_eq!(
        decode_frame(&bytes, KTP_MAX_PAYLOAD_LEN + 1024),
        Err(KtpError::PayloadTooLarge(payload_len))
    );
}

fn encoded_hello() -> Vec<u8> {
    encode_frame(&KtpFrame::connection(FrameType::Hello, b"agent".to_vec())).expect("encode hello")
}

fn encoded_session_data() -> Vec<u8> {
    encode_frame(&KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Egress,
        flags: 7,
        session_id: 9,
        payload: b"payload".to_vec(),
    })
    .expect("encode session data")
}
