use kelicloud_agent_rs::tunnel_session::{
    decode_session_accept_payload, decode_session_error_payload, decode_session_open_payload,
    encode_session_accept_payload, encode_session_error_payload, encode_session_open_payload,
    TunnelSessionErrorPayload, TunnelSessionOpenPayload, TunnelSessionPayloadError,
};

#[test]
fn encodes_and_decodes_session_open_payload() {
    let payload = TunnelSessionOpenPayload {
        rule_id: 7,
        listen_host: "127.0.0.1".to_string(),
        listen_port: 10088,
        source_addr: "127.0.0.1:50123".to_string(),
    };

    let bytes = encode_session_open_payload(&payload).expect("encode session open");
    let decoded = decode_session_open_payload(&bytes).expect("decode session open");

    assert_eq!(decoded, payload);
}

#[test]
fn session_open_payload_matches_backend_big_endian_layout() {
    let payload = TunnelSessionOpenPayload {
        rule_id: 7,
        listen_host: "0.0.0.0".to_string(),
        listen_port: 10088,
        source_addr: "127.0.0.1:50123".to_string(),
    };

    let bytes = encode_session_open_payload(&payload).expect("encode session open");

    assert_eq!(&bytes[0..8], &7u64.to_be_bytes());
    assert_eq!(&bytes[8..10], &7u16.to_be_bytes());
    assert_eq!(&bytes[10..17], b"0.0.0.0");
    assert_eq!(&bytes[17..19], &10088u16.to_be_bytes());
}

#[test]
fn encodes_and_decodes_session_accept_payload() {
    let bytes = encode_session_accept_payload(42);
    let rule_id = decode_session_accept_payload(&bytes).expect("decode accept payload");

    assert_eq!(rule_id, 42);
}

#[test]
fn encodes_and_decodes_session_error_payload() {
    let payload = TunnelSessionErrorPayload {
        rule_id: 42,
        code: "connect_failed".to_string(),
        message: "connection refused".to_string(),
    };

    let bytes = encode_session_error_payload(&payload).expect("encode error payload");
    let decoded = decode_session_error_payload(&bytes).expect("decode error payload");

    assert_eq!(decoded, payload);
}

#[test]
fn rejects_trailing_session_payload_bytes() {
    let mut bytes = encode_session_accept_payload(42);
    bytes.push(0);

    assert_eq!(
        decode_session_accept_payload(&bytes),
        Err(TunnelSessionPayloadError::TrailingBytes(1))
    );
}
