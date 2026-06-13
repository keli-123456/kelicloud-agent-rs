use kelicloud_agent_rs::ping::{
    parse_linux_ping_latency_ms, FixedPingExecutor, PingExecutor, PingResult, PingTask,
};

#[test]
fn ping_result_serializes_backend_shape() {
    let result = PingResult::new(42, "tcp", 17, "2026-06-12T12:00:00Z");

    let value = serde_json::to_value(result).unwrap();

    assert_eq!(value["type"], "ping_result");
    assert_eq!(value["task_id"], 42);
    assert_eq!(value["ping_type"], "tcp");
    assert_eq!(value["value"], 17);
    assert_eq!(value["finished_at"], "2026-06-12T12:00:00Z");
}

#[test]
fn fixed_ping_executor_returns_configured_latency() {
    let executor = FixedPingExecutor::new(23);
    let task = PingTask {
        task_id: 7,
        ping_type: "tcp".to_string(),
        target: "1.1.1.1:443".to_string(),
    };

    let result = executor.run(&task);

    assert_eq!(result.task_id, 7);
    assert_eq!(result.ping_type, "tcp");
    assert_eq!(result.value, 23);
}

#[test]
fn parse_linux_ping_latency_from_common_output() {
    let output = "64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=12.3 ms\n";

    assert_eq!(parse_linux_ping_latency_ms(output), Some(12));
}
