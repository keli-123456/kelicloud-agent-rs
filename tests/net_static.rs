use kelicloud_agent_rs::linux_proc::{
    parse_net_static_total_between, NetworkFilter, NetworkTotals,
};
use kelicloud_agent_rs::net_static::{InterfaceCounter, NetStaticSampler, NetStaticSamplerConfig};
use std::fs;

#[test]
fn sampler_records_deltas_and_flushes_go_compatible_json() {
    let path = temp_net_static_path("delta");
    let mut sampler = NetStaticSampler::with_config(NetStaticSamplerConfig {
        path: path.clone(),
        data_preserve_days: 31.0,
        detect_interval_seconds: 2.0,
        save_interval_seconds: 600.0,
        nics: Vec::new(),
    });
    let filter = NetworkFilter::default();

    sampler.sample(100, &[InterfaceCounter::new("eth0", 1_000, 2_000)]);
    sampler.sample(101, &[InterfaceCounter::new("eth0", 1_100, 2_200)]);
    assert_eq!(
        sampler.total_between(0, 200, &filter),
        NetworkTotals::default()
    );

    sampler.sample(102, &[InterfaceCounter::new("eth0", 1_300, 2_500)]);
    assert_eq!(
        sampler.total_between(0, 200, &filter),
        NetworkTotals {
            total_up: 300,
            total_down: 500,
        }
    );

    sampler.flush(160).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed = parse_net_static_total_between(&contents, 0, 200, &filter).unwrap();
    drop(sampler);
    let _ = fs::remove_file(path);

    assert!(contents.contains("\"interfaces\""));
    assert!(contents.contains("\"config\""));
    assert_eq!(parsed.total_up, 300);
    assert_eq!(parsed.total_down, 500);
}

#[test]
fn sampler_prunes_expired_records_on_load_like_go_agent() {
    let path = temp_net_static_path("load-prune");
    let now = chrono::Local::now().timestamp().max(0) as u64;
    let old = now.saturating_sub(2 * 24 * 60 * 60);
    fs::write(
        &path,
        format!(
            r#"{{"interfaces":{{"eth0":[{{"timestamp":{old},"tx":10,"rx":20}},{{"timestamp":{now},"tx":30,"rx":40}}]}},"config":{{"data_preserve_day":1,"detect_interval":2,"save_interval":600,"nics":[]}}}}"#
        ),
    )
    .unwrap();

    let sampler = NetStaticSampler::with_config(NetStaticSamplerConfig {
        path: path.clone(),
        data_preserve_days: 1.0,
        detect_interval_seconds: 2.0,
        save_interval_seconds: 600.0,
        nics: Vec::new(),
    });

    let totals = sampler.total_between(0, now + 1, &NetworkFilter::default());
    drop(sampler);
    let _ = fs::remove_file(path);

    assert_eq!(totals.total_up, 30);
    assert_eq!(totals.total_down, 40);
}

#[test]
fn sampler_loads_existing_file_and_prunes_expired_records_on_load() {
    let path = temp_net_static_path("load");
    let now = chrono::Local::now().timestamp().max(0) as u64;
    let old = now.saturating_sub(2 * 24 * 60 * 60);
    fs::write(
        &path,
        format!(
            r#"{{"interfaces":{{"eth0":[{{"timestamp":{old},"tx":10,"rx":20}},{{"timestamp":{now},"tx":30,"rx":40}}]}},"config":{{"data_preserve_day":1,"detect_interval":2,"save_interval":600,"nics":[]}}}}"#
        ),
    )
    .unwrap();

    let mut sampler = NetStaticSampler::with_config(NetStaticSamplerConfig {
        path: path.clone(),
        data_preserve_days: 1.0,
        detect_interval_seconds: 2.0,
        save_interval_seconds: 600.0,
        nics: Vec::new(),
    });

    assert_eq!(
        sampler.total_between(0, now + 1, &NetworkFilter::default()),
        NetworkTotals {
            total_up: 30,
            total_down: 40,
        }
    );

    sampler.flush(now).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed =
        parse_net_static_total_between(&contents, 0, now + 1, &NetworkFilter::default()).unwrap();
    drop(sampler);
    let _ = fs::remove_file(path);

    assert_eq!(parsed.total_up, 30);
    assert_eq!(parsed.total_down, 40);
}

#[test]
fn sampler_uses_persisted_preserve_days_like_go_agent() {
    let path = temp_net_static_path("persisted-config");
    let now = chrono::Local::now().timestamp().max(0) as u64;
    let old = now.saturating_sub(2 * 24 * 60 * 60);
    fs::write(
        &path,
        format!(
            r#"{{"interfaces":{{"eth0":[{{"timestamp":{old},"tx":10,"rx":20}},{{"timestamp":{now},"tx":30,"rx":40}}]}},"config":{{"data_preserve_day":1,"detect_interval":2,"save_interval":600,"nics":[]}}}}"#
        ),
    )
    .unwrap();

    let mut sampler = NetStaticSampler::with_config(NetStaticSamplerConfig {
        path: path.clone(),
        ..NetStaticSamplerConfig::default()
    });

    sampler.flush(now).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    let parsed =
        parse_net_static_total_between(&contents, 0, now + 1, &NetworkFilter::default()).unwrap();
    drop(sampler);
    let _ = fs::remove_file(path);

    assert_eq!(parsed.total_up, 30);
    assert_eq!(parsed.total_down, 40);
}

#[test]
fn sampler_uses_persisted_nics_whitelist_like_go_agent() {
    let path = temp_net_static_path("persisted-nics");
    fs::write(
        &path,
        r#"{"interfaces":{},"config":{"data_preserve_day":31,"detect_interval":2,"save_interval":600,"nics":["eth0"]}}"#,
    )
    .unwrap();

    let mut sampler = NetStaticSampler::with_config(NetStaticSamplerConfig {
        path: path.clone(),
        ..NetStaticSamplerConfig::default()
    });
    let filter = NetworkFilter::default();

    sampler.sample(
        100,
        &[
            InterfaceCounter::new("eth0", 1_000, 2_000),
            InterfaceCounter::new("ens18", 10_000, 20_000),
        ],
    );
    sampler.sample(
        102,
        &[
            InterfaceCounter::new("eth0", 1_100, 2_200),
            InterfaceCounter::new("ens18", 10_900, 20_900),
        ],
    );

    let totals = sampler.total_between(0, 200, &filter);
    drop(sampler);
    let _ = fs::remove_file(path);

    assert_eq!(
        totals,
        NetworkTotals {
            total_up: 100,
            total_down: 200,
        }
    );
}

#[test]
fn parse_net_static_total_between_rejects_negative_counters_like_go_agent() {
    let contents = r#"
{
  "interfaces": {
    "eth0": [
      {"timestamp": 100, "tx": -10, "rx": 20}
    ]
  }
}
"#;

    let parsed = parse_net_static_total_between(contents, 0, 200, &NetworkFilter::default());

    assert_eq!(parsed, None);
}

#[test]
fn parse_net_static_total_between_accepts_missing_interfaces_like_go_agent() {
    let parsed = parse_net_static_total_between("{}", 0, 200, &NetworkFilter::default());

    assert_eq!(parsed, Some(NetworkTotals::default()));
}

fn temp_net_static_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kelicloud-agent-rs-net-static-{name}-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ))
}
