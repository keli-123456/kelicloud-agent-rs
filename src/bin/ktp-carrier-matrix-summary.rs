use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::process;

const DIRECTION_CLIENT_TO_RELAY: &str = "client_to_relay";
const DIRECTION_BATCH_WRITE: &str = "client_to_relay_batch_write";
const DIRECTION_BATCH_READ: &str = "relay_to_client_batch_read";

fn main() {
    match run(env::args().skip(1)) {
        Ok(report) => {
            println!("{}", report.output);
        }
        Err(error) => {
            print_usage();
            eprintln!("{}", error.message);
            process::exit(error.exit_code);
        }
    }
}

#[derive(Clone, Debug)]
struct SummaryReport {
    output: String,
}

#[derive(Clone, Debug)]
struct SummaryError {
    exit_code: i32,
    message: String,
}

impl SummaryError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            message: message.into(),
        }
    }

    fn gate(failures: Vec<String>) -> Self {
        Self {
            exit_code: 3,
            message: failures.join("\n"),
        }
    }
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

type SummaryResult<T> = Result<T, SummaryError>;

#[derive(Clone, Copy, Debug, Default)]
struct SummaryOptions {
    require_ktp_aead: bool,
    require_batch_reuse: bool,
    require_positive_throughput: bool,
    min_batch_write_throughput_mib_s: Option<f64>,
    min_batch_read_throughput_mib_s: Option<f64>,
}

#[derive(Clone, Debug)]
struct CarrierRow {
    carrier: String,
    crypto: String,
    direction: String,
    write_batch_frames: u64,
    write_batch_reused: u64,
    read_batch_frames: u64,
    read_batch_reused: u64,
    throughput_mib_s_median: f64,
}

fn run(args: impl Iterator<Item = String>) -> SummaryResult<SummaryReport> {
    let mut options = SummaryOptions::default();
    let mut path = None::<String>;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--require-ktp-aead" => options.require_ktp_aead = true,
            "--require-batch-reuse" => options.require_batch_reuse = true,
            "--require-positive-throughput" => options.require_positive_throughput = true,
            "--min-batch-write-throughput-mib-s" => {
                options.min_batch_write_throughput_mib_s = Some(next_f64_arg(
                    &mut args,
                    "--min-batch-write-throughput-mib-s",
                )?);
            }
            "--min-batch-read-throughput-mib-s" => {
                options.min_batch_read_throughput_mib_s = Some(next_f64_arg(
                    &mut args,
                    "--min-batch-read-throughput-mib-s",
                )?);
            }
            _ if arg.trim().is_empty() => {
                return Err(SummaryError::usage("empty argument is not allowed"))
            }
            _ if path.is_none() => path = Some(arg),
            _ => return Err(SummaryError::usage("unexpected extra argument")),
        }
    }

    let path = path.ok_or_else(|| SummaryError::usage("matrix CSV path is required"))?;
    let content = fs::read_to_string(&path)
        .map_err(|error| SummaryError::usage(format!("failed to read {path}: {error}")))?;
    summarize_csv(&content, options)
}

fn summarize_csv(content: &str, options: SummaryOptions) -> SummaryResult<SummaryReport> {
    let rows = parse_rows(content)?;
    let mut by_direction = HashMap::<String, CarrierRow>::new();
    for row in rows {
        by_direction.insert(row.direction.clone(), row);
    }

    let mut failures = Vec::<String>::new();
    let client_to_relay = required_row(&by_direction, DIRECTION_CLIENT_TO_RELAY, &mut failures);
    let batch_write = required_row(&by_direction, DIRECTION_BATCH_WRITE, &mut failures);
    let batch_read = required_row(&by_direction, DIRECTION_BATCH_READ, &mut failures);

    if let Some(rows) = all_required_rows(client_to_relay, batch_write, batch_read) {
        if options.require_ktp_aead {
            for row in rows {
                if row.carrier != "ktp_tcp" || row.crypto != "ktp_aead" {
                    failures.push(format!(
                        "{} expected carrier=ktp_tcp crypto=ktp_aead, got carrier={} crypto={}",
                        row.direction, row.carrier, row.crypto
                    ));
                }
            }
        }

        if options.require_positive_throughput {
            for row in rows {
                if row.throughput_mib_s_median <= 0.0 {
                    failures.push(format!(
                        "{} throughput_mib_s_median must be positive",
                        row.direction
                    ));
                }
            }
        }
    }

    if let Some(row) = batch_write {
        if options.require_batch_reuse {
            if row.write_batch_frames == 0 {
                failures.push(
                    "client_to_relay_batch_write write_batch_frames must be positive".to_string(),
                );
            }
            if row.write_batch_reused != 1 {
                failures
                    .push("client_to_relay_batch_write write_batch_reused is not 1".to_string());
            }
        }
        if let Some(min) = options.min_batch_write_throughput_mib_s {
            if row.throughput_mib_s_median < min {
                failures.push(format!(
                    "client_to_relay_batch_write throughput_mib_s_median {:.3} below min {:.3}",
                    row.throughput_mib_s_median, min
                ));
            }
        }
    }

    if let Some(row) = batch_read {
        if options.require_batch_reuse {
            if row.read_batch_frames == 0 {
                failures.push(
                    "relay_to_client_batch_read read_batch_frames must be positive".to_string(),
                );
            }
            if row.read_batch_reused != 1 {
                failures.push("relay_to_client_batch_read read_batch_reused is not 1".to_string());
            }
        }
        if let Some(min) = options.min_batch_read_throughput_mib_s {
            if row.throughput_mib_s_median < min {
                failures.push(format!(
                    "relay_to_client_batch_read throughput_mib_s_median {:.3} below min {:.3}",
                    row.throughput_mib_s_median, min
                ));
            }
        }
    }

    if !failures.is_empty() {
        return Err(SummaryError::gate(failures));
    }

    let client_to_relay = client_to_relay.expect("required row gate already passed");
    let batch_write = batch_write.expect("required row gate already passed");
    let batch_read = batch_read.expect("required row gate already passed");
    Ok(SummaryReport {
        output: format!(
            "ktp_carrier_matrix_summary rows={} gate=pass\nclient_to_relay_throughput_mib_s_median={:.3}\nbatch_write_throughput_mib_s_median={:.3}\nbatch_read_throughput_mib_s_median={:.3}",
            by_direction.len(),
            client_to_relay.throughput_mib_s_median,
            batch_write.throughput_mib_s_median,
            batch_read.throughput_mib_s_median
        ),
    })
}

