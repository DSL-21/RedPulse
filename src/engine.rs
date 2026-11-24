use std::{
    error::Error as StdError,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStats {
    pub scheduled: u64,
    pub started: u64,
    pub completed: u64,
    pub queued: u64,
    pub in_flight: u64,
    pub cancelled: u64,
    pub dropped: u64,
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
    Started {
        at: Instant,
    },
    Stats(EngineStats),
    Sample(Sample),
    Finished {
        stopped: bool,
        elapsed_secs: f64,
        stats: EngineStats,
    },
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

impl Drop for TestHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

const MAX_WORKERS: usize = 64;
const MAX_RPS: u32 = 200;
const MAX_DURATION_SECS: u64 = 300;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_QUEUE_CAPACITY: usize = 128;

#[derive(Default)]
struct Counters {
    scheduled: AtomicU64,
    started: AtomicU64,
    completed: AtomicU64,
    dropped: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> EngineStats {
        let scheduled = self.scheduled.load(Ordering::Relaxed);
        let started = self.started.load(Ordering::Relaxed);
        let completed = self.completed.load(Ordering::Relaxed);
        let dropped = self.dropped.load(Ordering::Relaxed);
        let accepted = scheduled.saturating_sub(dropped);

        EngineStats {
            scheduled,
            started,
            completed,
            queued: accepted.saturating_sub(started),
            in_flight: started.saturating_sub(completed),
            cancelled: 0,
            dropped,
        }
    }

    fn final_snapshot(&self) -> EngineStats {
        let mut stats = self.snapshot();
        let accepted = stats.scheduled.saturating_sub(stats.dropped);
        stats.cancelled = accepted.saturating_sub(stats.completed);
        stats.queued = 0;
        stats.in_flight = 0;
        stats
    }
}

fn emit_stats(tx: &Sender<EngineEvent>, counters: &Counters) {
    let _ = tx.send(EngineEvent::Stats(counters.snapshot()));
}

fn validate(config: &TestConfig) -> Result<(), String> {
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
        return Err(format!(
            "Request rate must be between 1 and {MAX_RPS} requests/second."
        ));
    }
    if !(1..=MAX_DURATION_SECS).contains(&config.duration_secs) {
        return Err(format!(
            "Duration must be between 1 and {MAX_DURATION_SECS} seconds."
        ));
    }
    if !(1..=60).contains(&config.timeout_secs) {
        return Err("Timeout must be between 1 and 60 seconds.".into());
    }
    if !matches!(config.method.as_str(), "GET" | "POST") {
        return Err("Supported methods are GET and POST.".into());
    }
    Ok(())
}

pub fn start(config: TestConfig) -> Result<TestHandle, String> {
    validate(&config)?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let (tx, rx) = mpsc::channel();
    let join = thread::Builder::new()
        .name("redpulse-engine".into())
        .spawn(move || run_engine(config, thread_stop, tx))
        .map_err(|error| format!("Could not start engine: {error}"))?;

    Ok(TestHandle {
        stop,
        rx,
        join: Some(join),
    })
}

fn describe_reqwest_error(error: &reqwest::Error) -> String {
    let category = if error.is_timeout() {
        "Request timed out"
    } else if error.is_connect() {
        "Connection error"
    } else if error.is_body() {
        "Response body error"
    } else if error.is_decode() {
        "Response decoding error"
    } else {
        "HTTP request error"
    };

    let mut details = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !detail.is_empty() && !details.contains(&detail) {
            details.push(detail);
        }
        source = cause.source();
    }

    if details.is_empty() {
        format!("{category}: {error}")
    } else {
        format!("{category}: {error} — {}", details.join(" → "))
    }
}

