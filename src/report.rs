use std::{fs::File, io::Write, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::engine::{DnsStrategy, EngineStats, Sample};

#[derive(Serialize)]
pub struct RunConfig<'a> {
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
    pub resolved_addresses: &'a [String],
    pub stats: EngineStats,
}

#[derive(Serialize)]
struct Report<'a> {
    application: &'static str,
    version: &'static str,
    generated_at: String,
    #[serde(flatten)]
    config: &'a RunConfig<'a>,
    samples: &'a [Sample],
}

pub fn export_json(path: &Path, config: &RunConfig<'_>, samples: &[Sample]) -> Result<()> {
    let report = Report {
        application: "RedPulse",
        version: env!("CARGO_PKG_VERSION"),
        generated_at: chrono::Utc::now().to_rfc3339(),
        config,
        samples,
    };
    let mut file = File::create(path).context("Could not create report file")?;
    serde_json::to_writer_pretty(&mut file, &report).context("Could not serialize report")?;
    file.flush().context("Could not flush report")?;
    Ok(())
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
        "elapsed_us,elapsed_ms,latency_ms,status,status_class,endpoint_ip,bytes,ok,error,timestamp"
    )?;
    for sample in samples {
        let error = sample.error.as_deref().unwrap_or("").replace('"', "\"\"");
        let endpoint_ip = sample.endpoint_ip.as_deref().unwrap_or("");
        writeln!(
            file,
            "{},{},{:.3},{},{},\"{}\",{},{},\"{}\",{}",
            sample.elapsed_us,
            sample.elapsed_ms,
            sample.latency_ms,
            sample.status,
            status_class(sample.status),
            endpoint_ip,
            sample.bytes,
            sample.ok,
            error,
            sample.timestamp.to_rfc3339()
        )?;
    }
    file.flush()?;
    Ok(())
}
