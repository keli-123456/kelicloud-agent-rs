use std::collections::HashMap;
use std::error::Error;
use std::fs;

type SummaryResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
struct CarrierRow {
    carrier: String,
    ktp_tcp: String,
    ktp_crypto: String,
    status: String,
    summary_file: String,
    ktp_evidence_file: String,
    tunnel_evidence_file: String,
    tunnel_profile: String,
    tunnel_clients: String,
    tunnel_rounds: String,
    tunnel_total_payload_bytes: String,
    rtt_micros_p50: String,
    rtt_micros_p95: String,
    rtt_micros_p99: String,
    rtt_micros_max: String,
    rtt_client_p95_spread_micros: String,
}

#[derive(Clone, Debug)]
struct CarrierReport {
    output: String,
    gate_failures: Vec<String>,
}

fn main() {
    match run(std::env::args().skip(1)) {
        Ok(report) => {
            println!("{}", report.output);
            if !report.gate_failures.is_empty() {
                for failure in report.gate_failures {
                    eprintln!("{failure}");
                }
                std::process::exit(3);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            print_usage();
            std::process::exit(2);
        }
    }
}

fn run(args: impl Iterator<Item = String>) -> SummaryResult<CarrierReport> {
    let mut require_pass = false;
    let mut require_ktp_aead = false;
    let mut require_ktp_tunnel_rtt = false;
    let mut require_ktp_rdp_like_rtt = false;
    let mut max_ktp_rdp_like_rtt_p95_micros = None::<u64>;
    let mut max_ktp_rdp_like_client_p95_spread_micros = None::<u64>;
    let mut path = None::<String>;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--require-pass" => require_pass = true,
            "--require-ktp-aead" => require_ktp_aead = true,
            "--require-ktp-tunnel-rtt" => require_ktp_tunnel_rtt = true,
            "--require-ktp-rdp-like-rtt" => require_ktp_rdp_like_rtt = true,
            "--max-ktp-rdp-like-rtt-p95-micros" => {
                max_ktp_rdp_like_rtt_p95_micros = Some(next_u64_arg(
                    &mut args,
                    "--max-ktp-rdp-like-rtt-p95-micros",
                )?);
            }
            "--max-ktp-rdp-like-client-p95-spread-micros" => {
                max_ktp_rdp_like_client_p95_spread_micros = Some(next_u64_arg(
                    &mut args,
                    "--max-ktp-rdp-like-client-p95-spread-micros",
                )?);
            }
            _ if arg.trim().is_empty() => return Err("empty argument is not allowed".into()),
            _ if path.is_none() => path = Some(arg),
            _ => return Err("unexpected extra argument".into()),
        }
    }

    let path = path.ok_or("matrix-summary.tsv path is required")?;
    let content = fs::read_to_string(&path)?;
    summarize_tsv(
        &content,
        require_pass,
        require_ktp_aead,
        require_ktp_tunnel_rtt,
        require_ktp_rdp_like_rtt,
        max_ktp_rdp_like_rtt_p95_micros,
        max_ktp_rdp_like_client_p95_spread_micros,
    )
}

