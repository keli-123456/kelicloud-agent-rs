use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_async_runtime::{
    AsyncTunnelCore, AsyncTunnelFrameQueue, TunnelFrameReadyNotifier, TunnelIngressListenerSpec,
    TunnelQueueDwellStatsSnapshot, TunnelRelayBatchPolicy, TunnelRuntimeLimits, TunnelRuntimeStats,
};
use kelicloud_agent_rs::tunnel_session::{
    decode_session_open_payload, encode_session_open_payload, TunnelSessionOpenPayload,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener as TokioTcpListener, TcpStream as TokioTcpStream};

#[test]
fn async_runtime_limits_have_bounded_defaults() {
    let limits = TunnelRuntimeLimits::default();

    assert_eq!(limits.max_sessions_per_agent, 1024);
    assert_eq!(limits.max_outbound_frames, 4096);
    assert_eq!(limits.max_session_pending_bytes, 4 * 1024 * 1024);
    assert_eq!(limits.tcp_read_chunk_size, 16 * 1024);
    assert!(limits.target_dial_timeout.as_secs() <= 5);
    assert!(limits.idle_timeout.as_secs() >= 600);
    assert_eq!(limits.relay_batch_policy, TunnelRelayBatchPolicy::Fixed);
}

#[test]
fn relay_batch_policy_keeps_default_fixed_and_caps_adaptive_only_at_high_concurrency() {
    assert_eq!(
        TunnelRelayBatchPolicy::Fixed.effective_batch_frames(64, 32),
        64
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames(64, 4),
        64
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames(64, 8),
        16
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames(64, 12),
        16
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames(64, 16),
        16
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames(8, 16),
        8
    );
}

#[test]
fn relay_batch_policy_adaptive_responds_to_queue_dwell_pressure() {
    let elevated_dwell = TunnelQueueDwellStatsSnapshot {
        frames: 100,
        micros_total: 6_000_000,
        micros_max: 90_000,
        p50_micros: 25_000,
        p95_micros: 60_000,
        p99_micros: 90_000,
    };
    let severe_dwell = TunnelQueueDwellStatsSnapshot {
        frames: 100,
        micros_total: 30_000_000,
        micros_max: 600_000,
        p50_micros: 80_000,
        p95_micros: 300_000,
        p99_micros: 600_000,
    };

    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames_with_dwell(64, 4, elevated_dwell),
        16
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames_with_dwell(64, 8, severe_dwell),
        8
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Fixed.effective_batch_frames_with_dwell(64, 8, severe_dwell),
        64
    );
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
fn async_frame_queue_drains_fifo_batches() {
    let queue = AsyncTunnelFrameQueue::new(4);
    queue.try_push(frame(1, b"one")).expect("push first");
    queue.try_push(frame(2, b"two")).expect("push second");
    queue.try_push(frame(3, b"three")).expect("push third");

    let first = queue.drain(2);

    assert_eq!(
        first
            .iter()
            .map(|frame| frame.session_id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(queue.len(), 1);

    let second = queue.drain(8);

    assert_eq!(
        second
            .iter()
            .map(|frame| frame.session_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert!(queue.is_empty());
}

#[test]
fn async_frame_queue_records_enqueue_to_drain_dwell() {
    let stats = TunnelRuntimeStats::default();
    let queue = AsyncTunnelFrameQueue::new_with_stats(4, stats.clone());
    queue.try_push(frame(7, b"queued")).expect("push frame");

    thread::sleep(Duration::from_millis(2));
    let frames = queue.drain(4);
    let snapshot = stats.snapshot();

    assert_eq!(frames.len(), 1);
    assert_eq!(snapshot.outbound_queue_dwell.frames, 1);
    assert!(
        snapshot.outbound_queue_dwell.micros_total > 0,
        "queue dwell total should be observable"
    );
    assert!(
        snapshot.outbound_queue_dwell.micros_max > 0,
        "queue dwell max should be observable"
    );
    assert!(
        snapshot.outbound_queue_dwell.p50_micros > 0,
        "queue dwell p50 should be observable"
    );
}

#[test]
fn async_frame_queue_keeps_recent_dwell_window_separate_from_lifetime_dwell() {
    let stats = TunnelRuntimeStats::default();
    let queue = AsyncTunnelFrameQueue::new_with_stats(32, stats.clone());
    queue.try_push(frame(17, b"slow")).expect("push slow frame");
    thread::sleep(Duration::from_millis(80));
    assert_eq!(queue.drain(32).len(), 1);

    for index in 0..16 {
        queue
            .try_push(frame(100 + index, b"fast"))
            .expect("push fast frame");
        assert_eq!(queue.drain(32).len(), 1);
    }

    let snapshot = stats.snapshot();

    assert!(
        snapshot.outbound_queue_dwell.p95_micros >= 50_000,
        "lifetime dwell should still remember the slow frame: {:?}",
        snapshot.outbound_queue_dwell
    );
    assert!(
        snapshot.recent_outbound_queue_dwell.p95_micros < 50_000,
        "recent dwell should recover after the fast window: {:?}",
        snapshot.recent_outbound_queue_dwell
    );
    assert_eq!(
        TunnelRelayBatchPolicy::Adaptive.effective_batch_frames_with_dwell(
            64,
            4,
            snapshot.recent_outbound_queue_dwell
        ),
        64
    );
}

#[test]
fn async_frame_queue_drain_after_wait_wakes_when_frame_is_pushed() {
    let queue = AsyncTunnelFrameQueue::new(4);
    let producer = queue.clone();
    let producer_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        producer
            .try_push(frame(9, b"late"))
            .expect("push delayed frame");
    });

    let frames = queue.drain_after_wait(4, Duration::from_secs(1));

    producer_thread.join().expect("producer should finish");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].session_id, 9);
}

