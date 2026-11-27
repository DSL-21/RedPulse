use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    engine::{DnsStrategy, EngineStats, HardwareSample, Sample},
    recon::ReconReport,
};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct RunSummary {
    pub completed_http_responses: u64,
    pub observed_rps: f64,
    pub min_latency_ms: Option<f64>,
    pub average_latency_ms: Option<f64>,
    pub p50_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub p99_latency_ms: Option<f64>,
    pub max_latency_ms: Option<f64>,
}

#[derive(Serialize)]
pub struct RunConfig<'a> {
    pub profile: &'a str,
    pub target: &'a str,
    pub method: &'a str,
    pub workers: usize,
    pub duration_secs: u64,
    pub request_rate: Option<u64>,
    pub load_model: crate::engine::LoadModel,
    pub timeout_secs: u64,
    pub actual_elapsed_secs: f64,
    pub max_total_requests: u64,
    pub dns_strategy: DnsStrategy,
    pub backend_id_header: &'a str,
    pub resolved_addresses: &'a [String],
    pub stats: EngineStats,
    pub summary: RunSummary,
    pub hardware_samples: &'a VecDeque<HardwareSample>,
}

#[derive(Serialize)]
struct TrafficRunPayload<'a> {
    application: &'static str,
    version: &'static str,
    generated_at: String,
    #[serde(flatten)]
    config: &'a RunConfig<'a>,
    samples: &'a VecDeque<Sample>,
}

#[derive(Clone, Debug)]
pub struct SessionEntry {
    pub id: u64,
    pub kind: String,
    pub profile: String,
    pub target: String,
    pub completed_at: String,
    payload_json: String,
}

impl SessionEntry {
    fn memory_bytes(&self) -> usize {
        self.payload_json.len()
            + self.kind.len()
            + self.profile.len()
            + self.target.len()
            + self.completed_at.len()
            + std::mem::size_of::<Self>()
    }

    pub fn payload_bytes(&self) -> usize {
        self.payload_json.len()
    }
}

pub const MAX_SESSION_RUNS: usize = 50;
pub const MAX_SESSION_MEMORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub struct SessionHistory {
    entries: VecDeque<SessionEntry>,
    memory_bytes: usize,
    evicted_runs: u64,
}

impl SessionHistory {
    pub fn entries(&self) -> &VecDeque<SessionEntry> {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub fn evicted_runs(&self) -> u64 {
        self.evicted_runs
    }

    pub fn push(&mut self, entry: SessionEntry) -> bool {
        self.push_with_limits(entry, MAX_SESSION_RUNS, MAX_SESSION_MEMORY_BYTES)
    }

    fn push_with_limits(
        &mut self,
        entry: SessionEntry,
        max_runs: usize,
        max_memory_bytes: usize,
    ) -> bool {
        let entry_bytes = entry.memory_bytes();
        if entry_bytes > max_memory_bytes {
            self.evicted_runs = self.evicted_runs.saturating_add(1);
            return false;
        }

        while self.entries.len() >= max_runs
            || self.memory_bytes.saturating_add(entry_bytes) > max_memory_bytes
        {
            let Some(oldest) = self.entries.pop_front() else {
                break;
            };
            self.memory_bytes = self.memory_bytes.saturating_sub(oldest.memory_bytes());
            self.evicted_runs = self.evicted_runs.saturating_add(1);
        }

        self.memory_bytes = self.memory_bytes.saturating_add(entry_bytes);
        self.entries.push_back(entry);
        true
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.memory_bytes = 0;
        self.evicted_runs = 0;
    }
}

#[derive(Serialize)]
struct SessionHeader {
    application: &'static str,
    version: &'static str,
    generated_at: String,
    session_started_at: String,
    storage: &'static str,
    automatic_persistence: bool,
    run_count: usize,
    evicted_older_runs: u64,
    memory_limit_bytes: usize,
    run_limit: usize,
}

#[derive(Serialize)]
struct SessionRunMetadata<'a> {
    id: u64,
    kind: &'a str,
    profile: &'a str,
    target: &'a str,
    completed_at: &'a str,
}