fn summarize_tsv(
    content: &str,
    require_pass: bool,
    require_ktp_aead: bool,
    require_ktp_tunnel_rtt: bool,
    require_ktp_rdp_like_rtt: bool,
    max_ktp_rdp_like_rtt_p95_micros: Option<u64>,
    max_ktp_rdp_like_client_p95_spread_micros: Option<u64>,
) -> SummaryResult<CarrierReport> {
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("matrix summary is empty")?;
    let indexes = CarrierIndexes::from_header(header)?;
    let mut rows = Vec::new();

    for line in lines {
        rows.push(parse_row(line, &indexes)?);
    }

    let pass = rows.iter().filter(|row| row.status == "pass").count();
    let fail = rows.iter().filter(|row| row.status == "fail").count();
    let timeout = rows.iter().filter(|row| row.status == "timeout").count();
    let other = rows
        .iter()
        .filter(|row| !matches!(row.status.as_str(), "pass" | "fail" | "timeout"))
        .count();
    let overall_status = if fail == 0 && timeout == 0 && other == 0 {
        "pass"
    } else {
        "fail"
    };

    let mut output = format!(
        "ktp_local_backend_matrix_summary rows={} pass={} fail={} timeout={} status={overall_status}",
        rows.len(),
        pass,
        fail,
        timeout
    );
    if other > 0 {
        output.push_str(&format!(" other={other}"));
    }

    let mut gate_failures = Vec::new();
    for row in &rows {
        if require_pass && row.status != "pass" {
            gate_failures.push(format!(
                "carrier matrix row carrier={} status={} failed require-pass gate",
                row.carrier, row.status
            ));
        }
        output.push('\n');
        output.push_str(&format!(
            "carrier={} ktp_tcp={} ktp_crypto={} status={} summary_file={} ktp_evidence_file={} tunnel_evidence_file={} tunnel_profile={} tunnel_clients={} tunnel_rounds={} tunnel_total_payload_bytes={} rtt_micros_p50={} rtt_micros_p95={} rtt_micros_p99={} rtt_micros_max={} rtt_client_p95_spread_micros={}",
            row.carrier,
            row.ktp_tcp,
            row.ktp_crypto,
            row.status,
            row.summary_file,
            row.ktp_evidence_file,
            row.tunnel_evidence_file,
            row.tunnel_profile,
            row.tunnel_clients,
            row.tunnel_rounds,
            row.tunnel_total_payload_bytes,
            row.rtt_micros_p50,
            row.rtt_micros_p95,
            row.rtt_micros_p99,
            row.rtt_micros_max,
            row.rtt_client_p95_spread_micros
        ));
    }

    let ktp_aead_row = rows.iter().find(|row| {
        row.carrier == "ktp_tcp"
            && row.ktp_tcp == "true"
            && row.ktp_crypto == "ktp_aead"
            && row.status == "pass"
            && row.ktp_evidence_file != "-"
    });
    match ktp_aead_row {
        Some(_) => output.push_str("\nktp_tcp_crypto=ktp_aead ktp_tcp_evidence=present"),
        None => {
            output.push_str("\nktp_tcp_crypto=- ktp_tcp_evidence=missing");
            if require_ktp_aead {
                gate_failures.push(
                    "carrier matrix missing pass row with carrier=ktp_tcp ktp_crypto=ktp_aead"
                        .to_string(),
                );
            }
        }
    }

    let ktp_tunnel_rtt_row = rows.iter().find(|row| {
        row.carrier == "ktp_tcp"
            && row.ktp_tcp == "true"
            && row.status == "pass"
            && row.tunnel_evidence_file != "-"
            && row.tunnel_profile != "-"
            && row.tunnel_clients != "-"
            && row.tunnel_rounds != "-"
            && row.tunnel_total_payload_bytes != "-"
            && row.rtt_micros_p50 != "-"
            && row.rtt_micros_p95 != "-"
            && row.rtt_micros_p99 != "-"
            && row.rtt_micros_max != "-"
            && row.rtt_client_p95_spread_micros != "-"
    });
    match ktp_tunnel_rtt_row {
        Some(row) => output.push_str(&format!(
            "\nktp_tcp_tunnel_rtt_evidence=present profile={} clients={} rounds={} rtt_micros_p95={} rtt_client_p95_spread_micros={}",
            row.tunnel_profile,
            row.tunnel_clients,
            row.tunnel_rounds,
            row.rtt_micros_p95,
            row.rtt_client_p95_spread_micros
        )),
        None => {
            output.push_str("\nktp_tcp_tunnel_rtt_evidence=missing");
            if require_ktp_tunnel_rtt {
                gate_failures.push(
                    "carrier matrix missing pass row with carrier=ktp_tcp tunnel RTT evidence"
                        .to_string(),
                );
            }
        }
    }

    let ktp_rdp_like_rtt_row = rows.iter().find(|row| {
        row.carrier == "ktp_tcp"
            && row.ktp_tcp == "true"
            && row.status == "pass"
            && row.tunnel_profile == "rdp-like"
            && parse_u64_metric(&row.tunnel_clients).is_some_and(|value| value >= 2)
            && parse_u64_metric(&row.tunnel_rounds).is_some_and(|value| value >= 4)
            && parse_u64_metric(&row.tunnel_total_payload_bytes).is_some_and(|value| value > 0)
            && parse_u64_metric(&row.rtt_micros_p50).is_some_and(|value| value > 0)
            && parse_u64_metric(&row.rtt_micros_p95).is_some_and(|value| value > 0)
            && parse_u64_metric(&row.rtt_micros_p99).is_some_and(|value| value > 0)
            && parse_u64_metric(&row.rtt_micros_max).is_some_and(|value| value > 0)
            && parse_u64_metric(&row.rtt_client_p95_spread_micros).is_some()
    });
    match ktp_rdp_like_rtt_row {
        Some(row) => {
            output.push_str(&format!(
                "\nktp_tcp_rdp_like_rtt_evidence=present clients={} rounds={} total_payload_bytes={} rtt_micros_p95={} rtt_client_p95_spread_micros={}",
                row.tunnel_clients,
                row.tunnel_rounds,
                row.tunnel_total_payload_bytes,
                row.rtt_micros_p95,
                row.rtt_client_p95_spread_micros
            ));
            if let (Some(actual), Some(max)) = (
                parse_u64_metric(&row.rtt_micros_p95),
                max_ktp_rdp_like_rtt_p95_micros,
            ) {
                if actual > max {
                    gate_failures.push(format!(
                        "ktp_tcp rdp-like rtt_micros_p95 {actual} exceeds max {max}"
                    ));
                }
            }
            if let (Some(actual), Some(max)) = (
                parse_u64_metric(&row.rtt_client_p95_spread_micros),
                max_ktp_rdp_like_client_p95_spread_micros,
            ) {
                if actual > max {
                    gate_failures.push(format!(
                        "ktp_tcp rdp-like rtt_client_p95_spread_micros {actual} exceeds max {max}"
                    ));
                }
            }
        }
        None => {
            output.push_str("\nktp_tcp_rdp_like_rtt_evidence=missing");
            if require_ktp_rdp_like_rtt {
                gate_failures.push(
                    "carrier matrix missing pass row with carrier=ktp_tcp rdp-like multi-client RTT evidence"
                        .to_string(),
                );
            }
        }
    }

    Ok(CarrierReport {
        output,
        gate_failures,
    })
}

