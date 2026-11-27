use std::{
    collections::BTreeSet,
    error::Error as StdError,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadModel {
    #[default]
    ConstantArrivalRate,
    ClosedLoop,
}

impl LoadModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::ConstantArrivalRate => "Constant Arrival Rate",
            Self::ClosedLoop => "Closed Loop",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ConstantArrivalRate => {
                "Schedules arrivals at a fixed rate; full queues are counted as dropped."
            }
            Self::ClosedLoop => {
                "Each worker starts its next request after the previous request completes."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsStrategy {
    #[default]
    System,
    RoundRobin,
}

impl DnsStrategy {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System DNS",
            Self::RoundRobin => "Round-robin DNS IPs",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::System => "Uses the normal system resolver and the standard HTTP client pool.",
            Self::RoundRobin => {
                "Resolves the hostname once and pins successive requests to each resolved IP."
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct TestConfig {
    pub url: String,
    pub method: String,
    pub load_model: LoadModel,
    pub dns_strategy: DnsStrategy,
    pub workers: usize,
    pub duration_secs: u64,
    pub max_requests_per_second: u64,
    pub max_total_requests: u64,
    pub timeout_secs: u64,
    pub body: String,
    pub backend_id_header: String,
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
    pub limit_reached: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub elapsed_us: u64,
    pub elapsed_ms: u64,
    pub latency_ms: f64,
    pub status: u16,
    pub http_version: Option<String>,
    pub endpoint_ip: Option<String>,
    pub backend_id: Option<String>,
    pub bytes: usize,
    pub ok: bool,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HardwareSample {
    pub elapsed_ms: u64,
    pub logical_cores: usize,
    pub system_cpu_percent: f32,
    /// Process CPU can exceed 100% when the process uses multiple logical cores.
    pub process_cpu_percent: f32,
    pub system_memory_used_bytes: u64,
    pub system_memory_total_bytes: u64,
    pub process_memory_bytes: Option<u64>,
    /// Host-wide interface throughput, not traffic attributed only to RedPulse.
    pub network_rx_bytes_per_sec: u64,
    pub network_tx_bytes_per_sec: u64,
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    OriginsResolved {
        addresses: Vec<String>,
    },
    Started {
        at: Instant,
    },
    Stats(EngineStats),
    Samples(Vec<Sample>),
    Hardware(HardwareSample),
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

pub const DEFAULT_MAX_TOTAL_REQUESTS: u64 = 10_000;
pub const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const HARDWARE_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Counters {
    scheduled: AtomicU64,
    started: AtomicU64,
    completed: AtomicU64,
    dropped: AtomicU64,
    limit_reached: AtomicBool,
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
            limit_reached: self.limit_reached.load(Ordering::Relaxed),
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

#[derive(Clone)]
struct OriginRoute {
    client: Client,
    ip: Option<IpAddr>,
}

fn resolve_origin_ips(url: &Url) -> Result<Vec<IpAddr>, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "The target URL does not contain a hostname.".to_string())?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| "The target URL does not have a known port.".to_string())?;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("Could not resolve {host}: {error}"))?;

    let unique: BTreeSet<IpAddr> = addresses.map(|address| address.ip()).collect();
    if unique.is_empty() {
        return Err(format!("DNS returned no IP addresses for {host}."));
    }

    Ok(unique.into_iter().collect())
}

fn build_origin_routes(
    config: &TestConfig,
    url: &Url,
) -> Result<(Vec<OriginRoute>, Vec<String>), String> {
    let host = url
        .host_str()
        .ok_or_else(|| "The target URL does not contain a hostname.".to_string())?;
    let ips = match config.dns_strategy {
        DnsStrategy::System => Vec::new(),
        DnsStrategy::RoundRobin => resolve_origin_ips(url)?,
    };
    let resolved_addresses: Vec<String> = ips.iter().map(ToString::to_string).collect();
    let route_ips: Vec<Option<IpAddr>> = if ips.is_empty() {
        vec![None]
    } else {
        ips.into_iter().map(Some).collect()
    };

    let mut routes = Vec::with_capacity(route_ips.len());
    for ip in route_ips {
        let mut builder = Client::builder()
            .connect_timeout(Duration::from_secs(config.timeout_secs))
            .timeout(Duration::from_secs(config.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(config.workers);

        if let Some(ip) = ip {
            // Pin the TCP destination while keeping the original URL hostname
            // intact for HTTP Host and TLS SNI. Proxies are disabled in this
            // mode because a proxy would resolve the origin on its own side.
            builder = builder.no_proxy().resolve(host, SocketAddr::new(ip, 0));
        }

        let client = builder
            .build()
            .map_err(|error| format!("HTTP client error: {}", describe_reqwest_error(&error)))?;
        routes.push(OriginRoute { client, ip });
    }

    Ok((routes, resolved_addresses))
}

fn next_route_index(cursor: &AtomicU64, route_count: usize) -> usize {
    debug_assert!(route_count > 0);
    (cursor.fetch_add(1, Ordering::Relaxed) % route_count as u64) as usize
}

fn validate(config: &TestConfig) -> Result<(), String> {
    let parsed = Url::parse(&config.url).map_err(|error| format!("Invalid URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("Only absolute HTTP and HTTPS URLs are supported.".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Credentials in target URLs are not supported. Use headers instead.".into());
    }
    if config.workers == 0 {
        return Err("At least one worker is required.".into());
    }
    let queue_capacity = config
        .workers
        .checked_mul(2)
        .ok_or_else(|| "Worker count is too large to size the bounded queue.".to_string())?;
    if queue_capacity > tokio::sync::Semaphore::MAX_PERMITS {
        return Err("Worker count exceeds the platform's bounded-queue capacity.".into());
    }
    if config.load_model == LoadModel::ConstantArrivalRate && config.max_requests_per_second == 0 {
        return Err("Arrival rate must be at least 1 request/second.".into());
    }
    if config.duration_secs == 0 {
        return Err("Duration must be at least 1 second.".into());
    }
    if config.max_total_requests == 0 {
        return Err("Request budget must be at least 1 request.".into());
    }
    if config.timeout_secs == 0 {
        return Err("Timeout must be at least 1 second.".into());
    }
    if !matches!(config.method.as_str(), "GET" | "POST") {
        return Err("Supported methods are GET and POST.".into());
    }
    if !config.backend_id_header.trim().is_empty()
        && reqwest::header::HeaderName::from_bytes(config.backend_id_header.trim().as_bytes())
            .is_err()
    {
        return Err("Backend identity header name is invalid.".into());
    }
    if config.method == "POST" {
        if config.body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(format!(
                "POST JSON body exceeds the {} KiB limit.",
                MAX_REQUEST_BODY_BYTES / 1024
            ));
        }
        serde_json::from_str::<serde_json::Value>(&config.body)
            .map_err(|error| format!("POST body must be valid JSON: {error}"))?;
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

#[derive(Clone)]
struct RequestSpec {
    method: Method,
    url: String,
    body: String,
    backend_id_header: Option<reqwest::header::HeaderName>,
}

fn extract_backend_id(
    headers: &reqwest::header::HeaderMap,
    header_name: Option<&reqwest::header::HeaderName>,
) -> Option<String> {
    let header_name = header_name?;
    headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

async fn execute_request(
    client: &Client,
    spec: &RequestSpec,
) -> (
    u16,
    usize,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let mut request = client.request(spec.method.clone(), &spec.url);
    if spec.method == Method::POST {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(spec.body.clone());
    }

    match request.send().await {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let http_version = Some(
                match response.version() {
                    reqwest::Version::HTTP_09 => "HTTP/0.9",
                    reqwest::Version::HTTP_10 => "HTTP/1.0",
                    reqwest::Version::HTTP_11 => "HTTP/1.1",
                    reqwest::Version::HTTP_2 => "HTTP/2",
                    reqwest::Version::HTTP_3 => "HTTP/3",
                    _ => "Unknown",
                }
                .to_string(),
            );
            let backend_id =
                extract_backend_id(response.headers(), spec.backend_id_header.as_ref());
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
            (
                status,
                count.min(MAX_RESPONSE_BYTES),
                ok,
                read_error,
                backend_id,
                http_version,
            )
        }
        Err(error) => (
            0,
            0,
            false,
            Some(describe_reqwest_error(&error)),
            None,
            None,
        ),
    }
}

fn overdue_slots(now: Instant, next_slot: Instant, interval: Duration) -> u64 {
    if now <= next_slot || interval.is_zero() {
        0
    } else {
        (now.duration_since(next_slot).as_nanos() / interval.as_nanos()) as u64
    }
}

fn reserve_slots(counter: &AtomicU64, requested: u64, maximum: u64) -> u64 {
    if requested == 0 {
        return 0;
    }

    let mut current = counter.load(Ordering::Relaxed);
    loop {
        if current >= maximum {
            return 0;
        }
        let granted = requested.min(maximum - current);
        match counter.compare_exchange_weak(
            current,
            current + granted,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return granted,
            Err(observed) => current = observed,
        }
    }
}

fn reserve_one(counter: &AtomicU64, maximum: u64) -> bool {
    reserve_slots(counter, 1, maximum) == 1
}

fn request_interval(requests_per_second: u64) -> Duration {
    // Tokio's time driver uses nanosecond duration granularity. Values above
    // one billion arrivals/second therefore share the 1 ns minimum interval.
    let nanos = (1_000_000_000u128 / u128::from(requests_per_second)).max(1);
    Duration::from_nanos(nanos.min(u64::MAX as u128) as u64)
}

#[derive(Clone, Copy)]
struct RunTiming {
    started: Instant,
    deadline: Instant,
}

async fn perform_request(
    client: &Client,
    spec: &RequestSpec,
    endpoint_ip: Option<String>,
    timing: RunTiming,
    stop: Arc<AtomicBool>,
) -> Option<Sample> {
    let request_started = Instant::now();
    let tokio_deadline = tokio::time::Instant::from_std(timing.deadline);

    let result = tokio::select! {
        _ = wait_for_stop(Arc::clone(&stop)) => None,
        _ = tokio::time::sleep_until(tokio_deadline) => None,
        response = execute_request(client, spec) => Some(response),
    };

    result.map(|(status, bytes, ok, error, backend_id, http_version)| {
        let elapsed_us = timing.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        Sample {
            elapsed_us,
            elapsed_ms: elapsed_us / 1000,
            latency_ms: request_started.elapsed().as_secs_f64() * 1000.0,
            status,
            http_version,
            endpoint_ip,
            backend_id,
            bytes,
            ok,
            error,
            timestamp: Utc::now(),
        }
    })
}

fn spawn_hardware_monitor(
    started: Instant,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    tx: Sender<EngineEvent>,
) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("redpulse-hardware-monitor".into())
        .spawn(move || {
            use sysinfo::{
                CpuRefreshKind, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
                RefreshKind, System,
            };

            let Ok(pid) = sysinfo::get_current_pid() else {
                return;
            };
            let Ok(mut system) = System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            ) else {
                return;
            };
            let Ok(mut networks) = Networks::new_with_refreshed_list() else {
                return;
            };

            // Prime the counters; CPU usage is calculated from the interval
            // between refreshes and is meaningful after the next sample.
            system.refresh_cpu_usage();
            system.refresh_memory();
            let process_ids = [pid];
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&process_ids),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );

            let logical_cores = system.cpus().len().max(1);
            let mut previous_tick = Instant::now();

            while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                thread::sleep(HARDWARE_SAMPLE_INTERVAL);

                if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    break;
                }

                system.refresh_cpu_usage();
                system.refresh_memory();
                system.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&process_ids),
                    true,
                    ProcessRefreshKind::nothing().with_cpu().with_memory(),
                );
                networks.refresh(true);

                let now = Instant::now();
                let interval_secs = now.duration_since(previous_tick).as_secs_f64().max(0.001);
                let received_bytes = networks
                    .values()
                    .map(|network| network.received())
                    .sum::<u64>();
                let transmitted_bytes = networks
                    .values()
                    .map(|network| network.transmitted())
                    .sum::<u64>();
                let process = system.process(pid);

                let sample = HardwareSample {
                    elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                    logical_cores,
                    system_cpu_percent: system.cpus().iter().map(|cpu| cpu.usage()).sum::<f32>()
                        / logical_cores as f32,
                    process_cpu_percent: process.map(|process| process.cpu_usage()).unwrap_or(0.0),
                    system_memory_used_bytes: system.used_memory(),
                    system_memory_total_bytes: system.total_memory(),
                    process_memory_bytes: process.map(|process| process.memory()),
                    network_rx_bytes_per_sec: (received_bytes as f64 / interval_secs)
                        .round()
                        .min(u64::MAX as f64) as u64,
                    network_tx_bytes_per_sec: (transmitted_bytes as f64 / interval_secs)
                        .round()
                        .min(u64::MAX as f64) as u64,
                };

                if tx.send(EngineEvent::Hardware(sample)).is_err() {
                    break;
                }
                previous_tick = now;
            }
        })
        .ok()
}

