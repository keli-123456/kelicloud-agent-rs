use std::process::Command;

#[test]
fn ktp_live_canary_script_collects_tunnel_data_diagnostics() {
    let script = std::fs::read_to_string("scripts/ktp-live-canary-evidence.sh")
        .expect("ktp live canary script should be readable");

    assert!(script.contains("journalctl"));
    assert!(script.contains("--log-file"));
    assert!(script.contains("tunnel data diagnostics"));
    assert!(script.contains("runtime_wait_elapsed_p50_micros"));
    assert!(script.contains("runtime_wait_elapsed_p95_micros"));
    assert!(script.contains("runtime_wait_elapsed_p99_micros"));
    assert!(script.contains("outbound_queue_dwell_p50_micros"));
    assert!(script.contains("outbound_queue_dwell_p95_micros"));
    assert!(script.contains("outbound_queue_dwell_p99_micros"));
    assert!(script.contains("socket_read_batches"));
    assert!(script.contains("socket_read_frames"));
    assert!(script.contains("socket_read_max_batch_frames"));
    assert!(script.contains("POSITIVE_FIELDS"));
    assert!(script.contains("expected positive diagnostics field"));
    assert!(script.contains("KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES"));
    assert!(script.contains("expected socket_read_max_batch_frames >="));
    assert!(script.contains("ktp-live-canary.evidence.md"));
}

#[test]
fn ktp_live_canary_script_has_valid_bash_syntax_when_bash_is_available() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }
    let status = Command::new("bash")
        .args(["-n", "scripts/ktp-live-canary-evidence.sh"])
        .status()
        .expect("bash -n should run");
    assert!(status.success());
}

#[test]
fn ktp_live_canary_script_accepts_sample_log_file() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let temp_dir =
        std::env::temp_dir().join(format!("kelicloud-ktp-canary-test-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let log_file = temp_dir.join("agent.log");
    let evidence_file = temp_dir.join("ktp-live-canary.evidence.md");
    std::fs::write(
        &log_file,
        "tunnel data diagnostics: runtime_wait_attempts=3 runtime_wait_hits=2 runtime_wait_elapsed_micros_total=120 runtime_wait_elapsed_micros_max=70 runtime_wait_elapsed_p50_micros=50 runtime_wait_elapsed_p95_micros=100 runtime_wait_elapsed_p99_micros=100 outbound_runtime_frames=9 outbound_queue_dwell_frames=9 outbound_queue_dwell_micros_total=240 outbound_queue_dwell_micros_max=90 outbound_queue_dwell_p50_micros=50 outbound_queue_dwell_p95_micros=100 outbound_queue_dwell_p99_micros=100 socket_idle_reads=4 socket_idle_empty_reads=1 socket_read_batches=2 socket_read_frames=9 socket_read_max_batch_frames=7\n",
    )
    .expect("sample log should be written");

    let output = Command::new("bash")
        .args([
            "scripts/ktp-live-canary-evidence.sh",
            "--log-file",
            log_file.to_str().expect("log path should be utf-8"),
            "--evidence-file",
            evidence_file
                .to_str()
                .expect("evidence path should be utf-8"),
        ])
        .output()
        .expect("ktp live canary script should run");

    assert!(
        output.status.success(),
        "script failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = std::fs::read_to_string(&evidence_file).expect("evidence should be written");
    assert!(evidence.contains("KTP Live Canary Evidence"));
    assert!(evidence.contains("runtime_wait_elapsed_p99_micros=100"));
    assert!(evidence.contains("outbound_queue_dwell_p99_micros=100"));
}

#[test]
fn ktp_live_canary_script_rejects_zero_socket_batch_reads() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "kelicloud-ktp-canary-zero-batch-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let log_file = temp_dir.join("agent.log");
    let evidence_file = temp_dir.join("ktp-live-canary.evidence.md");
    std::fs::write(
        &log_file,
        "tunnel data diagnostics: runtime_wait_attempts=3 runtime_wait_hits=2 runtime_wait_elapsed_micros_total=120 runtime_wait_elapsed_micros_max=70 runtime_wait_elapsed_p50_micros=50 runtime_wait_elapsed_p95_micros=100 runtime_wait_elapsed_p99_micros=100 outbound_runtime_frames=9 outbound_queue_dwell_frames=9 outbound_queue_dwell_micros_total=240 outbound_queue_dwell_micros_max=90 outbound_queue_dwell_p50_micros=50 outbound_queue_dwell_p95_micros=100 outbound_queue_dwell_p99_micros=100 socket_idle_reads=4 socket_idle_empty_reads=1 socket_read_batches=0 socket_read_frames=0 socket_read_max_batch_frames=0\n",
    )
    .expect("sample log should be written");

    let output = Command::new("bash")
        .args([
            "scripts/ktp-live-canary-evidence.sh",
            "--log-file",
            log_file.to_str().expect("log path should be utf-8"),
            "--evidence-file",
            evidence_file
                .to_str()
                .expect("evidence path should be utf-8"),
        ])
        .output()
        .expect("script should run");

    assert!(
        !output.status.success(),
        "script unexpectedly accepted zero socket batch reads: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("expected positive diagnostics field: socket_read_batches"),
        "stderr should explain the missing active batch-read evidence: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ktp_live_canary_script_can_require_multi_frame_socket_batch_reads() {
    if Command::new("bash").arg("--version").output().is_err() {
        return;
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "kelicloud-ktp-canary-multi-frame-batch-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
    let log_file = temp_dir.join("agent.log");
    let evidence_file = temp_dir.join("ktp-live-canary.evidence.md");
    std::fs::write(
        &log_file,
        "tunnel data diagnostics: runtime_wait_attempts=3 runtime_wait_hits=2 runtime_wait_elapsed_micros_total=120 runtime_wait_elapsed_micros_max=70 runtime_wait_elapsed_p50_micros=50 runtime_wait_elapsed_p95_micros=100 runtime_wait_elapsed_p99_micros=100 outbound_runtime_frames=9 outbound_queue_dwell_frames=9 outbound_queue_dwell_micros_total=240 outbound_queue_dwell_micros_max=90 outbound_queue_dwell_p50_micros=50 outbound_queue_dwell_p95_micros=100 outbound_queue_dwell_p99_micros=100 socket_idle_reads=4 socket_idle_empty_reads=1 socket_read_batches=3 socket_read_frames=3 socket_read_max_batch_frames=1\n",
    )
    .expect("sample log should be written");

    let output = Command::new("bash")
        .env("KTP_LIVE_CANARY_MIN_MAX_BATCH_FRAMES", "2")
        .args([
            "scripts/ktp-live-canary-evidence.sh",
            "--log-file",
            log_file.to_str().expect("log path should be utf-8"),
            "--evidence-file",
            evidence_file
                .to_str()
                .expect("evidence path should be utf-8"),
        ])
        .output()
        .expect("script should run");

    assert!(
        !output.status.success(),
        "script unexpectedly accepted single-frame socket batches: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "expected socket_read_max_batch_frames >= 2, found 1"
        ),
        "stderr should explain the stricter active batch-read threshold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
