use kelicloud_agent_rs::ktp::{
    encode_frame, FrameLeg, FrameType, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN,
    KTP_VERSION,
};
use kelicloud_agent_rs::ktp_transport::{KtpStreamCodec, KtpStreamCodecError};

#[test]
fn stream_codec_decodes_frame_split_across_tcp_chunks() {
    let frame = session_data(42, b"hello");
    let bytes = encode_frame(&frame).expect("encode frame");
    let mut codec = KtpStreamCodec::new(KTP_MAX_PAYLOAD_LEN, 1024 * 1024);

    codec.push(&bytes[..7]).expect("push first chunk");
    assert_eq!(codec.next_frame().expect("decode first chunk"), None);

    codec.push(&bytes[7..]).expect("push second chunk");
    assert_eq!(codec.next_frame().expect("decode frame"), Some(frame));
    assert_eq!(codec.next_frame().expect("decode empty"), None);
}

#[test]
fn stream_codec_decodes_multiple_frames_from_one_chunk() {
    let first = session_data(7, b"one");
    let second = session_data(8, b"two");
    let mut bytes = encode_frame(&first).expect("encode first");
    bytes.extend_from_slice(&encode_frame(&second).expect("encode second"));
    let mut codec = KtpStreamCodec::new(KTP_MAX_PAYLOAD_LEN, 1024 * 1024);

    codec.push(&bytes).expect("push combined chunk");

    assert_eq!(codec.next_frame().expect("decode first"), Some(first));
    assert_eq!(codec.next_frame().expect("decode second"), Some(second));
    assert_eq!(codec.next_frame().expect("decode empty"), None);
}

#[test]
fn stream_codec_rejects_oversized_payload_from_header_before_body_arrives() {
    let mut header = Vec::new();
    header.extend_from_slice(b"KTP1");
    header.push(KTP_VERSION);
    header.push(FrameType::SessionData as u8);
    header.push(FrameLeg::Ingress as u8);
    header.push(0);
    header.extend_from_slice(&9u64.to_be_bytes());
    header.extend_from_slice(&11u32.to_be_bytes());
    header.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(header.len(), KTP_HEADER_LEN);
    let mut codec = KtpStreamCodec::new(10, 1024);

    codec.push(&header).expect("push oversized header");
    let err = codec
        .next_frame()
        .expect_err("oversized header should be rejected before payload body");

    assert_eq!(err, KtpStreamCodecError::Ktp(KtpError::PayloadTooLarge(11)));
}

#[test]
fn stream_codec_reports_malformed_header_before_payload_length_limit() {
    let mut header = Vec::new();
    header.extend_from_slice(b"BAD!");
    header.push(KTP_VERSION);
    header.push(FrameType::SessionData as u8);
    header.push(FrameLeg::Ingress as u8);
    header.push(0);
    header.extend_from_slice(&9u64.to_be_bytes());
    header.extend_from_slice(&(KTP_MAX_PAYLOAD_LEN as u32 + 1).to_be_bytes());
    header.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(header.len(), KTP_HEADER_LEN);
    let mut codec = KtpStreamCodec::new(10, 1024);

    codec.push(&header).expect("push malformed header");
    let err = codec
        .next_frame()
        .expect_err("malformed header should be rejected first");

    assert_eq!(err, KtpStreamCodecError::Ktp(KtpError::WrongMagic));
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
