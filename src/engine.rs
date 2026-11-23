use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use tokio::runtime::Builder;
use url::Url;

#[derive(Clone, Debug)]
pub struct TestConfig {
    pub url: String,
    pub method: String,
    pub workers: usize,
    pub duration_secs: u64,
    pub max_requests_per_second: u32,
    pub timeout_secs: u64,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub elapsed_ms: u64,
    pub latency_ms: f64,
    pub status: u16,
    pub bytes: usize,
    pub ok: bool,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    Started,
    Sample(Sample),
    Finished { stopped: bool },
    Fatal(String),
}

pub struct TestHandle {
    pub stop: Arc<AtomicBool>,
    pub rx: Receiver<EngineEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn join(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

const MAX_WORKERS: usize = 64;
const MAX_RPS: u32 = 200;
const MAX_DURATION_SECS: u64 = 300;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub fn start(config: TestConfig) -> Result<TestHandle, String> {
    let parsed = Url::parse(&config.url).map_err(|error| format!("Invalid URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("Only absolute HTTP and HTTPS URLs are supported.".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Credentials in target URLs are not supported. Use headers instead.".into());
    }
    if !(1..=MAX_WORKERS).contains(&config.workers) {
        return Err(format!("Workers must be between 1 and {MAX_WORKERS}."));
    }
    if !(1..=MAX_RPS).contains(&config.max_requests_per_second) {
        return Err(format!("Request rate must be between 1 and {MAX_RPS} requests/second."));
    }
    if !(1..=MAX_DURATION_SECS).contains(&config.duration_secs) {
        return Err(format!("Duration must be between 1 and {MAX_DURATION_SECS} seconds."));
    }
    if !(1..=60).contains(&config.timeout_secs) {
        return Err("Timeout must be between 1 and 60 seconds.".into());
    }
    if !matches!(config.method.as_str(), "GET" | "POST") {
        return Err("Supported methods are GET and POST.".into());
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let (tx, rx) = mpsc::channel();
    let join = thread::Builder::new()
        .name("redpulse-engine".into())
        .spawn(move || run_engine(config, thread_stop, tx))
        .map_err(|error| format!("Could not start engine: {error}"))?;

    Ok(TestHandle { stop, rx, join: Some(join) })
}

fn run_engine(config: TestConfig, stop: Arc<AtomicBool>, tx: Sender<EngineEvent>) {
    let runtime = match Builder::new_multi_thread()
        .worker_threads(config.workers.min(8).max(2))
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = tx.send(EngineEvent::Fatal(format!("Tokio runtime error: {error}")));
            return;
        }
    };

    runtime.block_on(async move {
        let client = match Client::builder()
            .connect_timeout(Duration::from_secs(config.timeout_secs.min(15)))
            .timeout(Duration::from_secs(config.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(config.workers)
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                let _ = tx.send(EngineEvent::Fatal(format!("HTTP client error: {error}")));
                return;
            }
        };

        let method = if config.method == "POST" { Method::POST } else { Method::GET };
        let started = Instant::now();
        let deadline = started + Duration::from_secs(config.duration_secs);
        let _ = tx.send(EngineEvent::Started);
        let interval = Duration::from_secs_f64(1.0 / config.max_requests_per_second as f64);
        let next_slot = Arc::new(tokio::sync::Mutex::new(Instant::now()));
        let mut tasks = tokio::task::JoinSet::new();

        for _ in 0..config.workers {
            let client = client.clone();
            let tx = tx.clone();
            let stop = Arc::clone(&stop);
            let next_slot = Arc::clone(&next_slot);
            let url = config.url.clone();
            let body = config.body.clone();
            let method = method.clone();

            tasks.spawn(async move {
                loop {
                    if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                        break;
                    }

                    // Reserve one global request slot, shared by all workers.
                    let scheduled = {
                        let mut slot = next_slot.lock().await;
                        let at = (*slot).max(Instant::now());
                        *slot = at + interval;
                        at
                    };

                    if scheduled >= deadline || stop.load(Ordering::Relaxed) {
                        break;
                    }
                    tokio::select! {
                        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(scheduled)) => {},
                        _ = wait_for_stop(Arc::clone(&stop)) => break,
                    }
                    if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                        break;
                    }

                    let request_started = Instant::now();
                    let mut request = client.request(method.clone(), &url);
                    if method == Method::POST {
                        request = request
                            .header(reqwest::header::CONTENT_TYPE, "application/json")
                            .body(body.clone());
                    }

                    let result = request.send().await;
                    let (status, bytes, ok, error) = match result {
                        Ok(mut response) => {
                            let status = response.status().as_u16();
                            let mut count = 0usize;
                            let mut read_error = None;
                            loop {
                                match response.chunk().await {
                                    Ok(Some(chunk)) => {
                                        count = count.saturating_add(chunk.len());
                                        if count > MAX_RESPONSE_BYTES {
                                            read_error = Some("Response body exceeded the 2 MiB measurement limit".to_string());
                                            break;
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(error) => {
                                        read_error = Some(format!("Body read error: {error}"));
                                        break;
                                    }
                                }
                            }
                            (status, count.min(MAX_RESPONSE_BYTES), status < 400, read_error)
                        }
                        Err(error) => (0, 0, false, Some(error.to_string())),
                    };

                    let sample = Sample {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        latency_ms: request_started.elapsed().as_secs_f64() * 1000.0,
                        status,
                        bytes,
                        ok,
                        error,
                        timestamp: Utc::now(),
                    };
                    if tx.send(EngineEvent::Sample(sample)).is_err() {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                let _ = tx.send(EngineEvent::Fatal(format!("Worker task failed: {error}")));
            }
        }
        let _ = tx.send(EngineEvent::Finished {
            stopped: stop.load(Ordering::Relaxed),
        });
    });
}

async fn wait_for_stop(stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(url: &str) -> TestConfig {
        TestConfig {
            url: url.into(),
            method: "GET".into(),
            workers: 2,
            duration_secs: 10,
            max_requests_per_second: 5,
            timeout_secs: 5,
            body: String::new(),
        }
    }

    #[test]
    fn accepts_http_url() {
        assert!(start(config("https://example.com")).is_ok());
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(start(config("file:///etc/passwd")).is_err());
    }

    #[test]
    fn rejects_embedded_credentials() {
        assert!(start(config("https://user:pass@example.com")).is_err());
    }

    #[test]
    fn rejects_excessive_worker_count() {
        let mut cfg = config("https://example.com");
        cfg.workers = MAX_WORKERS + 1;
        assert!(start(cfg).is_err());
    }

    #[test]
    fn rejects_excessive_request_rate() {
        let mut cfg = config("https://example.com");
        cfg.max_requests_per_second = MAX_RPS + 1;
        assert!(start(cfg).is_err());
    }
}