fn parse_row(line: &str, indexes: &CarrierIndexes) -> SummaryResult<CarrierRow> {
    let fields = line.split('\t').map(str::trim).collect::<Vec<_>>();
    Ok(CarrierRow {
        carrier: field(&fields, indexes.carrier, "carrier")?.to_string(),
        ktp_tcp: field(&fields, indexes.ktp_tcp, "ktp_tcp")?.to_string(),
        ktp_crypto: field(&fields, indexes.ktp_crypto, "ktp_crypto")?.to_string(),
        status: field(&fields, indexes.status, "status")?.to_string(),
        summary_file: field(&fields, indexes.summary_file, "summary_file")?.to_string(),
        ktp_evidence_file: field(&fields, indexes.ktp_evidence_file, "ktp_evidence_file")?
            .to_string(),
        tunnel_evidence_file: optional_field(&fields, indexes.tunnel_evidence_file).to_string(),
        tunnel_profile: optional_field(&fields, indexes.tunnel_profile).to_string(),
        tunnel_clients: optional_field(&fields, indexes.tunnel_clients).to_string(),
        tunnel_rounds: optional_field(&fields, indexes.tunnel_rounds).to_string(),
        tunnel_total_payload_bytes: optional_field(&fields, indexes.tunnel_total_payload_bytes)
            .to_string(),
        rtt_micros_p50: optional_field(&fields, indexes.rtt_micros_p50).to_string(),
        rtt_micros_p95: optional_field(&fields, indexes.rtt_micros_p95).to_string(),
        rtt_micros_p99: optional_field(&fields, indexes.rtt_micros_p99).to_string(),
        rtt_micros_max: optional_field(&fields, indexes.rtt_micros_max).to_string(),
        rtt_client_p95_spread_micros: optional_field(&fields, indexes.rtt_client_p95_spread_micros)
            .to_string(),
    })
}