fn parse_rows(content: &str) -> SummaryResult<Vec<CarrierRow>> {
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| SummaryError::usage("carrier matrix CSV is empty"))?;
    let columns = header
        .split(',')
        .enumerate()
        .map(|(index, name)| (name.trim().to_string(), index))
        .collect::<HashMap<_, _>>();

    let mut rows = Vec::new();
    for (line_index, line) in lines.enumerate() {
        let fields = line.split(',').map(str::trim).collect::<Vec<_>>();
        rows.push(CarrierRow {
            carrier: field(&columns, &fields, "carrier", line_index)?.to_string(),
            crypto: field(&columns, &fields, "crypto", line_index)?.to_string(),
            direction: field(&columns, &fields, "direction", line_index)?.to_string(),
            write_batch_frames: parse_u64_field(
                &columns,
                &fields,
                "write_batch_frames",
                line_index,
            )?,
            write_batch_reused: parse_u64_field(
                &columns,
                &fields,
                "write_batch_reused",
                line_index,
            )?,
            read_batch_frames: parse_u64_field(&columns, &fields, "read_batch_frames", line_index)?,
            read_batch_reused: parse_u64_field(&columns, &fields, "read_batch_reused", line_index)?,
            throughput_mib_s_median: parse_f64_field(
                &columns,
                &fields,
                "throughput_mib_s_median",
                line_index,
            )?,
        });
    }

    if rows.is_empty() {
        return Err(SummaryError::usage("carrier matrix CSV has no rows"));
    }
    Ok(rows)
}

fn required_row<'a>(
    by_direction: &'a HashMap<String, CarrierRow>,
    direction: &str,
    failures: &mut Vec<String>,
) -> Option<&'a CarrierRow> {
    match by_direction.get(direction) {
        Some(row) => Some(row),
        None => {
            failures.push(format!("missing carrier matrix row direction={direction}"));
            None
        }
    }
}

fn all_required_rows<'a>(
    client_to_relay: Option<&'a CarrierRow>,
    batch_write: Option<&'a CarrierRow>,
    batch_read: Option<&'a CarrierRow>,
) -> Option<[&'a CarrierRow; 3]> {
    Some([client_to_relay?, batch_write?, batch_read?])
}

fn field<'a>(
    columns: &HashMap<String, usize>,
    fields: &'a [&str],
    name: &str,
    line_index: usize,
) -> SummaryResult<&'a str> {
    let column = columns
        .get(name)
        .ok_or_else(|| SummaryError::usage(format!("missing required CSV column: {name}")))?;
    fields.get(*column).copied().ok_or_else(|| {
        SummaryError::usage(format!(
            "line {} missing value for required CSV column: {name}",
            line_index + 2
        ))
    })
}

fn parse_u64_field(
    columns: &HashMap<String, usize>,
    fields: &[&str],
    name: &str,
    line_index: usize,
) -> SummaryResult<u64> {
    let value = field(columns, fields, name, line_index)?;
    value.parse::<u64>().map_err(|_| {
        SummaryError::usage(format!(
            "line {} column {name} must be an unsigned integer",
            line_index + 2
        ))
    })
}

fn parse_f64_field(
    columns: &HashMap<String, usize>,
    fields: &[&str],
    name: &str,
    line_index: usize,
) -> SummaryResult<f64> {
    let value = field(columns, fields, name, line_index)?;
    value.parse::<f64>().map_err(|_| {
        SummaryError::usage(format!(
            "line {} column {name} must be a number",
            line_index + 2
        ))
    })
}

fn next_f64_arg(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    name: &str,
) -> SummaryResult<f64> {
    let value = args
        .next()
        .ok_or_else(|| SummaryError::usage(format!("{name} value is required")))?;
    let parsed = value
        .parse::<f64>()
        .map_err(|_| SummaryError::usage(format!("{name} value must be a number")))?;
    if parsed < 0.0 {
        return Err(SummaryError::usage(format!(
            "{name} value must be greater than or equal to zero"
        )));
    }
    Ok(parsed)
}

fn print_usage() {
    eprintln!(
        "usage: ktp-carrier-matrix-summary [--require-ktp-aead] [--require-batch-reuse] [--require-positive-throughput] [--min-batch-write-throughput-mib-s N] [--min-batch-read-throughput-mib-s N] <ktp-carrier-matrix.csv>"
    );
}
