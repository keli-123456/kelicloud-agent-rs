use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fs;

type SummaryResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
struct MatrixRow {
    relay_batch_policy: String,
    clients: String,
    relay_adaptive_high_sessions: Option<String>,
    relay_adaptive_elevated_dwell_us: Option<String>,
    relay_adaptive_severe_dwell_us: Option<String>,
    relay_adaptive_elevated_cap: Option<String>,
    relay_adaptive_severe_cap: Option<String>,
    status: String,
    elapsed_millis: u64,
    total_payload_bytes: Option<u64>,
    echo_elapsed_micros: Option<u64>,
    rtt_micros_p95: Option<u64>,
    client_p95_spread_micros: Option<u64>,
    socket_read_max_batch_frames: Option<u64>,
    socket_write_max_batch_frames: Option<u64>,
    socket_write_batch_limit_max: Option<u64>,
    socket_write_batch_limit_min: Option<u64>,
    socket_write_batch_limit_last: Option<u64>,
}

#[derive(Clone, Debug)]
struct MatrixReport {
    output: String,
    gate_failures: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct GateConfig {
    max_rtt_p95_micros: Option<u64>,
    max_client_p95_spread_micros: Option<u64>,
    min_throughput_mib_s: Option<f64>,
    min_echo_throughput_mib_s: Option<f64>,
    expected_policies: Vec<String>,
    expected_clients: Vec<String>,
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
    let mut fail_on_fixed_better = false;
    let mut gate_config = GateConfig::default();
    let mut path = None::<String>;
    let mut args = args;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--require-pass" => require_pass = true,
            "--fail-on-fixed-better" => fail_on_fixed_better = true,
            "--max-rtt-p95-micros" => {
                gate_config.max_rtt_p95_micros =
                    Some(next_u64_arg(&mut args, "--max-rtt-p95-micros", false)?);
            }
            "--max-client-p95-spread-micros" => {
                gate_config.max_client_p95_spread_micros = Some(next_u64_arg(
                    &mut args,
                    "--max-client-p95-spread-micros",
                    true,
                )?);
            }
            "--min-throughput-mib-s" => {
                gate_config.min_throughput_mib_s =
                    Some(next_f64_arg(&mut args, "--min-throughput-mib-s")?);
            }
            "--min-echo-throughput-mib-s" => {
                gate_config.min_echo_throughput_mib_s =
                    Some(next_f64_arg(&mut args, "--min-echo-throughput-mib-s")?);
            }
            "--expect-policies" => {
                gate_config.expected_policies = next_list_arg(&mut args, "--expect-policies")?;
            }
            "--expect-clients" => {
                gate_config.expected_clients = next_list_arg(&mut args, "--expect-clients")?;
            }
            _ if arg.trim().is_empty() => return Err("empty argument is not allowed".into()),
            _ if path.is_none() => path = Some(arg),
            _ => return Err("unexpected extra argument".into()),
        }
    }

    if gate_config.expected_policies.is_empty() != gate_config.expected_clients.is_empty() {
        return Err("--expect-policies and --expect-clients must be provided together".into());
    }

    let path = path.ok_or("matrix-summary.tsv path is required")?;
    let content = fs::read_to_string(&path)?;
    summarize_tsv(&content, require_pass, fail_on_fixed_better, gate_config)
}

fn next_u64_arg(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    allow_zero: bool,
) -> SummaryResult<u64> {
    let value = args
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    if value.trim().is_empty() {
        return Err(format!("{flag} requires a non-empty value").into());
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|error| format!("{flag} invalid integer '{value}': {error}"))?;
    if !allow_zero && parsed == 0 {
        return Err(format!("{flag} must be greater than 0").into());
    }
    Ok(parsed)
}