fn field<'a>(fields: &'a [&str], index: usize, name: &str) -> SummaryResult<&'a str> {
    fields
        .get(index)
        .copied()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing field {name}").into())
}

fn optional_field<'a>(fields: &'a [&str], index: Option<usize>) -> &'a str {
    index
        .and_then(|index| fields.get(index).copied())
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
}

#[derive(Clone, Copy, Debug)]
struct CarrierIndexes {
    carrier: usize,
    ktp_tcp: usize,
    ktp_crypto: usize,
    status: usize,
    summary_file: usize,
    ktp_evidence_file: usize,
    tunnel_evidence_file: Option<usize>,
    tunnel_profile: Option<usize>,
    tunnel_clients: Option<usize>,
    tunnel_rounds: Option<usize>,
    tunnel_total_payload_bytes: Option<usize>,
    rtt_micros_p50: Option<usize>,
    rtt_micros_p95: Option<usize>,
    rtt_micros_p99: Option<usize>,
    rtt_micros_max: Option<usize>,
    rtt_client_p95_spread_micros: Option<usize>,
}

impl CarrierIndexes {
    fn from_header(header: &str) -> SummaryResult<Self> {
        let positions = header
            .split('\t')
            .enumerate()
            .map(|(index, name)| (name.trim().to_string(), index))
            .collect::<HashMap<_, _>>();
        Ok(Self {
            carrier: required_column(&positions, "carrier")?,
            ktp_crypto: required_column(&positions, "ktp_crypto")?,
            ktp_tcp: required_column(&positions, "ktp_tcp")?,
            status: required_column(&positions, "status")?,
            summary_file: required_column(&positions, "summary_file")?,
            ktp_evidence_file: required_column(&positions, "ktp_evidence_file")?,
            tunnel_evidence_file: optional_column(&positions, "tunnel_evidence_file"),
            tunnel_profile: optional_column(&positions, "tunnel_profile"),
            tunnel_clients: optional_column(&positions, "tunnel_clients"),
            tunnel_rounds: optional_column(&positions, "tunnel_rounds"),
            tunnel_total_payload_bytes: optional_column(&positions, "tunnel_total_payload_bytes"),
            rtt_micros_p50: optional_column(&positions, "rtt_micros_p50"),
            rtt_micros_p95: optional_column(&positions, "rtt_micros_p95"),
            rtt_micros_p99: optional_column(&positions, "rtt_micros_p99"),
            rtt_micros_max: optional_column(&positions, "rtt_micros_max"),
            rtt_client_p95_spread_micros: optional_column(
                &positions,
                "rtt_client_p95_spread_micros",
            ),
        })
    }
}

fn required_column(positions: &HashMap<String, usize>, name: &str) -> SummaryResult<usize> {
    positions
        .get(name)
        .copied()
        .ok_or_else(|| format!("missing required column: {name}").into())
}

fn optional_column(positions: &HashMap<String, usize>, name: &str) -> Option<usize> {
    positions.get(name).copied()
}

fn parse_u64_metric(value: &str) -> Option<u64> {
    if value == "-" {
        return None;
    }
    value.parse::<u64>().ok()
}

fn next_u64_arg(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    name: &str,
) -> SummaryResult<u64> {
    let value = args
        .next()
        .ok_or_else(|| format!("{name} value is required"))?;
    if value.trim().is_empty() {
        return Err(format!("{name} value is empty").into());
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} value must be an unsigned integer").into())
}

fn print_usage() {
    eprintln!(
        "usage: ktp-local-backend-matrix-summary [--require-pass] [--require-ktp-aead] [--require-ktp-tunnel-rtt] [--require-ktp-rdp-like-rtt] [--max-ktp-rdp-like-rtt-p95-micros N] [--max-ktp-rdp-like-client-p95-spread-micros N] <matrix-summary.tsv>"
    );
}