pub fn make_traffic_session_entry(
    id: u64,
    config: &RunConfig<'_>,
    samples: &VecDeque<Sample>,
) -> Result<SessionEntry> {
    let completed_at = chrono::Utc::now().to_rfc3339();
    let payload = TrafficRunPayload {
        application: "RedPulse",
        version: env!("CARGO_PKG_VERSION"),
        generated_at: completed_at.clone(),
        config,
        samples,
    };
    let payload_json =
        serde_json::to_string(&payload).context("Could not serialize completed traffic run")?;

    Ok(SessionEntry {
        id,
        kind: "traffic_test".into(),
        profile: config.profile.to_owned(),
        target: config.target.to_owned(),
        completed_at,
        payload_json,
    })
}

pub fn make_protection_session_entry(id: u64, report: &ReconReport) -> Result<SessionEntry> {
    let payload_json =
        serde_json::to_string(report).context("Could not serialize protection audit")?;
    Ok(SessionEntry {
        id,
        kind: "protection_audit".into(),
        profile: "Protection".into(),
        target: report.target.clone(),
        completed_at: report.generated_at.to_rfc3339(),
        payload_json,
    })
}

pub fn make_protection_error_entry(id: u64, target: &str, error: &str) -> Result<SessionEntry> {
    #[derive(Serialize)]
    struct FailedProtectionAudit<'a> {
        status: &'static str,
        target: &'a str,
        error: &'a str,
    }

    let payload_json = serde_json::to_string(&FailedProtectionAudit {
        status: "failed",
        target,
        error,
    })
    .context("Could not serialize failed protection audit")?;
    Ok(SessionEntry {
        id,
        kind: "protection_audit".into(),
        profile: "Protection".into(),
        target: target.to_owned(),
        completed_at: chrono::Utc::now().to_rfc3339(),
        payload_json,
    })
}

pub fn export_session_json(
    path: &Path,
    history: &SessionHistory,
    session_started_at: &str,
) -> Result<()> {
    if history.is_empty() {
        anyhow::bail!("There are no completed tests in the current session.");
    }

    let header = SessionHeader {
        application: "RedPulse",
        version: env!("CARGO_PKG_VERSION"),
        generated_at: chrono::Utc::now().to_rfc3339(),
        session_started_at: session_started_at.to_owned(),
        storage: "volatile RAM session history",
        automatic_persistence: false,
        run_count: history.len(),
        evicted_older_runs: history.evicted_runs(),
        memory_limit_bytes: MAX_SESSION_MEMORY_BYTES,
        run_limit: MAX_SESSION_RUNS,
    };

    let mut file = BufWriter::new(File::create(path).context("Could not create session report")?);
    let mut header_json =
        serde_json::to_vec(&header).context("Could not serialize session metadata")?;
    header_json.pop(); // Replace the closing brace with the runs array.
    file.write_all(&header_json)?;
    file.write_all(b",\"runs\":[")?;

    for (index, entry) in history.entries().iter().enumerate() {
        if index > 0 {
            file.write_all(b",")?;
        }
        let metadata = SessionRunMetadata {
            id: entry.id,
            kind: &entry.kind,
            profile: &entry.profile,
            target: &entry.target,
            completed_at: &entry.completed_at,
        };
        let mut metadata_json =
            serde_json::to_vec(&metadata).context("Could not serialize session run metadata")?;
        metadata_json.pop();
        file.write_all(&metadata_json)?;
        file.write_all(b",\"data\":")?;
        file.write_all(entry.payload_json.as_bytes())?;
        file.write_all(b"}")?;
    }

    file.write_all(b"]}")?;
    file.flush().context("Could not flush session report")?;
    Ok(())
}

fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn status_class(status: u16) -> &'static str {
    match status / 100 {
        0 => "network_error",
        2 => "http_2xx",
        3 => "http_3xx",
        4 => "http_4xx",
        5 => "http_5xx",
        _ => "other",
    }
}

