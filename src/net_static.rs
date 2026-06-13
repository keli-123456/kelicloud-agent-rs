use crate::linux_proc::{NetworkFilter, NetworkTotals};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

const DEFAULT_DATA_PRESERVE_DAYS: f64 = 31.0;
const DEFAULT_DETECT_INTERVAL_SECONDS: f64 = 2.0;
const DEFAULT_SAVE_INTERVAL_SECONDS: f64 = 600.0;

#[derive(Debug, Clone, PartialEq)]
pub struct NetStaticSamplerConfig {
    pub path: PathBuf,
    pub data_preserve_days: f64,
    pub detect_interval_seconds: f64,
    pub save_interval_seconds: f64,
    pub nics: Vec<String>,
}

impl Default for NetStaticSamplerConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("net_static.json"),
            data_preserve_days: DEFAULT_DATA_PRESERVE_DAYS,
            detect_interval_seconds: DEFAULT_DETECT_INTERVAL_SECONDS,
            save_interval_seconds: DEFAULT_SAVE_INTERVAL_SECONDS,
            nics: Vec::new(),
        }
    }
}

impl NetStaticSamplerConfig {
    fn normalized(mut self) -> Self {
        if self.data_preserve_days == 0.0 {
            self.data_preserve_days = DEFAULT_DATA_PRESERVE_DAYS;
        }
        if self.detect_interval_seconds == 0.0 {
            self.detect_interval_seconds = DEFAULT_DETECT_INTERVAL_SECONDS;
        }
        if self.save_interval_seconds == 0.0 {
            self.save_interval_seconds = DEFAULT_SAVE_INTERVAL_SECONDS;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCounter {
    pub name: String,
    pub total_up: u64,
    pub total_down: u64,
}

impl InterfaceCounter {
    pub fn new(name: impl Into<String>, total_up: u64, total_down: u64) -> Self {
        Self {
            name: name.into(),
            total_up,
            total_down,
        }
    }
}

#[derive(Debug)]
pub struct NetStaticSampler {
    config: NetStaticSamplerConfig,
    store: HashMap<String, Vec<TrafficData>>,
    cache: HashMap<String, Vec<TrafficData>>,
    last_counters: HashMap<String, CounterTotals>,
    last_sample_at: Option<u64>,
    last_save_at: Option<u64>,
}

impl NetStaticSampler {
    pub fn with_config(config: NetStaticSamplerConfig) -> Self {
        let config = config.normalized();
        let mut sampler = Self {
            config,
            store: HashMap::new(),
            cache: HashMap::new(),
            last_counters: HashMap::new(),
            last_sample_at: None,
            last_save_at: None,
        };
        sampler.load_from_file();
        sampler
    }

    pub fn sample(&mut self, timestamp: u64, counters: &[InterfaceCounter]) {
        if let Some(last_sample_at) = self.last_sample_at {
            if (timestamp.saturating_sub(last_sample_at) as f64)
                < self.config.detect_interval_seconds
            {
                return;
            }
        }

        for counter in counters {
            if !self.nic_allowed(&counter.name) {
                continue;
            }
            if let Some(previous) = self.last_counters.get(&counter.name) {
                let tx = counter.total_up.saturating_sub(previous.tx);
                let rx = counter.total_down.saturating_sub(previous.rx);
                if tx > 0 || rx > 0 {
                    self.cache
                        .entry(counter.name.clone())
                        .or_default()
                        .push(TrafficData { timestamp, tx, rx });
                }
            }
            self.last_counters.insert(
                counter.name.clone(),
                CounterTotals {
                    tx: counter.total_up,
                    rx: counter.total_down,
                },
            );
        }

        self.last_sample_at = Some(timestamp);
    }

    pub fn flush_if_due(&mut self, timestamp: u64) -> io::Result<()> {
        let should_flush = self.last_save_at.map_or(true, |last_save_at| {
            (timestamp.saturating_sub(last_save_at) as f64) >= self.config.save_interval_seconds
        });
        if should_flush {
            self.flush(timestamp)?;
        }
        Ok(())
    }

    pub fn flush(&mut self, timestamp: u64) -> io::Result<()> {
        self.flush_cache(timestamp);
        self.purge_expired(timestamp);
        self.save_to_file()?;
        self.last_save_at = Some(timestamp);
        Ok(())
    }

    pub fn total_between(&self, start: u64, end: u64, filter: &NetworkFilter) -> NetworkTotals {
        let mut totals = NetworkTotals::default();
        self.add_totals_between(&self.store, start, end, filter, &mut totals);
        self.add_totals_between(&self.cache, start, end, filter, &mut totals);
        totals
    }

    fn load_from_file(&mut self) {
        let Ok(contents) = fs::read_to_string(&self.config.path) else {
            return;
        };
        if contents.trim().is_empty() {
            return;
        }
        match serde_json::from_str::<NetStaticFile>(&contents) {
            Ok(file) => self.store = file.interfaces,
            Err(_) => {
                let _ = fs::rename(
                    &self.config.path,
                    self.config.path.with_extension("json.bak"),
                );
            }
        }
    }

    fn save_to_file(&self) -> io::Result<()> {
        if let Some(parent) = self.config.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let file = NetStaticFile {
            interfaces: self.store.clone(),
            config: NetStaticFileConfig {
                data_preserve_day: self.config.data_preserve_days,
                detect_interval: self.config.detect_interval_seconds,
                save_interval: self.config.save_interval_seconds,
                nics: self.config.nics.clone(),
            },
        };
        let bytes = serde_json::to_vec(&file).map_err(io::Error::other)?;
        let tmp = self.config.path.with_extension("json.tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, &self.config.path)
    }

    fn flush_cache(&mut self, timestamp: u64) {
        if self.cache.is_empty() {
            return;
        }

        for (name, entries) in std::mem::take(&mut self.cache) {
            let tx = entries.iter().map(|entry| entry.tx).sum::<u64>();
            let rx = entries.iter().map(|entry| entry.rx).sum::<u64>();
            if tx > 0 || rx > 0 {
                self.store
                    .entry(name)
                    .or_default()
                    .push(TrafficData { timestamp, tx, rx });
            }
        }
    }

    fn purge_expired(&mut self, now: u64) {
        let ttl = (self.config.data_preserve_days * 24.0 * 60.0 * 60.0).max(0.0) as u64;
        let cutoff = now.saturating_sub(ttl);
        self.store.retain(|_, entries| {
            entries.retain(|entry| entry.timestamp >= cutoff);
            !entries.is_empty()
        });
    }

    fn add_totals_between(
        &self,
        values: &HashMap<String, Vec<TrafficData>>,
        start: u64,
        end: u64,
        filter: &NetworkFilter,
        totals: &mut NetworkTotals,
    ) {
        for (name, entries) in values {
            if !filter.should_include(name) {
                continue;
            }
            for entry in entries {
                if (start != 0 && entry.timestamp < start) || (end != 0 && entry.timestamp > end) {
                    continue;
                }
                totals.total_up = totals.total_up.saturating_add(u64_to_i64(entry.tx));
                totals.total_down = totals.total_down.saturating_add(u64_to_i64(entry.rx));
            }
        }
    }

    fn nic_allowed(&self, name: &str) -> bool {
        self.config.nics.is_empty() || self.config.nics.iter().any(|nic| nic == name)
    }
}

impl Drop for NetStaticSampler {
    fn drop(&mut self) {
        let timestamp = chrono::Local::now().timestamp().max(0) as u64;
        let _ = self.flush(timestamp);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CounterTotals {
    tx: u64,
    rx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct NetStaticFile {
    #[serde(default)]
    interfaces: HashMap<String, Vec<TrafficData>>,
    #[serde(default)]
    config: NetStaticFileConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetStaticFileConfig {
    #[serde(default)]
    data_preserve_day: f64,
    #[serde(default)]
    detect_interval: f64,
    #[serde(default)]
    save_interval: f64,
    #[serde(default)]
    nics: Vec<String>,
}

impl Default for NetStaticFileConfig {
    fn default() -> Self {
        Self {
            data_preserve_day: DEFAULT_DATA_PRESERVE_DAYS,
            detect_interval: DEFAULT_DETECT_INTERVAL_SECONDS,
            save_interval: DEFAULT_SAVE_INTERVAL_SECONDS,
            nics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
struct TrafficData {
    #[serde(default)]
    timestamp: u64,
    #[serde(default)]
    tx: u64,
    #[serde(default)]
    rx: u64,
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
