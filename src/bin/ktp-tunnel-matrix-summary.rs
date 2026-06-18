use std::collections::HashMap;
use std::error::Error;
use std::fs;

type SummaryResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
struct MatrixRow {
    relay_batch_policy: String,
    clients: String,
    status: String,
    elapsed_millis: u64,
    rtt_micros_p95: Option<u64>,
    client_p95_spread_micros: Option<u64>,
    socket_read_max_batch_frames: Option<u64>,
}

#[derive(Clone, Debug)]
struct MatrixReport {
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

fn run(args: impl Iterator<Item = String>) -> SummaryResult<MatrixReport> {
    let mut require_pass = false;
    let mut path = None::<String>;
    for arg in args {
        match arg.as_str() {
            "--require-pass" => require_pass = true,
            _ if arg.trim().is_empty() => return Err("empty argument is not allowed".into()),
            _ if path.is_none() => path = Some(arg),
            _ => return Err("unexpected extra argument".into()),
        }
    }

    let path = path.ok_or("matrix-summary.tsv path is required")?;
    let content = fs::read_to_string(&path)?;
    summarize_tsv(&content, require_pass)
}

fn summarize_tsv(content: &str, require_pass: bool) -> SummaryResult<MatrixReport> {
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("matrix summary is empty")?;
    let indexes = MatrixIndexes::from_header(header)?;
    let mut rows = Vec::new();

    for (line_number, line) in lines.enumerate() {
        rows.push(parse_row(line, &indexes, line_number + 2)?);
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
        "ktp_tunnel_matrix_summary rows={} pass={} fail={} timeout={} status={overall_status}",
        rows.len(),
        pass,
        fail,
        timeout
    );
    if other > 0 {
        output.push_str(&format!(" other={other}"));
    }

    let mut max_rtt = MaxMetric::default();
    let mut max_spread = MaxMetric::default();
    let mut max_socket_batch = MaxMetric::default();
    let mut gate_failures = Vec::new();

    for row in &rows {
        if require_pass && row.status != "pass" {
            gate_failures.push(format!(
                "tunnel matrix row policy={} clients={} status={} failed require-pass gate",
                row.relay_batch_policy, row.clients, row.status
            ));
        }

        let rtt = metric_text(row.rtt_micros_p95);
        let spread = metric_text(row.client_p95_spread_micros);
        let socket_batch = metric_text(row.socket_read_max_batch_frames);
        output.push('\n');
        output.push_str(&format!(
            "policy={} clients={} status={} elapsed_millis={} rtt_micros_p95={} rtt_client_p95_spread_micros={} socket_read_max_batch_frames={}",
            row.relay_batch_policy,
            row.clients,
            row.status,
            row.elapsed_millis,
            rtt,
            spread,
            socket_batch
        ));

        if row.status == "pass" {
            max_rtt.record(&row.relay_batch_policy, &row.clients, row.rtt_micros_p95);
            max_spread.record(
                &row.relay_batch_policy,
                &row.clients,
                row.client_p95_spread_micros,
            );
            max_socket_batch.record(
                &row.relay_batch_policy,
                &row.clients,
                row.socket_read_max_batch_frames,
            );
        }
    }

    output.push('\n');
    output.push_str(&format!(
        "max_rtt_micros_p95={} policy={} clients={}",
        metric_text(max_rtt.value),
        max_rtt.policy.as_deref().unwrap_or("-"),
        max_rtt.clients.as_deref().unwrap_or("-")
    ));
    output.push('\n');
    output.push_str(&format!(
        "max_rtt_client_p95_spread_micros={} policy={} clients={}",
        metric_text(max_spread.value),
        max_spread.policy.as_deref().unwrap_or("-"),
        max_spread.clients.as_deref().unwrap_or("-")
    ));
    output.push('\n');
    output.push_str(&format!(
        "max_socket_read_max_batch_frames={} policy={} clients={}",
        metric_text(max_socket_batch.value),
        max_socket_batch.policy.as_deref().unwrap_or("-"),
        max_socket_batch.clients.as_deref().unwrap_or("-")
    ));

    Ok(MatrixReport {
        output,
        gate_failures,
    })
}

fn parse_row(line: &str, indexes: &MatrixIndexes, line_number: usize) -> SummaryResult<MatrixRow> {
    let fields = line.split('\t').map(str::trim).collect::<Vec<_>>();
    let relay_batch_policy = indexes
        .relay_batch_policy
        .map(|index| field(&fields, index, "relay_batch_policy").map(str::to_string))
        .transpose()?
        .unwrap_or_else(|| "fixed".to_string());
    let clients = field(&fields, indexes.clients, "clients")?.to_string();
    let status = field(&fields, indexes.status, "status")?.to_string();
    let elapsed_millis = parse_required_u64(&fields, indexes.elapsed_millis, "elapsed_millis")?;
    let pass_row = status == "pass";

    Ok(MatrixRow {
        relay_batch_policy,
        clients: clients.clone(),
        status,
        elapsed_millis,
        rtt_micros_p95: parse_metric(
            &fields,
            indexes.rtt_micros_p95,
            "rtt_micros_p95",
            pass_row,
            line_number,
            &clients,
        )?,
        client_p95_spread_micros: parse_metric(
            &fields,
            indexes.rtt_client_p95_spread_micros,
            "rtt_client_p95_spread_micros",
            pass_row,
            line_number,
            &clients,
        )?,
        socket_read_max_batch_frames: parse_metric(
            &fields,
            indexes.socket_read_max_batch_frames,
            "socket_read_max_batch_frames",
            pass_row,
            line_number,
            &clients,
        )?,
    })
}

fn field<'a>(fields: &'a [&str], index: usize, name: &str) -> SummaryResult<&'a str> {
    fields
        .get(index)
        .copied()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing field {name}").into())
}

