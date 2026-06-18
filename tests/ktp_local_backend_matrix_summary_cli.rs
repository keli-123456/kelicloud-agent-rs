use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn ktp_local_backend_matrix_summary_reports_carrier_comparison() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-pass",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-
ktp_tcp	true	ktp_aead	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md
"#,
    );

    let output = Command::new(summary_exe())
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert!(
        output.status.success(),
        "summary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout
        .contains("ktp_local_backend_matrix_summary rows=2 pass=2 fail=0 timeout=0 status=pass"));
    assert!(stdout.contains(
        "carrier=websocket ktp_tcp=false ktp_crypto=- status=pass summary_file=logs/websocket/agent.summary.md ktp_evidence_file=-"
    ));
    assert!(stdout.contains(
        "carrier=ktp_tcp ktp_tcp=true ktp_crypto=ktp_aead status=pass summary_file=logs/ktp_tcp/agent.summary.md ktp_evidence_file=logs/ktp_tcp/ktp-live-canary.evidence.md"
    ));
    assert!(stdout.contains("ktp_tcp_crypto=ktp_aead ktp_tcp_evidence=present"));
}

#[test]
fn ktp_local_backend_matrix_summary_require_pass_rejects_failed_rows() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-fail",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-
ktp_tcp	true	ktp_aead	fail	logs/ktp_tcp	-	-
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-pass")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "require-pass should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("carrier matrix row carrier=ktp_tcp status=fail failed require-pass gate")
    );
}

#[test]
fn ktp_local_backend_matrix_summary_require_ktp_aead_rejects_missing_crypto() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-missing-crypto",
        r#"carrier	ktp_tcp	ktp_crypto	status	log_dir	summary_file	ktp_evidence_file
websocket	false	-	pass	logs/websocket	logs/websocket/agent.summary.md	-
ktp_tcp	true	-	pass	logs/ktp_tcp	logs/ktp_tcp/agent.summary.md	logs/ktp_tcp/ktp-live-canary.evidence.md
"#,
    );

    let output = Command::new(summary_exe())
        .arg("--require-ktp-aead")
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "missing KTP AEAD should exit 3: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("carrier matrix missing pass row with carrier=ktp_tcp ktp_crypto=ktp_aead")
    );
}

#[test]
fn ktp_local_backend_matrix_summary_rejects_missing_required_columns() {
    let summary_path = write_temp_summary(
        "ktp-local-backend-matrix-summary-missing-column",
        r#"carrier	status
websocket	pass
"#,
    );

    let output = Command::new(summary_exe())
        .arg(&summary_path)
        .output()
        .expect("ktp-local-backend-matrix-summary should run");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing-column summary should exit 2: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing required column: ktp_crypto"));
}

fn write_temp_summary(prefix: &str, content: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.tsv", std::process::id()));
    std::fs::write(&path, content).expect("summary fixture should be written");
    path
}

fn summary_exe() -> String {
    std::env::var("CARGO_BIN_EXE_ktp-local-backend-matrix-summary")
        .expect("ktp-local-backend-matrix-summary binary should be built by cargo")
}
