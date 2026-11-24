use std::{fs::File, io::Write, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::engine::Sample;

#[derive(Serialize)]
struct Report<'a> {
    application: &'static str,
    version: &'static str,
    generated_at: String,
    target: &'a str,
    method: &'a str,
    workers: usize,
    samples: &'a [Sample],
}

pub fn export_json(
    path: &Path,
    target: &str,
    method: &str,
    workers: usize,
    samples: &[Sample],
) -> Result<()> {
    let report = Report {
        application: "RedPulse",
        version: env!("CARGO_PKG_VERSION"),
        generated_at: chrono::Utc::now().to_rfc3339(),
        target,
        method,
        workers,
        samples,
    };
    let mut file = File::create(path).context("Could not create report file")?;
    serde_json::to_writer_pretty(&mut file, &report).context("Could not serialize report")?;
    file.flush().context("Could not flush report")?;
    Ok(())
}

pub fn export_csv(path: &Path, samples: &[Sample]) -> Result<()> {
    let mut file = File::create(path).context("Could not create CSV file")?;
    writeln!(
        file,
        "elapsed_ms,latency_ms,status,bytes,ok,error,timestamp"
    )?;
    for sample in samples {
        let error = sample.error.as_deref().unwrap_or("").replace('"', "\"\"");
        writeln!(
            file,
            "{},{:.3},{},{},{},\"{}\",{}",
            sample.elapsed_ms,
            sample.latency_ms,
            sample.status,
            sample.bytes,
            sample.ok,
            error,
            sample.timestamp.to_rfc3339()
        )?;
    }
    file.flush()?;
    Ok(())
}