fn parse_required_u64(fields: &[&str], index: usize, name: &str) -> SummaryResult<u64> {
    let value = field(fields, index, name)?;
    Ok(value.parse::<u64>()?)
}

fn parse_metric(
    fields: &[&str],
    index: usize,
    name: &str,
    required: bool,
    line_number: usize,
    clients: &str,
) -> SummaryResult<Option<u64>> {
    let value = field(fields, index, name)?;
    if value == "-" {
        if required {
            return Err(format!(
                "missing required pass-row metric {name} at line {line_number} clients={clients}"
            )
            .into());
        }
        return Ok(None);
    }
    Ok(Some(value.parse::<u64>()?))
}

fn metric_text(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[derive(Clone, Debug, Default)]
struct MaxMetric {
    value: Option<u64>,
    policy: Option<String>,
    clients: Option<String>,
}

impl MaxMetric {
    fn record(&mut self, policy: &str, clients: &str, value: Option<u64>) {
        let Some(value) = value else {
            return;
        };
        if self.value.is_none_or(|current| value > current) {
            self.value = Some(value);
            self.policy = Some(policy.to_string());
            self.clients = Some(clients.to_string());
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MatrixIndexes {
    relay_batch_policy: Option<usize>,
    clients: usize,
    status: usize,
    elapsed_millis: usize,
    rtt_micros_p95: usize,
    rtt_client_p95_spread_micros: usize,
    socket_read_max_batch_frames: usize,
}

impl MatrixIndexes {
    fn from_header(header: &str) -> SummaryResult<Self> {
        let positions = header
            .split('\t')
            .enumerate()
            .map(|(index, name)| (name.trim().to_string(), index))
            .collect::<HashMap<_, _>>();
        Ok(Self {
            relay_batch_policy: positions.get("relay_batch_policy").copied(),
            clients: required_column(&positions, "clients")?,
            status: required_column(&positions, "status")?,
            elapsed_millis: required_column(&positions, "elapsed_millis")?,
            rtt_micros_p95: required_column(&positions, "rtt_micros_p95")?,
            rtt_client_p95_spread_micros: required_column(
                &positions,
                "rtt_client_p95_spread_micros",
            )?,
            socket_read_max_batch_frames: required_column(
                &positions,
                "socket_read_max_batch_frames",
            )?,
        })
    }
}

fn required_column(positions: &HashMap<String, usize>, name: &str) -> SummaryResult<usize> {
    positions
        .get(name)
        .copied()
        .ok_or_else(|| format!("missing required column: {name}").into())
}

fn print_usage() {
    eprintln!("usage: ktp-tunnel-matrix-summary [--require-pass] <matrix-summary.tsv>");
}
