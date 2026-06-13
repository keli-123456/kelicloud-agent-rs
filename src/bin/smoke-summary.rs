use kelicloud_agent_rs::smoke_summary::summarize_log_text;
use std::env;
use std::fs;
use std::process;

fn main() {
    let paths = env::args().skip(1).collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!("usage: smoke-summary <smoke-log> [smoke-log ...]");
        process::exit(2);
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
    println!("{}", summarize_log_text(source, &combined).to_markdown());
}
