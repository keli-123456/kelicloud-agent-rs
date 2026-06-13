use kelicloud_agent_rs::smoke_summary::summarize_log_text;
use std::env;
use std::fs;
use std::process;

fn main() {
    let mut require_pass = false;
    let mut paths = Vec::new();
    for arg in env::args().skip(1) {
        if arg == "--require-pass" {
            require_pass = true;
        } else if arg == "--help" || arg == "-h" {
            print_usage_and_exit(0);
        } else {
            paths.push(arg);
        }
    }
    if paths.is_empty() {
        print_usage_and_exit(2);
    }

    let mut combined = String::new();
    for path in &paths {
        match fs::read_to_string(path) {
            Ok(contents) => {
                combined.push_str("\n===== ");
                combined.push_str(path);
                combined.push_str(" =====\n");
                combined.push_str(&contents);
            }
            Err(error) => {
                eprintln!("failed to read {path}: {error}");
                process::exit(1);
            }
        }
    }

    let source = paths.join(", ");
    let summary = summarize_log_text(source, &combined);
    println!("{}", summary.to_markdown());
    if require_pass && !summary.is_pass() {
        eprintln!("smoke summary did not pass:");
        for evidence in summary.failed_or_missing_evidence() {
            eprintln!(
                "- {}: {} ({})",
                evidence.label,
                evidence_status_text(evidence.status),
                evidence.detail
            );
        }
        process::exit(1);
    }
}

fn print_usage_and_exit(code: i32) -> ! {
    eprintln!("usage: smoke-summary [--require-pass] <smoke-log> [smoke-log ...]");
    process::exit(code);
}

fn evidence_status_text(
    status: kelicloud_agent_rs::smoke_summary::SmokeEvidenceStatus,
) -> &'static str {
    match status {
        kelicloud_agent_rs::smoke_summary::SmokeEvidenceStatus::Pass => "pass",
        kelicloud_agent_rs::smoke_summary::SmokeEvidenceStatus::Fail => "fail",
        kelicloud_agent_rs::smoke_summary::SmokeEvidenceStatus::Missing => "missing",
    }
}