fn run_engine(config: TestConfig, stop: Arc<AtomicBool>, tx: Sender<EngineEvent>) {
    let runtime = match Builder::new_multi_thread()
        .worker_threads(
            thread::available_parallelism()
                .map(|parallelism| parallelism.get())
                .unwrap_or(1),
        )
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
        let parsed_url = match Url::parse(&config.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = tx.send(EngineEvent::Fatal(format!("Invalid target URL: {error}")));
                return;
            }
        };
        let (origin_routes, resolved_addresses) = match build_origin_routes(&config, &parsed_url) {
            Ok(routes) => routes,
            Err(error) => {
                let _ = tx.send(EngineEvent::Fatal(error));
                return;
            }
        };
        let _ = tx.send(EngineEvent::OriginsResolved {
            addresses: resolved_addresses,
        });
        let origin_routes = Arc::new(origin_routes);
        let route_cursor = Arc::new(AtomicU64::new(0));

        let method = if config.method == "POST" {
            Method::POST
        } else {
            Method::GET
        };
        let request_spec = RequestSpec {
            method,
            url: config.url.clone(),
            body: config.body.clone(),
            backend_id_header: if config.backend_id_header.trim().is_empty() {
                None
            } else {
                reqwest::header::HeaderName::from_bytes(config.backend_id_header.trim().as_bytes()).ok()
            },
        };
        let started = Instant::now();
        let Some(deadline) = started.checked_add(Duration::from_secs(config.duration_secs)) else {
            let _ = tx.send(EngineEvent::Fatal(
                "Configured duration exceeds this platform's monotonic clock range.".into(),
            ));
            return;
        };
        let _ = tx.send(EngineEvent::Started { at: started });
        // Auxiliary monitors must stop when the workload finishes, even if
        // the configured duration has not elapsed (for example, the request
        // budget was reached early). Keep this separate from the user stop flag
        // so Finished.stopped continues to mean an explicit cancellation.
        let monitor_shutdown = Arc::new(AtomicBool::new(false));
        let hardware_monitor = spawn_hardware_monitor(
            started,
            deadline,
            Arc::clone(&monitor_shutdown),
            tx.clone(),
        );

        let counters = Arc::new(Counters::default());
        let sample_queue_capacity = config.workers.saturating_mul(64).clamp(256, 65_536);
        let (sample_tx, mut sample_rx) =
            tokio::sync::mpsc::channel::<Sample>(sample_queue_capacity);
        let sample_publisher = {
            let sample_event_tx = tx.clone();
            tokio::spawn(async move {
                const BATCH_SIZE: usize = 64;
                let mut batch = Vec::with_capacity(BATCH_SIZE);
                let mut ticker = tokio::time::interval(Duration::from_millis(50));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        sample = sample_rx.recv() => {
                            match sample {
                                Some(sample) => {
                                    batch.push(sample);
                                    if batch.len() >= BATCH_SIZE {
                                        let ready = std::mem::replace(
                                            &mut batch,
                                            Vec::with_capacity(BATCH_SIZE),
                                        );
                                        if sample_event_tx.send(EngineEvent::Samples(ready)).is_err() {
                                            break;
                                        }
                                    }
                                }
                                None => break,
                            }
                        }
                        _ = ticker.tick() => {
                            if !batch.is_empty() {
                                let ready = std::mem::replace(
                                    &mut batch,
                                    Vec::with_capacity(BATCH_SIZE),
                                );
                                if sample_event_tx.send(EngineEvent::Samples(ready)).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }

                if !batch.is_empty() {
                    let _ = sample_event_tx.send(EngineEvent::Samples(batch));
                }
            })
        };
        let stats_task = {
            let stats_tx = tx.clone();
            let stats_counters = Arc::clone(&counters);
            let stats_stop = Arc::clone(&monitor_shutdown);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(250));
                loop {
                    ticker.tick().await;
                    emit_stats(&stats_tx, &stats_counters);
                    if stats_stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                        break;
                    }
                }
            })
        };
        let mut tasks = tokio::task::JoinSet::new();
        let mut scheduler = None;

        match config.load_model {
            LoadModel::ConstantArrivalRate => {
                let interval = request_interval(config.max_requests_per_second);
                let queue_capacity = config.workers.saturating_mul(2).max(2);
                let (job_tx, job_rx) = async_channel::bounded::<()>(queue_capacity);

                // Fixed-rate arrivals are scheduled independently of worker
                // completion. The bounded queue makes overload measurable.
                for _ in 0..config.workers {
                    let worker_routes = Arc::clone(&origin_routes);
                    let worker_route_cursor = Arc::clone(&route_cursor);
                    let worker_sample_tx = sample_tx.clone();
                    let worker_stop = Arc::clone(&stop);
                    let worker_rx = job_rx.clone();
                    let worker_counters = Arc::clone(&counters);
                    let worker_request = request_spec.clone();

                    tasks.spawn(async move {
                        loop {
                            let tokio_deadline = tokio::time::Instant::from_std(deadline);
                            let job = tokio::select! {
                                _ = wait_for_stop(Arc::clone(&worker_stop)) => None,
                                _ = tokio::time::sleep_until(tokio_deadline) => None,
                                job = worker_rx.recv() => job.ok(),
                            };

                            if job.is_none()
                                || worker_stop.load(Ordering::Relaxed)
                                || Instant::now() >= deadline
                            {
                                break;
                            }

                            worker_counters.started.fetch_add(1, Ordering::Relaxed);
                            let route_index =
                                next_route_index(&worker_route_cursor, worker_routes.len());
                            let route = worker_routes[route_index].clone();
                            let Some(sample) = perform_request(
                                &route.client,
                                &worker_request,
                                route.ip.map(|ip| ip.to_string()),
                                RunTiming { started, deadline },
                                Arc::clone(&worker_stop),
                            )
                            .await
                            else {
                                break;
                            };

                            worker_counters.completed.fetch_add(1, Ordering::Relaxed);
                            if worker_sample_tx.send(sample).await.is_err() {
                                worker_stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    });
                }

                let scheduler_stop = Arc::clone(&stop);
                let scheduler_counters = Arc::clone(&counters);
                let max_total_requests = config.max_total_requests;
                scheduler = Some(tokio::spawn(async move {
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

                        // Missed slots are accounted as dropped; no catch-up burst.
                        let overdue = overdue_slots(Instant::now(), next_slot, interval);
                        if overdue > 0 {
                            let missed = reserve_slots(
                                &scheduler_counters.scheduled,
                                overdue,
                                max_total_requests,
                            );
                            if missed > 0 {
                                scheduler_counters
                                    .dropped
                                    .fetch_add(missed, Ordering::Relaxed);
                            }
                            next_slot += interval * missed.min(u32::MAX as u64) as u32;
                            if missed < overdue {
                                scheduler_counters
                                    .limit_reached
                                    .store(true, Ordering::Relaxed);
                                break;
                            }
                            if scheduler_counters.scheduled.load(Ordering::Relaxed)
                                >= max_total_requests
                            {
                                scheduler_counters
                                    .limit_reached
                                    .store(true, Ordering::Relaxed);
                                break;
                            }
                        }

                        if next_slot >= deadline {
                            break;
                        }

                        if !reserve_one(&scheduler_counters.scheduled, max_total_requests) {
                            scheduler_counters
                                .limit_reached
                                .store(true, Ordering::Relaxed);
                            break;
                        }
                        if job_tx.try_send(()).is_err() {
                            scheduler_counters.dropped.fetch_add(1, Ordering::Relaxed);
                        }
                        next_slot += interval;

                        if scheduler_counters.scheduled.load(Ordering::Relaxed)
                            >= max_total_requests
                        {
                            scheduler_counters
                                .limit_reached
                                .store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }));
            }
            LoadModel::ClosedLoop => {
                // Closed-loop mode has no arrival timer or global rate cap.
                // Worker concurrency and the user-defined run budget control
                // the workload; response time naturally feeds back into RPS.
                let max_total_requests = config.max_total_requests;
                for _ in 0..config.workers {
                    let worker_routes = Arc::clone(&origin_routes);
                    let worker_route_cursor = Arc::clone(&route_cursor);
                    let worker_sample_tx = sample_tx.clone();
                    let worker_stop = Arc::clone(&stop);
                    let worker_counters = Arc::clone(&counters);
                    let worker_request = request_spec.clone();

                    tasks.spawn(async move {
                        loop {
                            if worker_stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
                                break;
                            }
                            if !reserve_one(&worker_counters.scheduled, max_total_requests) {
                                worker_counters.limit_reached.store(true, Ordering::Relaxed);
                                break;
                            }

                            worker_counters.started.fetch_add(1, Ordering::Relaxed);
                            let route_index =
                                next_route_index(&worker_route_cursor, worker_routes.len());
                            let route = worker_routes[route_index].clone();
                            let Some(sample) = perform_request(
                                &route.client,
                                &worker_request,
                                route.ip.map(|ip| ip.to_string()),
                                RunTiming { started, deadline },
                                Arc::clone(&worker_stop),
                            )
                            .await
                            else {
                                break;
                            };

                            worker_counters.completed.fetch_add(1, Ordering::Relaxed);
                            if worker_sample_tx.send(sample).await.is_err() {
                                worker_stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    });
                }
            }
        }

        if let Some(scheduler) = scheduler {
            let _ = scheduler.await;
        }
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                let _ = tx.send(EngineEvent::Fatal(format!("Worker task failed: {error}")));
            }
        }
        // Capture the measured workload duration before joining telemetry
        // helpers so monitor shutdown cannot inflate observed RPS.
        let measured_elapsed_secs = started.elapsed().as_secs_f64();
        monitor_shutdown.store(true, Ordering::Relaxed);
        drop(sample_tx);
        let _ = sample_publisher.await;
        if let Some(monitor) = hardware_monitor {
            let _ = monitor.join();
        }
        let _ = stats_task.await;

        let stats = counters.final_snapshot();
        let _ = tx.send(EngineEvent::Finished {
            stopped: stop.load(Ordering::Relaxed),
            elapsed_secs: measured_elapsed_secs,
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
            load_model: LoadModel::ConstantArrivalRate,
            dns_strategy: DnsStrategy::System,
            workers: 2,
            duration_secs: 10,
            max_requests_per_second: 5,
            max_total_requests: DEFAULT_MAX_TOTAL_REQUESTS,
            timeout_secs: 5,
            body: String::new(),
            backend_id_header: "x-backend-id".into(),
        }
    }

    #[test]
    fn backend_identity_header_is_case_insensitive_and_trimmed() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-backend-id",
            reqwest::header::HeaderValue::from_static("  node-a  "),
        );
        let name = reqwest::header::HeaderName::from_static("x-backend-id");
        assert_eq!(
            extract_backend_id(&headers, Some(&name)),
            Some("node-a".into())
        );
        assert_eq!(extract_backend_id(&headers, None), None);
    }

    #[test]
    fn rejects_invalid_backend_identity_header_name() {
        let mut cfg = config("https://example.com");
        cfg.backend_id_header = "bad header".into();
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn dns_strategy_defaults_to_system_and_round_robin_is_named() {
        assert_eq!(DnsStrategy::default(), DnsStrategy::System);
        assert_eq!(DnsStrategy::System.label(), "System DNS");
        assert_eq!(DnsStrategy::RoundRobin.label(), "Round-robin DNS IPs");
    }

    #[test]
    fn literal_ip_resolution_returns_the_literal_without_dns() {
        let url = Url::parse("https://192.0.2.10/").expect("valid IP URL");
        assert_eq!(
            resolve_origin_ips(&url).expect("literal IP resolution"),
            vec!["192.0.2.10".parse::<IpAddr>().expect("literal IP")]
        );
    }

    #[test]
    fn round_robin_route_selection_cycles_through_every_ip() {
        let cursor = AtomicU64::new(0);
        let indices: Vec<usize> = (0..7).map(|_| next_route_index(&cursor, 3)).collect();
        assert_eq!(indices, vec![0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn round_robin_builds_a_pinned_client_for_literal_ip() {
        let mut cfg = config("http://127.0.0.1/");
        cfg.dns_strategy = DnsStrategy::RoundRobin;
        let url = Url::parse(&cfg.url).expect("target URL");
        let (routes, addresses) =
            build_origin_routes(&cfg, &url).expect("build pinned origin client");
        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0].ip,
            Some("127.0.0.1".parse().expect("loopback IP"))
        );
        assert_eq!(addresses, vec!["127.0.0.1"]);
        assert_eq!(url.host_str(), Some("127.0.0.1"));
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
    fn accepts_user_configured_high_worker_and_arrival_values() {
        let mut cfg = config("https://example.com");
        cfg.workers = 4_096;
        cfg.max_requests_per_second = 1_000_000;
        cfg.max_total_requests = 2_000_000;
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn rejects_zero_worker_rate_duration_and_request_budget() {
        let mut cfg = config("https://example.com");
        cfg.workers = 0;
        assert!(validate(&cfg).is_err());

        let mut cfg = config("https://example.com");
        cfg.max_requests_per_second = 0;
        assert!(validate(&cfg).is_err());

        let mut cfg = config("https://example.com");
        cfg.duration_secs = 0;
        assert!(validate(&cfg).is_err());

        let mut cfg = config("https://example.com");
        cfg.max_total_requests = 0;
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn request_interval_scales_beyond_the_old_rate_ceiling() {
        assert_eq!(request_interval(200), Duration::from_millis(5));
        assert_eq!(request_interval(1_000_000), Duration::from_micros(1));
        assert_eq!(request_interval(2_000_000_000), Duration::from_nanos(1));
    }

    #[test]
    fn closed_loop_does_not_require_a_user_arrival_rate() {
        let mut cfg = config("https://example.com");
        cfg.load_model = LoadModel::ClosedLoop;
        cfg.max_requests_per_second = 0;
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn rejects_invalid_json_post_body() {
        let mut cfg = config("https://example.com");
        cfg.method = "POST".into();
        cfg.body = r#"{"broken":"json""#.into();
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn rejects_oversized_json_post_body() {
        let mut cfg = config("https://example.com");
        cfg.method = "POST".into();
        cfg.body = format!(r#"{{"data":"{}"}}"#, "x".repeat(MAX_REQUEST_BODY_BYTES));
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn request_budget_obeys_the_user_configured_cap() {
        let counter = AtomicU64::new(0);
        let configured_budget = 3;
        assert_eq!(reserve_slots(&counter, 2, configured_budget), 2);
        assert!(reserve_one(&counter, configured_budget));
        assert!(!reserve_one(&counter, configured_budget));
        assert_eq!(reserve_slots(&counter, 100, configured_budget), 0);
        assert_eq!(counter.load(Ordering::Relaxed), configured_budget);
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
    fn closed_loop_workers_repeat_after_each_response() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind closed-loop test server");
        listener
            .set_nonblocking(true)
            .expect("set nonblocking listener");
        let address = listener.local_addr().expect("local server address");
        let server_stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&server_stop);
        let server = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0u8; 2048];
                        let _ = stream.read(&mut request);
                        thread::sleep(Duration::from_millis(20));
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        let mut cfg = config(&format!("http://{address}/"));
        cfg.load_model = LoadModel::ClosedLoop;
        cfg.workers = 2;
        cfg.duration_secs = 1;
        cfg.max_requests_per_second = 0;
        cfg.timeout_secs = 2;
        let mut handle = start(cfg).expect("start closed-loop benchmark");

        let stats = loop {
            match handle
                .rx
                .recv_timeout(Duration::from_secs(4))
                .expect("receive closed-loop event")
            {
                EngineEvent::OriginsResolved { .. }
                | EngineEvent::Started { .. }
                | EngineEvent::Stats(_)
                | EngineEvent::Samples(_)
                | EngineEvent::Hardware(_) => {}
                EngineEvent::Finished { stats, .. } => break stats,
                EngineEvent::Fatal(message) => panic!("closed-loop engine failed: {message}"),
            }
        };

        handle.join();
        server_stop.store(true, Ordering::Relaxed);
        server.join().expect("closed-loop test server thread");

        assert!(
            stats.completed > 10,
            "workers should repeat completed requests"
        );
        assert_eq!(stats.scheduled, stats.started);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.queued, 0);
        assert_eq!(stats.in_flight, 0);
        assert_eq!(stats.cancelled, stats.started - stats.completed);
    }

    #[test]
    fn arrival_scheduler_runs_independently_of_slow_workers() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind slow local test server");
        let address = listener.local_addr().expect("local server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            thread::sleep(Duration::from_millis(2_500));
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
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
                EngineEvent::OriginsResolved { .. } | EngineEvent::Started { .. } => {}
                EngineEvent::Stats(snapshot) => {
                    max_queued = max_queued.max(snapshot.queued);
                    max_in_flight = max_in_flight.max(snapshot.in_flight);
                }
                EngineEvent::Samples(_) | EngineEvent::Hardware(_) => {}
                EngineEvent::Finished { stats, .. } => break stats,
                EngineEvent::Fatal(message) => panic!("engine failed: {message}"),
            }
        };

        handle.join();
        server.join().expect("slow local server thread");

        assert!(
            stats.scheduled >= 8,
            "scheduler should keep producing arrival slots"
        );
        assert_eq!(
            stats.started, 1,
            "one worker can have only one active request"
        );
        assert_eq!(
            stats.completed, 0,
            "the response is slower than the run duration"
        );
        assert!(
            stats.dropped > 0,
            "a full bounded queue must count dropped arrivals"
        );
        assert!(
            max_queued > 0,
            "scheduled arrivals should be visible in the queue"
        );
        assert!(
            max_in_flight > 0,
            "the active request should be counted in flight"
        );
        assert_eq!(stats.queued, 0);
        assert_eq!(stats.in_flight, 0);
        assert_eq!(
            stats.cancelled,
            stats.scheduled - stats.dropped - stats.completed
        );
    }

    #[test]
    fn closed_loop_reports_request_timeout_as_network_error() {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind timeout test server");
        let address = listener.local_addr().expect("timeout server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept timeout test client");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            thread::sleep(Duration::from_millis(1_400));
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        });

        let mut cfg = config(&format!("http://{address}/"));
        cfg.load_model = LoadModel::ClosedLoop;
        cfg.workers = 1;
        cfg.duration_secs = 4;
        cfg.timeout_secs = 1;
        let mut handle = start(cfg).expect("start timeout benchmark");

        let sample = loop {
            match handle
                .rx
                .recv_timeout(Duration::from_secs(3))
                .expect("receive timeout benchmark event")
            {
                EngineEvent::OriginsResolved { .. }
                | EngineEvent::Started { .. }
                | EngineEvent::Stats(_)
                | EngineEvent::Hardware(_) => {}
                EngineEvent::Samples(mut samples) => {
                    handle.stop();
                    break samples.pop().expect("sample batch should not be empty");
                }
                EngineEvent::Finished { .. } => panic!("run finished before timeout sample"),
                EngineEvent::Fatal(message) => panic!("timeout benchmark failed: {message}"),
            }
        };

        loop {
            match handle
                .rx
                .recv_timeout(Duration::from_secs(3))
                .expect("receive finished timeout benchmark event")
            {
                EngineEvent::Finished { .. } => break,
                EngineEvent::OriginsResolved { .. }
                | EngineEvent::Started { .. }
                | EngineEvent::Stats(_)
                | EngineEvent::Samples(_)
                | EngineEvent::Hardware(_) => {}
                EngineEvent::Fatal(message) => panic!("timeout benchmark failed: {message}"),
            }
        }

        handle.join();
        server.join().expect("timeout test server thread");
        assert_eq!(sample.status, 0);
        assert!(!sample.ok);
        assert!(
            sample
                .error
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains("timed out"),
            "timeout should be classified clearly"
        );
        assert!(sample.elapsed_us >= 900_000);
    }

    #[test]
    fn request_budget_completion_stops_auxiliary_monitors_before_duration_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind budget test server");
        let address = listener.local_addr().expect("budget test server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept budget test request");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write budget test response");
            stream.flush().expect("flush budget test response");
        });

        let mut cfg = config(&format!("http://{address}/"));
        cfg.load_model = LoadModel::ClosedLoop;
        cfg.workers = 1;
        cfg.duration_secs = 30;
        cfg.max_total_requests = 1;
        cfg.timeout_secs = 3;
        let mut handle = start(cfg).expect("start budget completion benchmark");
        let test_started = Instant::now();

        let stats = loop {
            match handle
                .rx
                .recv_timeout(Duration::from_secs(4))
                .expect("request-budget run should finish before the 30-second deadline")
            {
                EngineEvent::OriginsResolved { .. }
                | EngineEvent::Started { .. }
                | EngineEvent::Stats(_)
                | EngineEvent::Samples(_)
                | EngineEvent::Hardware(_) => {}
                EngineEvent::Finished { stats, stopped, .. } => {
                    assert!(!stopped, "request-budget completion is not a user cancellation");
                    break stats;
                }
                EngineEvent::Fatal(message) => panic!("budget completion failed: {message}"),
            }
        };

        handle.join();
        server.join().expect("budget test server thread");
        assert!(test_started.elapsed() < Duration::from_secs(4));
        assert_eq!(stats.scheduled, 1);
        assert_eq!(stats.started, 1);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.cancelled, 0);
        assert_eq!(stats.dropped, 0);
        assert!(stats.limit_reached);
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
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Backend-ID: node-a\r\nConnection: close\r\n\r\nok",
                )
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
                EngineEvent::OriginsResolved { .. } => {}
                EngineEvent::Started { at } => assert!(at <= Instant::now()),
                EngineEvent::Stats(_) | EngineEvent::Hardware(_) => {}
                EngineEvent::Samples(samples) => {
                    for sample in samples {
                        assert_eq!(sample.status, 200);
                        assert_eq!(sample.bytes, 2);
                        assert_eq!(sample.http_version.as_deref(), Some("HTTP/1.1"));
                        assert_eq!(sample.backend_id.as_deref(), Some("node-a"));
                        assert!(sample.ok);
                        received_sample = true;
                    }
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