pub fn export_csv(path: &Path, samples: &[Sample]) -> Result<()> {
    let mut file = File::create(path).context("Could not create CSV file")?;
    writeln!(
        file,
        "elapsed_us,elapsed_ms,latency_ms,status,status_class,http_version,endpoint_ip,backend_id,bytes,ok,error,timestamp"
    )?;
    for sample in samples {
        let http_version = csv_field(sample.http_version.as_deref().unwrap_or(""));
        let error = csv_field(sample.error.as_deref().unwrap_or(""));
        let endpoint_ip = csv_field(sample.endpoint_ip.as_deref().unwrap_or(""));
        let backend_id = csv_field(sample.backend_id.as_deref().unwrap_or(""));
        writeln!(
            file,
            "{},{},{:.3},{},{},{},{},{},{},{},{},{}",
            sample.elapsed_us,
            sample.elapsed_ms,
            sample.latency_ms,
            sample.status,
            status_class(sample.status),
            http_version,
            endpoint_ip,
            backend_id,
            sample.bytes,
            sample.ok,
            error,
            sample.timestamp.to_rfc3339()
        )?;
    }
    file.flush()?;
    Ok(())
}


#[cfg(test)]
mod session_history_tests {
    use super::{MAX_SESSION_MEMORY_BYTES, MAX_SESSION_RUNS, SessionEntry, SessionHistory, export_session_json};

    fn entry(id: u64, payload: String) -> SessionEntry {
        SessionEntry {
            id,
            kind: "traffic_test".into(),
            profile: "Performance".into(),
            target: "https://example.test/".into(),
            completed_at: "2026-11-27T00:00:00Z".into(),
            payload_json: payload,
        }
    }

    #[test]
    fn session_history_keeps_only_the_newest_entries_when_run_limit_is_reached() {
        let mut history = SessionHistory::default();
        for id in 1..=(MAX_SESSION_RUNS as u64 + 1) {
            assert!(history.push(entry(id, r#"{"ok":true}"#.into())));
        }

        assert_eq!(history.len(), MAX_SESSION_RUNS);
        assert_eq!(history.evicted_runs(), 1);
        assert_eq!(history.entries().front().map(|run| run.id), Some(2));
        assert_eq!(history.entries().back().map(|run| run.id), Some(MAX_SESSION_RUNS as u64 + 1));
    }

    #[test]
    fn session_history_rejects_an_entry_larger_than_its_memory_budget() {
        let mut history = SessionHistory::default();
        let oversized = "x".repeat(128);
        assert!(!history.push_with_limits(entry(1, oversized), 5, 64));
        assert!(history.is_empty());
        assert_eq!(history.evicted_runs(), 1);
    }

    #[test]
    fn session_export_contains_every_profile_as_structured_json() {
        let mut history = SessionHistory::default();
        history.push(entry(1, r#"{"samples":[{"status":200}]}"#.into()));
        let mut protection = entry(2, r#"{"findings":[{"title":"HSTS"}]}"#.into());
        protection.kind = "protection_audit".into();
        protection.profile = "Protection".into();
        history.push(protection);

        let path = std::env::temp_dir().join(format!(
            "redpulse-session-test-{}.json",
            std::process::id()
        ));
        export_session_json(&path, &history, "2026-11-27T00:00:00Z").expect("write session report");
        let text = std::fs::read_to_string(&path).expect("read session report");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid session JSON");
        assert_eq!(parsed["run_count"], 2);
        assert_eq!(parsed["automatic_persistence"], false);
        assert_eq!(parsed["runs"][0]["profile"], "Performance");
        assert_eq!(parsed["runs"][0]["data"]["samples"][0]["status"], 200);
        assert_eq!(parsed["runs"][1]["profile"], "Protection");
        assert_eq!(parsed["runs"][1]["data"]["findings"][0]["title"], "HSTS");
        let _ = std::fs::remove_file(path);
    }
}
