use kelicloud_agent_rs::ktp::{
    encode_frame, FrameLeg, FrameType, KtpError, KtpFrame, KTP_HEADER_LEN, KTP_MAX_PAYLOAD_LEN,
    KTP_VERSION,
};
use kelicloud_agent_rs::ktp_transport::{
    KtpCryptoDirection, KtpCryptoKey, KtpCryptoOpen, KtpCryptoRecordCodec, KtpCryptoSeal,
    KtpEncryptedTcpFrameRelay, KtpEncryptedTcpStream, KtpStreamCodec, KtpStreamCodecError,
};
use tokio::net::{TcpListener, TcpStream};

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

#[test]
fn crypto_record_round_trips_ktp_frame_and_hides_plaintext() {
    let key = test_crypto_key();
    let frame = session_data(700, b"rdp payload bytes");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::ClientToRelay);
    let mut open = KtpCryptoOpen::new(key, KtpCryptoDirection::ClientToRelay, KTP_MAX_PAYLOAD_LEN);

    let record = seal.seal_frame(&frame).expect("seal frame");
    assert!(!record
        .windows(b"rdp payload bytes".len())
        .any(|window| window == b"rdp payload bytes"));

    let decoded = open.open_record(&record).expect("open record");
    assert_eq!(decoded, frame);
}

#[test]
fn crypto_record_rejects_tampered_ciphertext() {
    let key = test_crypto_key();
    let frame = session_data(701, b"secret");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::ClientToRelay);
    let mut open = KtpCryptoOpen::new(key, KtpCryptoDirection::ClientToRelay, KTP_MAX_PAYLOAD_LEN);
    let mut record = seal.seal_frame(&frame).expect("seal frame");
    let last = record.len() - 1;
    record[last] ^= 0x55;

    let err = open
        .open_record(&record)
        .expect_err("tampered record should fail auth");

    assert_eq!(err.code(), "auth_failed");
}

#[test]
fn crypto_record_rejects_out_of_order_sequence() {
    let key = test_crypto_key();
    let first = session_data(710, b"first");
    let second = session_data(711, b"second");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::ClientToRelay);
    let first_record = seal.seal_frame(&first).expect("seal first");
    let second_record = seal.seal_frame(&second).expect("seal second");
    let mut open = KtpCryptoOpen::new(key, KtpCryptoDirection::ClientToRelay, KTP_MAX_PAYLOAD_LEN);

    let err = open
        .open_record(&second_record)
        .expect_err("second sequence should not open before first");
    assert_eq!(err.code(), "sequence_mismatch");

    assert_eq!(open.open_record(&first_record).expect("open first"), first);
}

#[test]
fn crypto_record_codec_decodes_split_encrypted_records() {
    let key = test_crypto_key();
    let frame = session_data(702, b"chunked");
    let mut seal = KtpCryptoSeal::new(key.clone(), KtpCryptoDirection::RelayToClient);
    let record = seal.seal_frame(&frame).expect("seal frame");
    let mut codec = KtpCryptoRecordCodec::new(
        key,
        KtpCryptoDirection::RelayToClient,
        KTP_MAX_PAYLOAD_LEN,
        1024 * 1024,
    );

    codec.push(&record[..5]).expect("push first chunk");
    assert_eq!(codec.next_frame().expect("decode first chunk"), None);
    codec.push(&record[5..]).expect("push second chunk");

    assert_eq!(codec.next_frame().expect("decode frame"), Some(frame));
    assert_eq!(codec.next_frame().expect("decode empty"), None);
}