async fn execute_request(
    client: &Client,
    method: Method,
    url: &str,
    body: &str,
) -> (u16, usize, bool, Option<String>) {
    let mut request = client.request(method.clone(), url);
    if method == Method::POST {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string());
    }

    match request.send().await {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let mut count = 0usize;
            let mut read_error = None;

            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        count = count.saturating_add(chunk.len());
                        if count > MAX_RESPONSE_BYTES {
                            read_error =
                                Some("Response body exceeded the 2 MiB measurement limit".into());
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        read_error = Some(describe_reqwest_error(&error));
                        break;
                    }
                }
            }

            let ok = (200..300).contains(&status) && read_error.is_none();
            (status, count.min(MAX_RESPONSE_BYTES), ok, read_error)
        }
        Err(error) => (0, 0, false, Some(describe_reqwest_error(&error))),
    }
}

fn overdue_slots(now: Instant, next_slot: Instant, interval: Duration) -> u64 {
    if now <= next_slot || interval.is_zero() {
        0
    } else {
        (now.duration_since(next_slot).as_nanos() / interval.as_nanos()) as u64
    }
}

fn run_engine(config: TestConfig, stop: Arc<AtomicBool>, tx: Sender<EngineEvent>) {
    let runtime = match Builder::new_multi_thread()
        .worker_threads(config.workers.clamp(2, 8))
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
                let _ = tx.send(EngineEvent::Fatal(format!(
                    "HTTP client error: {}",
                    describe_reqwest_error(&error)
                )));
                return;
            }
        };

        let method = if config.method == "POST" {
            Method::POST
        } else {
            Method::GET
        };
        let started = Instant::now();
        let deadline = started + Duration::from_secs(config.duration_secs);
        let _ = tx.send(EngineEvent::Started { at: started });

        let interval = Duration::from_secs_f64(1.0 / config.max_requests_per_second as f64);
        let queue_capacity = config
            .workers
            .saturating_mul(2)
            .clamp(2, MAX_QUEUE_CAPACITY);
        let (job_tx, job_rx) = tokio::sync::mpsc::channel::<()>(queue_capacity);
        let job_rx = Arc::new(tokio::sync::Mutex::new(job_rx));
        let counters = Arc::new(Counters::default());
        let mut tasks = tokio::task::JoinSet::new();

        // Workers consume an independently scheduled stream of request jobs.
        // The bounded queue prevents unbounded memory growth when the target
        // responds more slowly than the configured arrival rate.
        for _ in 0..config.workers {
            let client = client.clone();
            let worker_tx = tx.clone();
            let worker_stop = Arc::clone(&stop);
            let worker_rx = Arc::clone(&job_rx);
            let worker_counters = Arc::clone(&counters);
            let url = config.url.clone();
            let body = config.body.clone();
            let method = method.clone();

            tasks.spawn(async move {
                let tokio_deadline = tokio::time::Instant::from_std(deadline);

                loop {
                    let next_job = async {
                        let mut receiver = worker_rx.lock().await;
                        receiver.recv().await
                    };

                    let job = tokio::select! {
                        _ = wait_for_stop(Arc::clone(&worker_stop)) => None,
                        _ = tokio::time::sleep_until(tokio_deadline) => None,
                        job = next_job => job,
                    };

                    if job.is_none()
                        || worker_stop.load(Ordering::Relaxed)
                        || Instant::now() >= deadline
                    {
                        break;
                    }

                    worker_counters.started.fetch_add(1, Ordering::Relaxed);
                    emit_stats(&worker_tx, &worker_counters);

                    let request_started = Instant::now();
                    let request_future = execute_request(&client, method.clone(), &url, &body);

                    let result = tokio::select! {
                        _ = wait_for_stop(Arc::clone(&worker_stop)) => None,
                        _ = tokio::time::sleep_until(tokio_deadline) => None,
                        response = request_future => Some(response),
                    };

                    let Some((status, bytes, ok, error)) = result else {
                        break;
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

                    worker_counters.completed.fetch_add(1, Ordering::Relaxed);
                    if worker_tx.send(EngineEvent::Sample(sample)).is_err() {
                        worker_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    emit_stats(&worker_tx, &worker_counters);
                }
            });
        }

        // The scheduler is independent of worker completion. It attempts one
        // arrival at each monotonic time slot and never creates catch-up bursts.
        let scheduler_stop = Arc::clone(&stop);
        let scheduler_counters = Arc::clone(&counters);
        let scheduler_tx = tx.clone();
        let scheduler = tokio::spawn(async move {
            let mut next_slot = started;
            loop {
                tokio::select! {
                    _ = wait_for_stop(Arc::clone(&scheduler_stop)) => break,
                    _ = tokio::time::sleep_until(
                        tokio::time::Instant::from_std(next_slot)
                    ) => {},
                }

                if scheduler_stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    break;
                }

                // If the scheduler itself wakes late, account for missed
                // arrival slots as dropped instead of sending a burst.
                let overdue = overdue_slots(Instant::now(), next_slot, interval);
                if overdue > 0 {
                    scheduler_counters
                        .scheduled
                        .fetch_add(overdue, Ordering::Relaxed);
                    scheduler_counters
                        .dropped
                        .fetch_add(overdue, Ordering::Relaxed);
                    next_slot += interval * overdue.min(u32::MAX as u64) as u32;
                    emit_stats(&scheduler_tx, &scheduler_counters);
                }

                if next_slot >= deadline {
                    break;
                }

                scheduler_counters.scheduled.fetch_add(1, Ordering::Relaxed);
                // try_send is deliberately non-blocking. A full queue is an
                // explicit dropped arrival, not a delay to later arrivals.
                if job_tx.try_send(()).is_err() {
                    scheduler_counters.dropped.fetch_add(1, Ordering::Relaxed);
                }
                emit_stats(&scheduler_tx, &scheduler_counters);

                next_slot += interval;
            }
        });

        let _ = scheduler.await;
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                let _ = tx.send(EngineEvent::Fatal(format!("Worker task failed: {error}")));
            }
        }

        let stats = counters.final_snapshot();
        let _ = tx.send(EngineEvent::Finished {
            stopped: stop.load(Ordering::Relaxed),
            elapsed_secs: started.elapsed().as_secs_f64(),
            stats,
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
    use std::io::{Read, Write};

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
        assert!(validate(&config("https://example.com")).is_ok());
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(validate(&config("file:///etc/passwd")).is_err());
    }

    #[test]
    fn rejects_embedded_credentials() {
        assert!(validate(&config("https://user:pass@example.com")).is_err());
    }

    #[test]
    fn rejects_excessive_worker_count() {
        let mut cfg = config("https://example.com");
        cfg.workers = MAX_WORKERS + 1;
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn rejects_excessive_request_rate() {
        let mut cfg = config("https://example.com");
        cfg.max_requests_per_second = MAX_RPS + 1;
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn scheduler_counts_overdue_slots_without_catch_up_bursts() {
        let start = Instant::now();
        let interval = Duration::from_millis(200);
        assert_eq!(overdue_slots(start, start, interval), 0);
        assert_eq!(
            overdue_slots(start + Duration::from_millis(399), start, interval),
            1
        );
        assert_eq!(
            overdue_slots(start + Duration::from_millis(600), start, interval),
            3
        );
    }

    #[test]
    fn final_stats_account_for_queued_and_in_flight_cancellations() {
        let counters = Counters::default();
        counters.scheduled.store(5, Ordering::Relaxed);
        counters.started.store(3, Ordering::Relaxed);
        counters.completed.store(2, Ordering::Relaxed);
        counters.dropped.store(1, Ordering::Relaxed);

        let live = counters.snapshot();
        assert_eq!(live.queued, 1);
        assert_eq!(live.in_flight, 1);
        assert_eq!(live.cancelled, 0);

        let finished = counters.final_snapshot();
        assert_eq!(finished.scheduled, 5);
        assert_eq!(finished.started, 3);
        assert_eq!(finished.completed, 2);
        assert_eq!(finished.queued, 0);
        assert_eq!(finished.in_flight, 0);
        assert_eq!(finished.cancelled, 2);
        assert_eq!(finished.dropped, 1);
    }

    #[test]
    fn arrival_scheduler_runs_independently_of_slow_workers() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind slow local test server");
        let address = listener.local_addr().expect("local server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            thread::sleep(Duration::from_millis(2_500));
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\\r\\nContent-Length: 2\\r\\nConnection: close\\r\\n\\r\\nok",
            );
        });

        let mut cfg = config(&format!("http://{address}/"));
        cfg.workers = 1;
        cfg.duration_secs = 2;
        cfg.max_requests_per_second = 5;
        cfg.timeout_secs = 5;
        let mut handle = start(cfg).expect("start slow-response benchmark");

        let mut max_queued = 0;
        let mut max_in_flight = 0;
        let stats = loop {
            match handle
                .rx
                .recv_timeout(Duration::from_secs(4))
                .expect("receive engine event")
            {
                EngineEvent::Started { .. } => {}
                EngineEvent::Stats(snapshot) => {
                    max_queued = max_queued.max(snapshot.queued);
                    max_in_flight = max_in_flight.max(snapshot.in_flight);
                }
                EngineEvent::Sample(_) => {}
                EngineEvent::Finished { stats, .. } => break stats,
                EngineEvent::Fatal(message) => panic!("engine failed: {message}"),
            }
        };

        handle.join();
        server.join().expect("slow local server thread");

        assert!(stats.scheduled >= 8, "scheduler should keep producing arrival slots");
        assert_eq!(stats.started, 1, "one worker can have only one active request");
        assert_eq!(stats.completed, 0, "the response is slower than the run duration");
        assert!(stats.dropped > 0, "a full bounded queue must count dropped arrivals");
        assert!(max_queued > 0, "scheduled arrivals should be visible in the queue");
        assert!(max_in_flight > 0, "the active request should be counted in flight");
        assert_eq!(stats.queued, 0);
        assert_eq!(stats.in_flight, 0);
        assert_eq!(
            stats.cancelled,
            stats.scheduled - stats.dropped - stats.completed
        );
    }

    #[test]
    fn sends_http_request_and_stops_cleanly() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let address = listener.local_addr().expect("local server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write response");
            stream.flush().expect("flush response");
        });

        let mut cfg = config(&format!("http://{address}/"));
        cfg.workers = 1;
        cfg.duration_secs = 5;
        cfg.max_requests_per_second = 2;
        let mut handle = start(cfg).expect("start local benchmark");

        let mut received_sample = false;
        let (measured_elapsed, stats) = loop {
            match handle
                .rx
                .recv_timeout(Duration::from_secs(3))
                .expect("receive engine event")
            {
                EngineEvent::Started { at } => assert!(at <= Instant::now()),
                EngineEvent::Stats(_) => {}
                EngineEvent::Sample(sample) => {
                    assert_eq!(sample.status, 200);
                    assert_eq!(sample.bytes, 2);
                    assert!(sample.ok);
                    received_sample = true;
                    handle.stop();
                }
                EngineEvent::Finished {
                    elapsed_secs,
                    stats,
                    ..
                } => break (elapsed_secs, stats),
                EngineEvent::Fatal(message) => panic!("engine failed: {message}"),
            }
        };

        handle.join();
        server.join().expect("local test server thread");
        assert!(received_sample, "engine should emit an HTTP sample");
        assert!(measured_elapsed.is_finite() && measured_elapsed > 0.0);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.started, 1);
        assert_eq!(stats.in_flight, 0);
        assert_eq!(stats.queued, 0);
        assert_eq!(
            stats.cancelled,
            stats.scheduled - stats.dropped - stats.completed
        );
    }
}