#[test]
fn async_frame_queue_shared_notifier_wakes_when_any_attached_queue_pushes() {
    let notifier = Arc::new(TunnelFrameReadyNotifier::new());
    let first = AsyncTunnelFrameQueue::new_with_notifier(4, Arc::clone(&notifier));
    let second = AsyncTunnelFrameQueue::new_with_notifier(4, Arc::clone(&notifier));
    let observed_generation = notifier.generation();
    let producer = second.clone();
    let producer_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        producer
            .try_push(frame(19, b"shared"))
            .expect("push delayed frame through second queue");
    });

    let changed_generation = notifier.wait_for_change(observed_generation, Duration::from_secs(1));

    producer_thread.join().expect("producer should finish");
    assert!(
        changed_generation > observed_generation,
        "shared notifier should wake when any attached queue receives a frame"
    );
    assert!(first.drain(4).is_empty());
    let frames = second.drain(4);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].session_id, 19);
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
fn async_runtime_adaptive_batch_frames_use_observed_queue_dwell() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let listen_port = free_tcp_port();
        let mut limits = TunnelRuntimeLimits::default();
        limits.relay_batch_policy = TunnelRelayBatchPolicy::Adaptive;
        let core = AsyncTunnelCore::new(limits);
        core.start_ingress_listener(TunnelIngressListenerSpec {
            rule_id: 91,
            listen_address: "127.0.0.1".to_string(),
            listen_port,
            source_allowlist: String::new(),
        })
        .await
        .expect("start ingress listener");

        assert_eq!(core.effective_outbound_batch_frames(64), 64);

        let _client = TokioTcpStream::connect(("127.0.0.1", listen_port))
            .await
            .expect("connect ingress client");
        tokio::time::sleep(Duration::from_millis(80)).await;
        let frames = core.next_frames(64).await;
        let dwell = core.stats_snapshot().outbound_queue_dwell;

        assert!(
            !frames.is_empty(),
            "ingress connect should queue open frame"
        );
        assert!(
            dwell.p95_micros >= 50_000,
            "test should observe queue dwell pressure: {dwell:?}"
        );
        assert_eq!(core.effective_outbound_batch_frames(64), 16);

        core.stop_ingress_listener(91)
            .await
            .expect("stop ingress listener");
    });
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
        let open_payload = decode_session_open_payload(&open.payload).expect("decode open");
        assert_eq!(open_payload.rule_id, 17);
        assert_eq!(open_payload.listen_host, "127.0.0.1");
        assert_eq!(open_payload.listen_port, listen_port);

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