#[test]
fn encrypted_tcp_stream_round_trips_frame_over_loopback() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let key = test_crypto_key();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server_key = key.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept encrypted tcp");
            let mut server = KtpEncryptedTcpStream::from_stream(
                stream,
                server_key,
                KtpCryptoDirection::RelayToClient,
                KtpCryptoDirection::ClientToRelay,
                KTP_MAX_PAYLOAD_LEN,
                1024 * 1024,
            );
            let request = server.next_frame().await.expect("read request");
            assert_eq!(request, session_data(801, b"hello relay"));
            server
                .send_frame(&session_data(802, b"hello client"))
                .await
                .expect("send response");
        });

        let stream = TcpStream::connect(addr).await.expect("connect client");
        let mut client = KtpEncryptedTcpStream::from_stream(
            stream,
            key,
            KtpCryptoDirection::ClientToRelay,
            KtpCryptoDirection::RelayToClient,
            KTP_MAX_PAYLOAD_LEN,
            1024 * 1024,
        );
        client
            .send_frame(&session_data(801, b"hello relay"))
            .await
            .expect("send request");
        assert_eq!(
            client.next_frame().await.expect("read response"),
            session_data(802, b"hello client")
        );
        server.await.expect("server task");
    });
}

#[test]
fn encrypted_tcp_stream_handles_100_concurrent_loopback_round_trips() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(4)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let key = test_crypto_key();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server_key = key.clone();
        let server = tokio::spawn(async move {
            let mut handlers = Vec::new();
            for _ in 0..100 {
                let (stream, _) = listener.accept().await.expect("accept encrypted tcp");
                let key = server_key.clone();
                handlers.push(tokio::spawn(async move {
                    let mut server = KtpEncryptedTcpStream::from_stream(
                        stream,
                        key,
                        KtpCryptoDirection::RelayToClient,
                        KtpCryptoDirection::ClientToRelay,
                        KTP_MAX_PAYLOAD_LEN,
                        1024 * 1024,
                    );
                    let request = server.next_frame().await.expect("read request");
                    assert_eq!(request.payload, b"ping");
                    server
                        .send_frame(&session_data(request.session_id + 1000, b"pong"))
                        .await
                        .expect("send response");
                }));
            }
            for handler in handlers {
                handler.await.expect("server handler");
            }
        });

        let mut clients = Vec::new();
        for index in 0..100u64 {
            let key = key.clone();
            clients.push(tokio::spawn(async move {
                let stream = TcpStream::connect(addr).await.expect("connect client");
                let mut client = KtpEncryptedTcpStream::from_stream(
                    stream,
                    key,
                    KtpCryptoDirection::ClientToRelay,
                    KtpCryptoDirection::RelayToClient,
                    KTP_MAX_PAYLOAD_LEN,
                    1024 * 1024,
                );
                let session_id = 9000 + index;
                client
                    .send_frame(&session_data(session_id, b"ping"))
                    .await
                    .expect("send request");
                let response = client.next_frame().await.expect("read response");
                assert_eq!(response.session_id, session_id + 1000);
                assert_eq!(response.payload, b"pong");
            }));
        }

        for client in clients {
            client.await.expect("client task");
        }
        server.await.expect("server task");
    });
}

