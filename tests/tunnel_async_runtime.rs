use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_async_runtime::{
    AsyncTunnelCore, AsyncTunnelFrameQueue, TunnelIngressListenerSpec, TunnelRuntimeLimits,
    TunnelRuntimeStats,
};
use kelicloud_agent_rs::tunnel_session::{encode_session_open_payload, TunnelSessionOpenPayload};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream as TokioTcpStream;

#[test]
fn async_runtime_limits_have_bounded_defaults() {
    let limits = TunnelRuntimeLimits::default();

    assert_eq!(limits.max_sessions_per_agent, 1024);
    assert_eq!(limits.max_outbound_frames, 4096);
    assert_eq!(limits.max_session_pending_bytes, 4 * 1024 * 1024);
    assert_eq!(limits.tcp_read_chunk_size, 16 * 1024);
    assert!(limits.target_dial_timeout.as_secs() <= 5);
    assert!(limits.idle_timeout.as_secs() >= 600);
}

#[test]
fn async_frame_queue_enforces_frame_capacity() {
    let queue = AsyncTunnelFrameQueue::new(2);
    queue
        .try_push(frame(1, b"a"))
        .expect("first frame should fit");
    queue
        .try_push(frame(2, b"b"))
        .expect("second frame should fit");

    let err = queue
        .try_push(frame(3, b"c"))
        .expect_err("third frame should exceed capacity");

    assert_eq!(err.code(), "backpressure_limit");
    assert_eq!(queue.len(), 2);
}

#[test]
fn runtime_stats_snapshot_tracks_session_and_byte_counters() {
    let stats = TunnelRuntimeStats::default();
    stats.session_opened(7);
    stats.bytes_in(7, 12);
    stats.bytes_out(7, 34);
    stats.session_closed(7);

    let snapshot = stats.snapshot();

    assert_eq!(snapshot.active_sessions, 0);
    assert_eq!(snapshot.total_sessions, 1);
    assert_eq!(snapshot.bytes_in, 12);
    assert_eq!(snapshot.bytes_out, 34);
    assert_eq!(snapshot.rule_session_counts.get(&7).copied(), Some(0));
}

#[test]
fn async_egress_session_connects_target_and_queues_response() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let target = TcpListener::bind("127.0.0.1:0").expect("bind target echo listener");
        let target_addr = target.local_addr().expect("target local addr");
        let echo_thread = thread::spawn(move || {
            let (mut stream, _) = target.accept().expect("accept target connection");
            let mut buffer = [0u8; 16];
            let read = stream.read(&mut buffer).expect("read target input");
            assert_eq!(&buffer[..read], b"ping");
            stream.write_all(b"pong").expect("write target output");
        });

        let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
        let responses = core
            .open_egress_session(
                77,
                7,
                "127.0.0.1",
                target_addr.port(),
                session_open_payload(7),
            )
            .await
            .expect("open egress session");

        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].frame_type, FrameType::SessionAccept);
        assert_eq!(responses[0].leg, FrameLeg::Egress);

        core.handle_session_data(77, FrameLeg::Egress, b"ping".to_vec())
            .await
            .expect("write target data");

        let frame = wait_for_core_frame(&core)
            .await
            .expect("target response frame");
        assert_eq!(frame.frame_type, FrameType::SessionData);
        assert_eq!(frame.leg, FrameLeg::Egress);
        assert_eq!(frame.session_id, 77);
        assert_eq!(frame.payload, b"pong");
        echo_thread.join().expect("echo thread should finish");
    });
}

#[test]
fn async_ingress_listener_queues_open_data_and_writes_response() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let listen_port = free_tcp_port();
        let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
        core.start_ingress_listener(TunnelIngressListenerSpec {
            rule_id: 17,
            listen_address: "127.0.0.1".to_string(),
            listen_port,
            source_allowlist: "127.0.0.0/8".to_string(),
        })
        .await
        .expect("start ingress listener");

        let mut client = connect_tokio_with_retry("127.0.0.1", listen_port).await;
        client.write_all(b"hello").await.expect("write client data");

        let open = wait_for_core_frame(&core).await.expect("open frame");
        assert_eq!(open.frame_type, FrameType::SessionOpen);
        assert_eq!(open.leg, FrameLeg::Ingress);
        assert_ne!(open.session_id, 0);

        let data = wait_for_core_frame(&core).await.expect("data frame");
        assert_eq!(data.frame_type, FrameType::SessionData);
        assert_eq!(data.leg, FrameLeg::Ingress);
        assert_eq!(data.session_id, open.session_id);
        assert_eq!(data.payload, b"hello");

        core.handle_session_data(open.session_id, FrameLeg::Ingress, b"world".to_vec())
            .await
            .expect("write response to client");

        let mut buffer = [0u8; 16];
        let read = client
            .read(&mut buffer)
            .await
            .expect("read client response");
        assert_eq!(&buffer[..read], b"world");
    });
}

fn frame(session_id: u64, payload: &[u8]) -> KtpFrame {
    KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 0,
        session_id,
        payload: payload.to_vec(),
    }
}

fn free_tcp_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().expect("local addr").port()
}

fn session_open_payload(rule_id: u64) -> Vec<u8> {
    encode_session_open_payload(&TunnelSessionOpenPayload {
        rule_id,
        listen_host: "127.0.0.1".to_string(),
        listen_port: 10088,
        source_addr: "127.0.0.1:50123".to_string(),
    })
    .expect("encode session open")
}

async fn wait_for_core_frame(core: &AsyncTunnelCore) -> Option<KtpFrame> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(frame) = core.next_frame().await {
            return Some(frame);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

async fn connect_tokio_with_retry(host: &str, port: u16) -> TokioTcpStream {
    let addr = format!("{host}:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match TokioTcpStream::connect(&addr).await {
            Ok(stream) => return stream,
            Err(error) if std::time::Instant::now() < deadline => {
                let _ = error;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(error) => panic!("connect {addr}: {error}"),
        }
    }
}