#[test]
fn async_runtime_rejects_session_when_agent_limit_is_reached() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let mut limits = TunnelRuntimeLimits::default();
        limits.max_sessions_per_agent = 1;
        let core = AsyncTunnelCore::new(limits);
        let (target_port, _hold_thread) = hold_one_target_connection();

        core.open_egress_session(1, 7, "127.0.0.1", target_port, session_open_payload(7))
            .await
            .expect("first session should open");
        assert_eq!(core.stats_snapshot().active_sessions, 1);

        let err = core
            .open_egress_session(2, 7, "127.0.0.1", target_port, session_open_payload(7))
            .await
            .expect_err("second session should exceed agent limit");
        assert_eq!(err.code(), "session_limit");
        assert_eq!(core.stats_snapshot().active_sessions, 1);
    });
}

#[test]
fn async_runtime_close_session_removes_active_count() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
        let (target_port, _hold_thread) = hold_one_target_connection();
        core.open_egress_session(10, 7, "127.0.0.1", target_port, session_open_payload(7))
            .await
            .expect("session should open");

        assert_eq!(core.stats_snapshot().active_sessions, 1);
        core.close_session(10, "test_close")
            .await
            .expect("session should close");
        assert_eq!(core.stats_snapshot().active_sessions, 0);
    });
}

#[test]
fn async_runtime_rejects_payload_over_session_pending_byte_limit() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let mut limits = TunnelRuntimeLimits::default();
        limits.max_session_pending_bytes = 4;
        let core = AsyncTunnelCore::new(limits);
        let (target_port, _hold_thread) = hold_one_target_connection();
        core.open_egress_session(11, 7, "127.0.0.1", target_port, session_open_payload(7))
            .await
            .expect("session should open");

        let err = core
            .handle_session_data(11, FrameLeg::Egress, b"12345".to_vec())
            .await
            .expect_err("oversized payload should exceed per-session pending byte limit");

        assert_eq!(err.code(), "session_pending_bytes_limit");
        assert_eq!(core.stats_snapshot().active_sessions, 1);
    });
}

#[test]
fn async_runtime_handles_100_concurrent_loopback_sessions() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .worker_threads(4)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let listener = TokioTcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind async target listener");
        let target_port = listener.local_addr().expect("target addr").port();
        let held_streams = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let accepted = std::sync::Arc::clone(&held_streams);
        let accept_task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                accepted.lock().await.push(stream);
            }
        });
        let core = AsyncTunnelCore::new(TunnelRuntimeLimits::default());
        let mut handles = Vec::new();
        for session_id in 1..=100u64 {
            let cloned = core.clone();
            handles.push(tokio::spawn(async move {
                cloned
                    .open_egress_session(
                        session_id,
                        7,
                        "127.0.0.1",
                        target_port,
                        session_open_payload(7),
                    )
                    .await
                    .expect("open loopback session");
            }));
        }

        for handle in handles {
            handle.await.expect("session task");
        }
        let snapshot = core.stats_snapshot();
        assert_eq!(snapshot.active_sessions, 100);
        assert_eq!(snapshot.total_sessions, 100);

        for session_id in 1..=100u64 {
            core.close_session(session_id, "test cleanup")
                .await
                .expect("close loopback session");
        }
        assert_eq!(core.stats_snapshot().active_sessions, 0);
        accept_task.abort();
    });
}

#[test]
fn async_ingress_listener_stops_when_removed() {
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
            rule_id: 71,
            listen_address: "127.0.0.1".to_string(),
            listen_port,
            source_allowlist: "127.0.0.0/8".to_string(),
        })
        .await
        .expect("start listener");

        let _stream = connect_tokio_with_retry("127.0.0.1", listen_port).await;
        core.stop_ingress_listener(71).await.expect("stop listener");

        assert_tokio_port_eventually_closed("127.0.0.1", listen_port).await;
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

fn hold_one_target_connection() -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind target listener");
    let port = listener.local_addr().expect("target addr").port();
    let thread = thread::spawn(move || {
        if let Ok((_stream, _)) = listener.accept() {
            thread::sleep(std::time::Duration::from_secs(5));
        }
    });
    (port, thread)
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

async fn assert_tokio_port_eventually_closed(host: &str, port: u16) {
    let addr = format!("{host}:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match TokioTcpStream::connect(&addr).await {
            Ok(stream) if std::time::Instant::now() < deadline => {
                drop(stream);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Ok(_) => panic!("port {addr} stayed open"),
            Err(_) => return,
        }
    }
}
