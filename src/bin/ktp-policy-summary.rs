use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fs;

type SummaryResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug)]
struct PolicyRow {
    clients: usize,
    relay_batch_frames: usize,
    policy: String,
    effective_batch_frames: usize,
    throughput_mib_s_median: f64,
    rtt_micros_p95: f64,
    client_p95_spread_micros: f64,
}

#[derive(Clone, Debug, Default)]
struct PolicyPair {
    fixed: Option<PolicyRow>,
    adaptive: Option<PolicyRow>,
}

fn main() {
    match run(std::env::args().skip(1)) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            print_usage();
            std::process::exit(2);
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> SummaryResult<String> {
    let path = args
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or("CSV path is required")?;
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }

    let content = fs::read_to_string(&path)?;
    summarize_csv(&content)
}

fn summarize_csv(content: &str) -> SummaryResult<String> {
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("CSV is empty")?;
    let indexes = CsvIndexes::from_header(header)?;
    let mut row_count = 0usize;
    let mut pairs = BTreeMap::<(usize, usize), PolicyPair>::new();

    for line in lines {
        row_count += 1;
        let row = parse_row(line, &indexes)?;
        let pair = pairs
            .entry((row.clients, row.relay_batch_frames))
            .or_default();
        match row.policy.as_str() {
            "fixed" => pair.fixed = Some(row),
            "adaptive" => pair.adaptive = Some(row),
            _ => {}
        }
    }

    let complete_pairs = pairs
        .values()
        .filter(|pair| pair.fixed.is_some() && pair.adaptive.is_some())
        .count();
    if complete_pairs == 0 {
        return Err("no fixed/adaptive policy pairs found".into());
    }

    let mut output = format!("ktp_policy_summary rows={row_count} pairs={complete_pairs}");
    for ((_clients, _batch), pair) in pairs {
        let (Some(fixed), Some(adaptive)) = (pair.fixed, pair.adaptive) else {
            continue;
        };
        output.push('\n');
        output.push_str(&format!(
            "clients={} relay_batch_frames={} fixed_effective={} adaptive_effective={} throughput_delta_pct={:.2} rtt_p95_delta_pct={:.2} client_p95_spread_delta_pct={:.2} verdict={}",
            fixed.clients,
            fixed.relay_batch_frames,
            fixed.effective_batch_frames,
            adaptive.effective_batch_frames,
            percent_delta(adaptive.throughput_mib_s_median, fixed.throughput_mib_s_median),
            percent_delta(adaptive.rtt_micros_p95, fixed.rtt_micros_p95),
            percent_delta(adaptive.client_p95_spread_micros, fixed.client_p95_spread_micros),
            verdict(&fixed, &adaptive)
        ));
    }
    Ok(output)
}

fn parse_row(line: &str, indexes: &CsvIndexes) -> SummaryResult<PolicyRow> {
    let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
    Ok(PolicyRow {
        clients: parse_field(&fields, indexes.clients, "clients")?,
        relay_batch_frames: parse_field(&fields, indexes.relay_batch_frames, "relay_batch_frames")?,
        policy: field(&fields, indexes.relay_batch_policy, "relay_batch_policy")?.to_string(),
        effective_batch_frames: parse_field(
            &fields,
            indexes.relay_batch_frames_effective,
            "relay_batch_frames_effective",
        )?,
        throughput_mib_s_median: parse_field(
            &fields,
            indexes.throughput_mib_s_median,
            "throughput_mib_s_median",
        )?,
        rtt_micros_p95: parse_field(&fields, indexes.rtt_micros_p95, "rtt_micros_p95")?,
        client_p95_spread_micros: parse_field(
            &fields,
            indexes.rtt_client_p95_spread_micros,
            "rtt_client_p95_spread_micros",
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

fn parse_field<T: std::str::FromStr>(fields: &[&str], index: usize, name: &str) -> SummaryResult<T>
where
    T::Err: Error + Send + Sync + 'static,
{
    Ok(field(fields, index, name)?.parse::<T>()?)
}

fn percent_delta(candidate: f64, baseline: f64) -> f64 {
    if baseline == 0.0 {
        if candidate == 0.0 {
            0.0
        } else if candidate.is_sign_positive() {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }
    } else {
        ((candidate - baseline) / baseline) * 100.0
    }
}

fn verdict(fixed: &PolicyRow, adaptive: &PolicyRow) -> &'static str {
    let throughput_improved = adaptive.throughput_mib_s_median >= fixed.throughput_mib_s_median;
    let rtt_improved = adaptive.rtt_micros_p95 <= fixed.rtt_micros_p95;
    let spread_improved = adaptive.client_p95_spread_micros <= fixed.client_p95_spread_micros;

    if throughput_improved && rtt_improved && spread_improved {
        "adaptive_better"
    } else if !throughput_improved && !rtt_improved && !spread_improved {
        "fixed_better"
    } else {
        "mixed"
    }
}

#[derive(Clone, Copy, Debug)]
struct CsvIndexes {
    clients: usize,
    relay_batch_frames: usize,
    relay_batch_policy: usize,
    relay_batch_frames_effective: usize,
    throughput_mib_s_median: usize,
    rtt_micros_p95: usize,
    rtt_client_p95_spread_micros: usize,
}

impl CsvIndexes {
    fn from_header(header: &str) -> SummaryResult<Self> {
        let positions = header
            .split(',')
            .enumerate()
            .map(|(index, name)| (name.trim().to_string(), index))
            .collect::<HashMap<_, _>>();
        Ok(Self {
            clients: required_column(&positions, "clients")?,
            relay_batch_frames: required_column(&positions, "relay_batch_frames")?,
            relay_batch_policy: required_column(&positions, "relay_batch_policy")?,
            relay_batch_frames_effective: required_column(
                &positions,
                "relay_batch_frames_effective",
            )?,
            throughput_mib_s_median: required_column(&positions, "throughput_mib_s_median")?,
            rtt_micros_p95: required_column(&positions, "rtt_micros_p95")?,
            rtt_client_p95_spread_micros: required_column(
                &positions,
                "rtt_client_p95_spread_micros",
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
    eprintln!("usage: ktp-policy-summary <ktp-relay-batch-matrix.csv>");
}