#[test]
fn encrypted_tcp_frame_relay_forwards_between_two_endpoints() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(4)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let key = test_crypto_key();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let relay_key = key.clone();
        let relay_task = tokio::spawn(async move {
            let (left_stream, _) = listener.accept().await.expect("accept left");
            let (right_stream, _) = listener.accept().await.expect("accept right");
            let left = KtpEncryptedTcpStream::from_stream(
                left_stream,
                relay_key.clone(),
                KtpCryptoDirection::RelayToClient,
                KtpCryptoDirection::ClientToRelay,
                KTP_MAX_PAYLOAD_LEN,
                1024 * 1024,
            );
            let right = KtpEncryptedTcpStream::from_stream(
                right_stream,
                relay_key,
                KtpCryptoDirection::RelayToClient,
                KtpCryptoDirection::ClientToRelay,
                KTP_MAX_PAYLOAD_LEN,
                1024 * 1024,
            );
            let mut relay = KtpEncryptedTcpFrameRelay::new(left, right);
            let forwarded = relay
                .relay_next_left_to_right()
                .await
                .expect("relay request");
            assert_eq!(forwarded.session_id, 1201);
            let forwarded = relay
                .relay_next_right_to_left()
                .await
                .expect("relay response");
            assert_eq!(forwarded.session_id, 1202);
            assert_eq!(relay.stats().frames_left_to_right, 1);
            assert_eq!(relay.stats().frames_right_to_left, 1);
        });

        let mut left_client = connect_encrypted_client(addr, key.clone()).await;
        let mut right_client = connect_encrypted_client(addr, key).await;
        left_client
            .send_frame(&session_data(1201, b"from left"))
            .await
            .expect("send left");
        assert_eq!(
            right_client.next_frame().await.expect("right receives"),
            session_data(1201, b"from left")
        );
        right_client
            .send_frame(&session_data(1202, b"from right"))
            .await
            .expect("send right");
        assert_eq!(
            left_client.next_frame().await.expect("left receives"),
            session_data(1202, b"from right")
        );
        relay_task.await.expect("relay task");
    });
}

#[test]
fn encrypted_tcp_frame_relay_handles_100_bidirectional_rounds() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(4)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let key = test_crypto_key();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let relay_key = key.clone();
        let relay_task = tokio::spawn(async move {
            let (left_stream, _) = listener.accept().await.expect("accept left");
            let (right_stream, _) = listener.accept().await.expect("accept right");
            let left = relay_side_stream(left_stream, relay_key.clone());
            let right = relay_side_stream(right_stream, relay_key);
            let mut relay = KtpEncryptedTcpFrameRelay::new(left, right);
            relay
                .relay_bidirectional_rounds(100)
                .await
                .expect("relay 100 rounds");
            assert_eq!(relay.stats().frames_left_to_right, 100);
            assert_eq!(relay.stats().frames_right_to_left, 100);
        });

        let mut left_client = connect_encrypted_client(addr, key.clone()).await;
        let mut right_client = connect_encrypted_client(addr, key).await;
        let left_task = tokio::spawn(async move {
            for index in 0..100u64 {
                left_client
                    .send_frame(&session_data(2200 + index, b"from left"))
                    .await
                    .expect("left send");
                let response = left_client.next_frame().await.expect("left receive");
                assert_eq!(response.session_id, 3200 + index);
                assert_eq!(response.payload, b"from right");
            }
        });
        let right_task = tokio::spawn(async move {
            for index in 0..100u64 {
                let request = right_client.next_frame().await.expect("right receive");
                assert_eq!(request.session_id, 2200 + index);
                assert_eq!(request.payload, b"from left");
                right_client
                    .send_frame(&session_data(3200 + index, b"from right"))
                    .await
                    .expect("right send");
            }
        });

        left_task.await.expect("left task");
        right_task.await.expect("right task");
        relay_task.await.expect("relay task");
    });
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

fn test_crypto_key() -> KtpCryptoKey {
    KtpCryptoKey::from_bytes([7u8; 32])
}

fn relay_side_stream(stream: TcpStream, key: KtpCryptoKey) -> KtpEncryptedTcpStream {
    KtpEncryptedTcpStream::from_stream(
        stream,
        key,
        KtpCryptoDirection::RelayToClient,
        KtpCryptoDirection::ClientToRelay,
        KTP_MAX_PAYLOAD_LEN,
        1024 * 1024,
    )
}

async fn connect_encrypted_client(
    addr: std::net::SocketAddr,
    key: KtpCryptoKey,
) -> KtpEncryptedTcpStream {
    let stream = TcpStream::connect(addr).await.expect("connect client");
    KtpEncryptedTcpStream::from_stream(
        stream,
        key,
        KtpCryptoDirection::ClientToRelay,
        KtpCryptoDirection::RelayToClient,
        KTP_MAX_PAYLOAD_LEN,
        1024 * 1024,
    )
}
