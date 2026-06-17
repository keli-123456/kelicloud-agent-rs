use kelicloud_agent_rs::ktp::{FrameLeg, FrameType, KtpFrame};
use kelicloud_agent_rs::tunnel_async_runtime::{
    AsyncTunnelFrameQueue, TunnelRuntimeLimits, TunnelRuntimeStats,
};

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

fn frame(session_id: u64, payload: &[u8]) -> KtpFrame {
    KtpFrame {
        frame_type: FrameType::SessionData,
        leg: FrameLeg::Ingress,
        flags: 0,
        session_id,
        payload: payload.to_vec(),
    }
}