fn next_f64_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> SummaryResult<f64> {
    let value = args
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    if value.trim().is_empty() {
        return Err(format!("{flag} requires a non-empty value").into());
    }
    let parsed = value
        .parse::<f64>()
        .map_err(|error| format!("{flag} invalid number '{value}': {error}"))?;
    if parsed < 0.0 {
        return Err(format!("{flag} must be greater than or equal to 0").into());
    }
    Ok(parsed)
}

fn next_list_arg(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> SummaryResult<Vec<String>> {
    let value = args
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    let values = parse_list_arg(&value);
    if values.is_empty() {
        return Err(format!("{flag} requires at least one value").into());
    }
    Ok(values)
}

fn parse_list_arg(value: &str) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut values = Vec::new();
    for item in value
        .split(|character: char| character == ',' || character.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if seen.insert(item.to_string()) {
            values.push(item.to_string());
        }
    }
    values
}

fn summarize_tsv(
    content: &str,
    require_pass: bool,
    fail_on_fixed_better: bool,
    gate_config: GateConfig,
) -> SummaryResult<MatrixReport> {
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
    let mut max_socket_write_batch = MaxMetric::default();
    let mut max_socket_write_batch_limit = MaxMetric::default();
    let mut min_socket_write_batch_limit = MinMetric::default();
    let mut min_throughput = MinFloatMetric::default();
    let mut min_echo_throughput = MinFloatMetric::default();
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
        let socket_write_batch = metric_text(row.socket_write_max_batch_frames);
        let socket_write_batch_limit = metric_text(row.socket_write_batch_limit_max);
        let socket_write_batch_limit_min = metric_text(row.socket_write_batch_limit_min);
        let socket_write_batch_limit_last = metric_text(row.socket_write_batch_limit_last);
        let throughput_mib_s = throughput_mib_s(row);
        let echo_throughput_mib_s = echo_throughput_mib_s(row);
        let relay_adaptive_high_sessions = text_value(row.relay_adaptive_high_sessions.as_deref());
        let relay_adaptive_elevated_dwell_us =
            text_value(row.relay_adaptive_elevated_dwell_us.as_deref());
        let relay_adaptive_severe_dwell_us =
            text_value(row.relay_adaptive_severe_dwell_us.as_deref());
        let relay_adaptive_elevated_cap = text_value(row.relay_adaptive_elevated_cap.as_deref());
        let relay_adaptive_severe_cap = text_value(row.relay_adaptive_severe_cap.as_deref());
        output.push('\n');
        output.push_str(&format!(
            "policy={} clients={} status={} elapsed_millis={} total_payload_bytes={} throughput_mib_s={} echo_elapsed_micros={} echo_throughput_mib_s={} rtt_micros_p95={} rtt_client_p95_spread_micros={} socket_read_max_batch_frames={} socket_write_max_batch_frames={} socket_write_batch_limit_max={} socket_write_batch_limit_min={} socket_write_batch_limit_last={} relay_adaptive_high_sessions={} relay_adaptive_elevated_dwell_us={} relay_adaptive_severe_dwell_us={} relay_adaptive_elevated_cap={} relay_adaptive_severe_cap={}",
            row.relay_batch_policy,
            row.clients,
            row.status,
            row.elapsed_millis,
            metric_text(row.total_payload_bytes),
            float_metric_text(throughput_mib_s),
            metric_text(row.echo_elapsed_micros),
            float_metric_text(echo_throughput_mib_s),
            rtt,
            spread,
            socket_batch,
            socket_write_batch,
            socket_write_batch_limit,
            socket_write_batch_limit_min,
            socket_write_batch_limit_last,
            relay_adaptive_high_sessions,
            relay_adaptive_elevated_dwell_us,
            relay_adaptive_severe_dwell_us,
            relay_adaptive_elevated_cap,
            relay_adaptive_severe_cap
        ));

        if row.status == "pass" {
            record_max_gate_failure(
                &mut gate_failures,
                row,
                "rtt_micros_p95",
                row.rtt_micros_p95,
                gate_config.max_rtt_p95_micros,
            );
            record_max_gate_failure(
                &mut gate_failures,
                row,
                "rtt_client_p95_spread_micros",
                row.client_p95_spread_micros,
                gate_config.max_client_p95_spread_micros,
            );
            record_min_throughput_gate_failure(
                &mut gate_failures,
                row,
                "throughput_mib_s",
                throughput_mib_s,
                gate_config.min_throughput_mib_s,
            );
            record_min_throughput_gate_failure(
                &mut gate_failures,
                row,
                "echo_throughput_mib_s",
                echo_throughput_mib_s,
                gate_config.min_echo_throughput_mib_s,
            );
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
            max_socket_write_batch.record(
                &row.relay_batch_policy,
                &row.clients,
                row.socket_write_max_batch_frames,
            );
            max_socket_write_batch_limit.record(
                &row.relay_batch_policy,
                &row.clients,
                row.socket_write_batch_limit_max,
            );
            min_socket_write_batch_limit.record(
                &row.relay_batch_policy,
                &row.clients,
                row.socket_write_batch_limit_min,
            );
            min_throughput.record(&row.relay_batch_policy, &row.clients, throughput_mib_s);
            min_echo_throughput.record(
                &row.relay_batch_policy,
                &row.clients,
                echo_throughput_mib_s,
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
    output.push('\n');
    output.push_str(&format!(
        "max_socket_write_max_batch_frames={} policy={} clients={}",
        metric_text(max_socket_write_batch.value),
        max_socket_write_batch.policy.as_deref().unwrap_or("-"),
        max_socket_write_batch.clients.as_deref().unwrap_or("-")
    ));
    output.push('\n');
    output.push_str(&format!(
        "max_socket_write_batch_limit_max={} policy={} clients={}",
        metric_text(max_socket_write_batch_limit.value),
        max_socket_write_batch_limit
            .policy
            .as_deref()
            .unwrap_or("-"),
        max_socket_write_batch_limit
            .clients
            .as_deref()
            .unwrap_or("-")
    ));
    output.push('\n');
    output.push_str(&format!(
        "min_socket_write_batch_limit_min={} policy={} clients={}",
        metric_text(min_socket_write_batch_limit.value),
        min_socket_write_batch_limit
            .policy
            .as_deref()
            .unwrap_or("-"),
        min_socket_write_batch_limit
            .clients
            .as_deref()
            .unwrap_or("-")
    ));
    output.push('\n');
    output.push_str(&format!(
        "min_throughput_mib_s={} policy={} clients={}",
        float_metric_text(min_throughput.value),
        min_throughput.policy.as_deref().unwrap_or("-"),
        min_throughput.clients.as_deref().unwrap_or("-")
    ));
    output.push('\n');
    output.push_str(&format!(
        "min_echo_throughput_mib_s={} policy={} clients={}",
        float_metric_text(min_echo_throughput.value),
        min_echo_throughput.policy.as_deref().unwrap_or("-"),
        min_echo_throughput.clients.as_deref().unwrap_or("-")
    ));

    if !gate_config.expected_policies.is_empty() {
        let missing = record_expected_matrix_failures(
            &mut gate_failures,
            &rows,
            &gate_config.expected_policies,
            &gate_config.expected_clients,
        );
        output.push('\n');
        output.push_str(&format!(
            "expected_matrix policies={} clients={} status={} missing={}",
            gate_config.expected_policies.join(","),
            gate_config.expected_clients.join(","),
            if missing == 0 { "pass" } else { "fail" },
            missing
        ));
    }

    for comparison in policy_comparisons(&rows) {
        output.push('\n');
        output.push_str(&comparison.report_line());
        output.push('\n');
        output.push_str(&comparison.recommendation_line());
        if fail_on_fixed_better && comparison.verdict == PolicyVerdict::FixedBetter {
            gate_failures.push(format!(
                "fixed_better tunnel matrix verdict failed KTP tunnel policy gate for clients={}",
                comparison.clients
            ));
        }
    }

    Ok(MatrixReport {
        output,
        gate_failures,
    })
}

fn record_expected_matrix_failures(
    gate_failures: &mut Vec<String>,
    rows: &[MatrixRow],
    expected_policies: &[String],
    expected_clients: &[String],
) -> usize {
    let observed = rows
        .iter()
        .map(|row| {
            (
                row.relay_batch_policy.as_str().to_string(),
                row.clients.as_str().to_string(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut missing = 0;
    for policy in expected_policies {
        for clients in expected_clients {
            if !observed.contains(&(policy.clone(), clients.clone())) {
                missing += 1;
                gate_failures.push(format!(
                    "missing tunnel matrix row policy={policy} clients={clients}"
                ));
            }
        }
    }
    missing
}

fn record_min_throughput_gate_failure(
    gate_failures: &mut Vec<String>,
    row: &MatrixRow,
    metric_name: &str,
    value: Option<f64>,
    min: Option<f64>,
) {
    let Some(min) = min else {
        return;
    };
    match value {
        Some(value) if value < min => gate_failures.push(format!(
            "tunnel matrix row policy={} clients={} {metric_name}={value:.3} below min {min:.3}",
            row.relay_batch_policy, row.clients
        )),
        None => gate_failures.push(format!(
            "tunnel matrix row policy={} clients={} {metric_name} missing for min {min:.3}",
            row.relay_batch_policy, row.clients
        )),
        _ => {}
    }
}

fn record_max_gate_failure(
    gate_failures: &mut Vec<String>,
    row: &MatrixRow,
    metric_name: &str,
    value: Option<u64>,
    max: Option<u64>,
) {
    let Some(max) = max else {
        return;
    };
    match value {
        Some(value) if value > max => gate_failures.push(format!(
            "tunnel matrix row policy={} clients={} {metric_name}={value} exceeds max {max}",
            row.relay_batch_policy, row.clients
        )),
        None => gate_failures.push(format!(
            "tunnel matrix row policy={} clients={} {metric_name} missing for max {max}",
            row.relay_batch_policy, row.clients
        )),
        _ => {}
    }
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
        relay_adaptive_high_sessions: optional_text(&fields, indexes.relay_adaptive_high_sessions),
        relay_adaptive_elevated_dwell_us: optional_text(
            &fields,
            indexes.relay_adaptive_elevated_dwell_us,
        ),
        relay_adaptive_severe_dwell_us: optional_text(
            &fields,
            indexes.relay_adaptive_severe_dwell_us,
        ),
        relay_adaptive_elevated_cap: optional_text(&fields, indexes.relay_adaptive_elevated_cap),
        relay_adaptive_severe_cap: optional_text(&fields, indexes.relay_adaptive_severe_cap),
        status,
        elapsed_millis,
        total_payload_bytes: parse_optional_metric(
            &fields,
            indexes.total_payload_bytes,
            "total_payload_bytes",
            false,
            line_number,
            &clients,
        )?,
        echo_elapsed_micros: parse_optional_metric(
            &fields,
            indexes.echo_elapsed_micros,
            "echo_elapsed_micros",
            false,
            line_number,
            &clients,
        )?,
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
        socket_write_max_batch_frames: parse_optional_metric(
            &fields,
            indexes.socket_write_max_batch_frames,
            "socket_write_max_batch_frames",
            pass_row,
            line_number,
            &clients,
        )?,
        socket_write_batch_limit_max: parse_optional_metric(
            &fields,
            indexes.socket_write_batch_limit_max,
            "socket_write_batch_limit_max",
            pass_row,
            line_number,
            &clients,
        )?,
        socket_write_batch_limit_min: parse_optional_metric(
            &fields,
            indexes.socket_write_batch_limit_min,
            "socket_write_batch_limit_min",
            pass_row,
            line_number,
            &clients,
        )?,
        socket_write_batch_limit_last: parse_optional_metric(
            &fields,
            indexes.socket_write_batch_limit_last,
            "socket_write_batch_limit_last",
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

fn optional_text(fields: &[&str], index: Option<usize>) -> Option<String> {
    index
        .and_then(|index| fields.get(index).copied())
        .filter(|value| !value.is_empty() && *value != "-")
        .map(str::to_string)
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

fn parse_optional_metric(
    fields: &[&str],
    index: Option<usize>,
    name: &str,
    required: bool,
    line_number: usize,
    clients: &str,
) -> SummaryResult<Option<u64>> {
    let Some(index) = index else {
        return Ok(None);
    };
    parse_metric(fields, index, name, required, line_number, clients)
}

fn metric_text(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn float_metric_text(value: Option<f64>) -> String {
    value
        .map(format_float_metric)
        .unwrap_or_else(|| "-".to_string())
}

fn format_float_metric(value: f64) -> String {
    if value != 0.0 && value.abs() < 0.001 {
        format!("{value:.6}")
    } else {
        format!("{value:.3}")
    }
}

fn throughput_mib_s(row: &MatrixRow) -> Option<f64> {
    let total_payload_bytes = row.total_payload_bytes?;
    if row.elapsed_millis == 0 {
        return None;
    }
    Some((total_payload_bytes as f64 / 1024.0 / 1024.0) / (row.elapsed_millis as f64 / 1000.0))
}

fn echo_throughput_mib_s(row: &MatrixRow) -> Option<f64> {
    let total_payload_bytes = row.total_payload_bytes?;
    let echo_elapsed_micros = row.echo_elapsed_micros?;
    if echo_elapsed_micros == 0 {
        return None;
    }
    Some(
        (total_payload_bytes as f64 / 1024.0 / 1024.0) / (echo_elapsed_micros as f64 / 1_000_000.0),
    )
}

fn text_value(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}

#[derive(Clone, Debug)]
struct PolicyComparison {
    clients: String,
    fixed_elapsed_millis: u64,
    adaptive_elapsed_millis: u64,
    fixed_rtt_micros_p95: u64,
    adaptive_rtt_micros_p95: u64,
    fixed_spread_micros: u64,
    adaptive_spread_micros: u64,
    verdict: PolicyVerdict,
}

impl PolicyComparison {
    fn report_line(&self) -> String {
        format!(
            "policy_compare clients={} fixed_elapsed_millis={} adaptive_elapsed_millis={} elapsed_delta_pct={:.2} fixed_rtt_micros_p95={} adaptive_rtt_micros_p95={} rtt_p95_delta_pct={:.2} fixed_rtt_client_p95_spread_micros={} adaptive_rtt_client_p95_spread_micros={} spread_delta_pct={:.2} verdict={}",
            self.clients,
            self.fixed_elapsed_millis,
            self.adaptive_elapsed_millis,
            percent_delta(self.adaptive_elapsed_millis, self.fixed_elapsed_millis),
            self.fixed_rtt_micros_p95,
            self.adaptive_rtt_micros_p95,
            percent_delta(self.adaptive_rtt_micros_p95, self.fixed_rtt_micros_p95),
            self.fixed_spread_micros,
            self.adaptive_spread_micros,
            percent_delta(self.adaptive_spread_micros, self.fixed_spread_micros),
            self.verdict.as_str(),
        )
    }

    fn recommendation_line(&self) -> String {
        let (recommended, reason) = self.verdict.recommendation();
        format!(
            "policy_recommend clients={} recommended={} verdict={} reason={}",
            self.clients,
            recommended,
            self.verdict.as_str(),
            reason
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PolicyVerdict {
    AdaptiveBetter,
    FixedBetter,
    Same,
    Mixed,
}

impl PolicyVerdict {
    fn from_metrics(
        fixed_elapsed_millis: u64,
        adaptive_elapsed_millis: u64,
        fixed_rtt_micros_p95: u64,
        adaptive_rtt_micros_p95: u64,
        fixed_spread_micros: u64,
        adaptive_spread_micros: u64,
    ) -> Self {
        let adaptive_not_worse = adaptive_elapsed_millis <= fixed_elapsed_millis
            && adaptive_rtt_micros_p95 <= fixed_rtt_micros_p95
            && adaptive_spread_micros <= fixed_spread_micros;
        let adaptive_strictly_better = adaptive_elapsed_millis < fixed_elapsed_millis
            || adaptive_rtt_micros_p95 < fixed_rtt_micros_p95
            || adaptive_spread_micros < fixed_spread_micros;
        let fixed_not_worse = fixed_elapsed_millis <= adaptive_elapsed_millis
            && fixed_rtt_micros_p95 <= adaptive_rtt_micros_p95
            && fixed_spread_micros <= adaptive_spread_micros;
        let fixed_strictly_better = fixed_elapsed_millis < adaptive_elapsed_millis
            || fixed_rtt_micros_p95 < adaptive_rtt_micros_p95
            || fixed_spread_micros < adaptive_spread_micros;

        if adaptive_not_worse && adaptive_strictly_better {
            Self::AdaptiveBetter
        } else if fixed_not_worse && fixed_strictly_better {
            Self::FixedBetter
        } else if !adaptive_strictly_better && !fixed_strictly_better {
            Self::Same
        } else {
            Self::Mixed
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::AdaptiveBetter => "adaptive_better",
            Self::FixedBetter => "fixed_better",
            Self::Same => "same",
            Self::Mixed => "mixed",
        }
    }

    fn recommendation(&self) -> (&'static str, &'static str) {
        match self {
            Self::AdaptiveBetter => ("adaptive", "adaptive_not_worse"),
            Self::FixedBetter => ("fixed", "fixed_not_worse"),
            Self::Same => ("fixed", "same_metrics_keep_default"),
            Self::Mixed => ("manual_review", "metric_tradeoff"),
        }
    }
}

#[derive(Default)]
struct PolicyPair<'a> {
    fixed: Option<&'a MatrixRow>,
    adaptive: Option<&'a MatrixRow>,
}

fn policy_comparisons(rows: &[MatrixRow]) -> Vec<PolicyComparison> {
    let mut pairs = BTreeMap::<&str, PolicyPair<'_>>::new();
    for row in rows {
        if row.status != "pass" {
            continue;
        }
        let pair = pairs.entry(row.clients.as_str()).or_default();
        match row.relay_batch_policy.as_str() {
            "fixed" => pair.fixed = Some(row),
            "adaptive" => pair.adaptive = Some(row),
            _ => {}
        }
    }

    pairs
        .into_iter()
        .filter_map(|(clients, pair)| {
            let fixed = pair.fixed?;
            let adaptive = pair.adaptive?;
            let fixed_rtt_micros_p95 = fixed.rtt_micros_p95?;
            let adaptive_rtt_micros_p95 = adaptive.rtt_micros_p95?;
            let fixed_spread_micros = fixed.client_p95_spread_micros?;
            let adaptive_spread_micros = adaptive.client_p95_spread_micros?;
            Some(PolicyComparison {
                clients: clients.to_string(),
                fixed_elapsed_millis: fixed.elapsed_millis,
                adaptive_elapsed_millis: adaptive.elapsed_millis,
                fixed_rtt_micros_p95,
                adaptive_rtt_micros_p95,
                fixed_spread_micros,
                adaptive_spread_micros,
                verdict: PolicyVerdict::from_metrics(
                    fixed.elapsed_millis,
                    adaptive.elapsed_millis,
                    fixed_rtt_micros_p95,
                    adaptive_rtt_micros_p95,
                    fixed_spread_micros,
                    adaptive_spread_micros,
                ),
            })
        })
        .collect()
}

fn percent_delta(candidate: u64, baseline: u64) -> f64 {
    if baseline == 0 {
        if candidate == 0 {
            0.0
        } else {
            100.0
        }
    } else {
        ((candidate as f64 - baseline as f64) / baseline as f64) * 100.0
    }
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

#[derive(Clone, Debug, Default)]
struct MinMetric {
    value: Option<u64>,
    policy: Option<String>,
    clients: Option<String>,
}

impl MinMetric {
    fn record(&mut self, policy: &str, clients: &str, value: Option<u64>) {
        let Some(value) = value else {
            return;
        };
        if self.value.is_none_or(|current| value < current) {
            self.value = Some(value);
            self.policy = Some(policy.to_string());
            self.clients = Some(clients.to_string());
        }
    }
}

#[derive(Clone, Debug, Default)]
struct MinFloatMetric {
    value: Option<f64>,
    policy: Option<String>,
    clients: Option<String>,
}

impl MinFloatMetric {
    fn record(&mut self, policy: &str, clients: &str, value: Option<f64>) {
        let Some(value) = value else {
            return;
        };
        if self.value.is_none_or(|current| value < current) {
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
    relay_adaptive_high_sessions: Option<usize>,
    relay_adaptive_elevated_dwell_us: Option<usize>,
    relay_adaptive_severe_dwell_us: Option<usize>,
    relay_adaptive_elevated_cap: Option<usize>,
    relay_adaptive_severe_cap: Option<usize>,
    status: usize,
    elapsed_millis: usize,
    total_payload_bytes: Option<usize>,
    echo_elapsed_micros: Option<usize>,
    rtt_micros_p95: usize,
    rtt_client_p95_spread_micros: usize,
    socket_read_max_batch_frames: usize,
    socket_write_max_batch_frames: Option<usize>,
    socket_write_batch_limit_max: Option<usize>,
    socket_write_batch_limit_min: Option<usize>,
    socket_write_batch_limit_last: Option<usize>,
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
            relay_adaptive_high_sessions: positions.get("relay_adaptive_high_sessions").copied(),
            relay_adaptive_elevated_dwell_us: positions
                .get("relay_adaptive_elevated_dwell_us")
                .copied(),
            relay_adaptive_severe_dwell_us: positions
                .get("relay_adaptive_severe_dwell_us")
                .copied(),
            relay_adaptive_elevated_cap: positions.get("relay_adaptive_elevated_cap").copied(),
            relay_adaptive_severe_cap: positions.get("relay_adaptive_severe_cap").copied(),
            status: required_column(&positions, "status")?,
            elapsed_millis: required_column(&positions, "elapsed_millis")?,
            total_payload_bytes: positions.get("total_payload_bytes").copied(),
            echo_elapsed_micros: positions.get("echo_elapsed_micros").copied(),
            rtt_micros_p95: required_column(&positions, "rtt_micros_p95")?,
            rtt_client_p95_spread_micros: required_column(
                &positions,
                "rtt_client_p95_spread_micros",
            )?,
            socket_read_max_batch_frames: required_column(
                &positions,
                "socket_read_max_batch_frames",
            )?,
            socket_write_max_batch_frames: positions.get("socket_write_max_batch_frames").copied(),
            socket_write_batch_limit_max: positions.get("socket_write_batch_limit_max").copied(),
            socket_write_batch_limit_min: positions.get("socket_write_batch_limit_min").copied(),
            socket_write_batch_limit_last: positions.get("socket_write_batch_limit_last").copied(),
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
    eprintln!(
        "usage: ktp-tunnel-matrix-summary [--require-pass] [--fail-on-fixed-better] [--max-rtt-p95-micros N] [--max-client-p95-spread-micros N] [--min-throughput-mib-s N] [--min-echo-throughput-mib-s N] [--expect-policies LIST --expect-clients LIST] <matrix-summary.tsv>"
    );
}
