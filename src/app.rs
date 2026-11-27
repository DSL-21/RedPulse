use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use hdrhistogram::Histogram;

use crate::{
    engine::{
        self, DEFAULT_MAX_TOTAL_REQUESTS, DnsStrategy, EngineEvent, EngineStats, HardwareSample,
        LoadModel, MAX_REQUEST_BODY_BYTES, Sample, TestConfig, TestHandle,
    },
    recon::{ReconReport, ReconState},
    report,
};

#[derive(PartialEq, Eq)]
enum Tab {
    Recon,
    Tester,
    Results,
    About,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TestProfile {
    #[default]
    Performance,
    LoadBalancer,
    Protection,
}

impl TestProfile {
    fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::LoadBalancer => "Load Balancer",
            Self::Protection => "Protection",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Performance => "Measure HTTP throughput, latency and generator headroom.",
            Self::LoadBalancer => {
                "Distribute requests across resolved IPs and compare observed backend responses."
            }
            Self::Protection => {
                "Run a controlled HTTP test and record access-denied, rate-limit and service-protection responses."
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DistributionMetrics {
    requests: u64,
    errors: u64,
    latency_total_ms: f64,
    latency_samples: u64,
}

impl DistributionMetrics {
    fn record(&mut self, status: u16, latency_ms: f64) {
        self.requests = self.requests.saturating_add(1);
        if status == 0 || status >= 400 {
            self.errors = self.errors.saturating_add(1);
        }
        if status != 0 && latency_ms.is_finite() && latency_ms >= 0.0 {
            self.latency_total_ms += latency_ms;
            self.latency_samples = self.latency_samples.saturating_add(1);
        }
    }

    fn average_latency_ms(&self) -> Option<f64> {
        (self.latency_samples > 0).then(|| self.latency_total_ms / self.latency_samples as f64)
    }
}

const MAX_UI_SAMPLES: usize = 10_000;
const MAX_PLOT_POINTS: usize = 2_000;
const MAX_HARDWARE_SAMPLES: usize = 3_600;

fn normalized_process_cpu_percent(process_cpu_percent: f32, logical_cores: usize) -> f32 {
    if !process_cpu_percent.is_finite() || process_cpu_percent <= 0.0 {
        return 0.0;
    }

    (process_cpu_percent / logical_cores.max(1) as f32).clamp(0.0, 100.0)
}

fn parse_numeric_input<T>(value: &str, field: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .trim()
        .parse::<T>()
        .map_err(|error| format!("{field} must be a valid whole number: {error}"))
}

fn parse_arrival_rate_input(
    value: &str,
    load_model: LoadModel,
    current_value: u64,
) -> Result<u64, String> {
    if load_model == LoadModel::ConstantArrivalRate {
        parse_numeric_input::<u64>(value, "Arrival rate")
    } else {
        Ok(current_value)
    }
}

fn downsample_min_max(points: &VecDeque<(f64, f64)>, max_points: usize) -> Vec<[f64; 2]> {
    let mut sorted: Vec<[f64; 2]> = points.iter().map(|(x, y)| [*x, *y]).collect();
    sorted.sort_by(|left, right| left[0].total_cmp(&right[0]));

    if sorted.len() <= max_points {
        return sorted;
    }
    if max_points < 4 || sorted.len() < 3 {
        return sorted.into_iter().take(max_points).collect();
    }

    let interior_len = sorted.len() - 2;
    let bucket_count = ((max_points - 2) / 2).max(1).min(interior_len);
    let mut reduced = Vec::with_capacity(max_points);
    reduced.push(sorted[0]);

    for bucket in 0..bucket_count {
        let start = 1 + bucket * interior_len / bucket_count;
        let end = 1 + (bucket + 1) * interior_len / bucket_count;
        if start >= end {
            continue;
        }

        let mut min_index = start;
        let mut max_index = start;
        for index in (start + 1)..end {
            if sorted[index][1] < sorted[min_index][1] {
                min_index = index;
            }
            if sorted[index][1] > sorted[max_index][1] {
                max_index = index;
            }
        }

        if min_index == max_index {
            reduced.push(sorted[min_index]);
        } else if min_index < max_index {
            reduced.push(sorted[min_index]);
            reduced.push(sorted[max_index]);
        } else {
            reduced.push(sorted[max_index]);
            reduced.push(sorted[min_index]);
        }
    }

    reduced.push(*sorted.last().expect("non-empty point series"));
    reduced
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseClass {
    NetworkError,
    Success,
    Redirect,
    ClientError,
    ServerError,
    Other,
}

fn classify_status(status: u16) -> ResponseClass {
    match status / 100 {
        0 => ResponseClass::NetworkError,
        2 => ResponseClass::Success,
        3 => ResponseClass::Redirect,
        4 => ResponseClass::ClientError,
        5 => ResponseClass::ServerError,
        _ => ResponseClass::Other,
    }
}

#[derive(Clone)]
struct RunSnapshot {
    profile: TestProfile,
    target: String,
    method: String,
    load_model: LoadModel,
    workers: usize,
    duration_secs: u64,
    request_rate: Option<u64>,
    max_total_requests: u64,
    dns_strategy: DnsStrategy,
    backend_id_header: String,
    resolved_addresses: Vec<String>,
    timeout_secs: u64,
    actual_elapsed_secs: f64,
}

pub struct RedPulseApp {
    tab: Tab,
    profile: TestProfile,
    recon: ReconState,
    target: String,
    method: String,
    load_model: LoadModel,
    dns_strategy: DnsStrategy,
    backend_id_header: String,
    workers: usize,
    duration: u64,
    rps: u64,
    max_requests: u64,
    timeout: u64,
    workers_input: String,
    duration_input: String,
    rps_input: String,
    max_requests_input: String,
    timeout_input: String,
    body: String,
    running: Option<TestHandle>,
    samples: VecDeque<Sample>,
    recent_latencies: VecDeque<(f64, f64)>,
    hardware_samples: VecDeque<HardwareSample>,
    resolved_addresses: Vec<String>,
    origin_metrics: BTreeMap<String, DistributionMetrics>,
    backend_metrics: BTreeMap<String, DistributionMetrics>,
    started: Option<Instant>,
    elapsed_seconds: f64,
    status: String,
    error: Option<String>,
    last_export: Option<String>,
    session_history: report::SessionHistory,
    next_session_run_id: u64,
    session_started_at: String,
    total_received: u64,
    total_success: u64,
    total_redirects: u64,
    total_http_4xx: u64,
    total_http_5xx: u64,
    total_unauthorized: u64,
    total_forbidden: u64,
    total_rate_limited: u64,
    total_unavailable: u64,
    total_network_errors: u64,
    engine_stats: EngineStats,
    http_response_count: u64,
    latency_sum_ms: f64,
    latency_min_ms: Option<f64>,
    latency_max_ms: Option<f64>,
    histogram: Histogram<u64>,
    last_run: Option<RunSnapshot>,
}

impl RedPulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(14, 19, 28);
        visuals.window_fill = egui::Color32::from_rgb(19, 26, 37);
        visuals.extreme_bg_color = egui::Color32::from_rgb(10, 14, 21);
        visuals.faint_bg_color = egui::Color32::from_rgb(29, 37, 51);
        visuals.override_text_color = Some(egui::Color32::from_rgb(226, 233, 243));
        visuals.selection.bg_fill = egui::Color32::from_rgb(34, 125, 190);
        visuals.hyperlink_color = egui::Color32::from_rgb(102, 190, 255);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 31, 43);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(37, 47, 63);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(47, 65, 87);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(35, 119, 174);
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(11.0, 10.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
            style.spacing.interact_size = egui::vec2(40.0, 32.0);
        });

        Self {
            tab: Tab::Tester,
            profile: TestProfile::Performance,
            recon: ReconState::new(),
            target: "https://example.com/".into(),
            method: "GET".into(),
            load_model: LoadModel::ConstantArrivalRate,
            dns_strategy: DnsStrategy::System,
            backend_id_header: "x-backend-id".into(),
            workers: 5,
            duration: 30,
            rps: 5,
            max_requests: DEFAULT_MAX_TOTAL_REQUESTS,
            timeout: 10,
            workers_input: "5".into(),
            duration_input: "30".into(),
            rps_input: "5".into(),
            max_requests_input: DEFAULT_MAX_TOTAL_REQUESTS.to_string(),
            timeout_input: "10".into(),
            body: r#"{ "ping": "redpulse" }"#.into(),
            running: None,
            samples: VecDeque::new(),
            recent_latencies: VecDeque::new(),
            hardware_samples: VecDeque::new(),
            resolved_addresses: Vec::new(),
            origin_metrics: BTreeMap::new(),
            backend_metrics: BTreeMap::new(),
            started: None,
            elapsed_seconds: 0.0,
            status: "Ready".into(),
            error: None,
            last_export: None,
            session_history: report::SessionHistory::default(),
            next_session_run_id: 1,
            session_started_at: chrono::Utc::now().to_rfc3339(),
            total_received: 0,
            total_success: 0,
            total_redirects: 0,
            total_http_4xx: 0,
            total_http_5xx: 0,
            total_unauthorized: 0,
            total_forbidden: 0,
            total_rate_limited: 0,
            total_unavailable: 0,
            total_network_errors: 0,
            engine_stats: EngineStats::default(),
            http_response_count: 0,
            latency_sum_ms: 0.0,
            latency_min_ms: None,
            latency_max_ms: None,
            histogram: Histogram::<u64>::new(3).expect("valid histogram configuration"),
            last_run: None,
        }
    }

    fn apply_profile_defaults(&mut self) {
        match self.profile {
            TestProfile::Performance => {
                self.dns_strategy = DnsStrategy::System;
            }
            TestProfile::LoadBalancer => {
                self.dns_strategy = DnsStrategy::RoundRobin;
                if self.backend_id_header.trim().is_empty() {
                    self.backend_id_header = "x-backend-id".into();
                }
            }
            TestProfile::Protection => {
                self.dns_strategy = DnsStrategy::System;
                self.method = "GET".into();
            }
        }
    }

    fn next_session_id(&mut self) -> u64 {
        let id = self.next_session_run_id;
        self.next_session_run_id = self.next_session_run_id.saturating_add(1);
        id
    }

    fn keep_session_entry(&mut self, entry: report::SessionEntry) {
        if !self.session_history.push(entry) {
            self.error = Some(
                "This run exceeded the 64 MiB session-memory budget and was not retained. You can still export the current run separately.".into(),
            );
        }
    }

    fn record_completed_traffic_run(&mut self) {
        if self.last_run.is_none() {
            return;
        }
        let id = self.next_session_id();
        let entry = {
            let run = self.last_run.as_ref().expect("checked above");
            let (average, minimum, p50, p95, p99, maximum) = self.metrics();
            let has_http_responses = self.http_response_count > 0;
            let summary = report::RunSummary {
                completed_http_responses: self.http_response_count,
                observed_rps: self.observed_rps(),
                min_latency_ms: has_http_responses.then_some(minimum),
                average_latency_ms: has_http_responses.then_some(average),
                p50_latency_ms: has_http_responses.then_some(p50),
                p95_latency_ms: has_http_responses.then_some(p95),
                p99_latency_ms: has_http_responses.then_some(p99),
                max_latency_ms: has_http_responses.then_some(maximum),
                protection_statuses: report::ProtectionStatusSummary {
                    http_401_unauthorized: self.total_unauthorized,
                    http_403_forbidden: self.total_forbidden,
                    http_429_rate_limited: self.total_rate_limited,
                    http_503_unavailable: self.total_unavailable,
                },
                destination_ip_distribution: self
                    .origin_metrics
                    .iter()
                    .map(|(identifier, metric)| report::RouteSummary {
                        identifier: identifier.clone(),
                        requests: metric.requests,
                        errors: metric.errors,
                        average_latency_ms: metric.average_latency_ms(),
                    })
                    .collect(),
                backend_distribution: self
                    .backend_metrics
                    .iter()
                    .map(|(identifier, metric)| report::RouteSummary {
                        identifier: identifier.clone(),
                        requests: metric.requests,
                        errors: metric.errors,
                        average_latency_ms: metric.average_latency_ms(),
                    })
                    .collect(),
            };
            let config = report::RunConfig {
                profile: run.profile.label(),
                target: &run.target,
                method: &run.method,
                workers: run.workers,
                duration_secs: run.duration_secs,
                request_rate: run.request_rate,
                load_model: run.load_model,
                timeout_secs: run.timeout_secs,
                actual_elapsed_secs: run.actual_elapsed_secs,
                max_total_requests: run.max_total_requests,
                dns_strategy: run.dns_strategy,
                backend_id_header: &run.backend_id_header,
                resolved_addresses: &run.resolved_addresses,
                stats: self.engine_stats,
                summary,
                hardware_samples: &self.hardware_samples,
            };
            report::make_traffic_session_entry(id, &config, &self.samples, &self.status)
        };
        match entry {
            Ok(entry) => self.keep_session_entry(entry),
            Err(error) => self.error = Some(format!("Could not retain completed test: {error}")),
        }
    }

    fn record_protection_result(&mut self, target: String, result: Result<ReconReport, String>) {
        let id = self.next_session_id();
        let entry = match result {
            Ok(report) => report::make_protection_session_entry(id, &report),
            Err(error) => report::make_protection_error_entry(id, &target, &error),
        };
        match entry {
            Ok(entry) => self.keep_session_entry(entry),
            Err(error) => self.error = Some(format!("Could not retain protection audit: {error}")),
        }
    }

    fn reset_results(&mut self) {
        self.samples.clear();
        self.recent_latencies.clear();
        self.hardware_samples.clear();
        self.resolved_addresses.clear();
        self.origin_metrics.clear();
        self.backend_metrics.clear();
        self.total_received = 0;
        self.total_success = 0;
        self.total_redirects = 0;
        self.total_http_4xx = 0;
        self.total_http_5xx = 0;
        self.total_unauthorized = 0;
        self.total_forbidden = 0;
        self.total_rate_limited = 0;
        self.total_unavailable = 0;
        self.total_network_errors = 0;
        self.engine_stats = EngineStats::default();
        self.http_response_count = 0;
        self.latency_sum_ms = 0.0;
        self.latency_min_ms = None;
        self.latency_max_ms = None;
        self.histogram.reset();
        self.elapsed_seconds = 0.0;
        self.last_run = None;
    }

    fn start_test(&mut self) {
        let parsed = (|| {
            Ok::<_, String>((
                parse_numeric_input::<usize>(&self.workers_input, "Workers")?,
                parse_numeric_input::<u64>(&self.duration_input, "Duration")?,
                parse_arrival_rate_input(&self.rps_input, self.load_model, self.rps)?,
                parse_numeric_input::<u64>(&self.timeout_input, "Timeout")?,
                parse_numeric_input::<u64>(&self.max_requests_input, "Request budget")?,
            ))
        })();

        let (workers, duration, rps, timeout, max_requests) = match parsed {
            Ok(values) => values,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };

        self.workers = workers;
        self.duration = duration;
        self.rps = rps;
        self.timeout = timeout;
        self.max_requests = max_requests;

        self.reset_results();
        self.error = None;
        self.last_export = None;

        let config = TestConfig {
            url: self.target.trim().to_string(),
            method: self.method.clone(),
            load_model: self.load_model,
            dns_strategy: self.dns_strategy,
            workers: self.workers,
            duration_secs: self.duration,
            max_requests_per_second: self.rps,
            max_total_requests: self.max_requests,
            timeout_secs: self.timeout,
            body: self.body.clone(),
            backend_id_header: self.backend_id_header.trim().to_string(),
        };

        let snapshot = RunSnapshot {
            profile: self.profile,
            target: config.url.clone(),
            method: config.method.clone(),
            workers: config.workers,
            duration_secs: config.duration_secs,
            request_rate: (config.load_model == LoadModel::ConstantArrivalRate)
                .then_some(config.max_requests_per_second),
            max_total_requests: config.max_total_requests,
            dns_strategy: config.dns_strategy,
            backend_id_header: config.backend_id_header.clone(),
            resolved_addresses: Vec::new(),
            load_model: config.load_model,
            timeout_secs: config.timeout_secs,
            actual_elapsed_secs: 0.0,
        };

        match engine::start(config) {
            Ok(handle) => {
                self.last_run = Some(snapshot);
                self.running = Some(handle);
                self.started = None;
                self.status = "Starting".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn stop_test(&mut self) {
        if let Some(handle) = &self.running {
            handle.stop();
            self.status = "Stopping…".into();
        }
    }

    fn record_sample(&mut self, sample: Sample) {
        self.total_received += 1;
        if let Some(ip) = sample.endpoint_ip.as_ref() {
            self.origin_metrics
                .entry(ip.clone())
                .or_default()
                .record(sample.status, sample.latency_ms);
        }
        if let Some(backend_id) = sample.backend_id.as_ref() {
            self.backend_metrics
                .entry(backend_id.clone())
                .or_default()
                .record(sample.status, sample.latency_ms);
        }
        match classify_status(sample.status) {
            ResponseClass::Success => self.total_success += 1,
            ResponseClass::Redirect => self.total_redirects += 1,
            ResponseClass::ClientError => self.total_http_4xx += 1,
            ResponseClass::ServerError => self.total_http_5xx += 1,
            ResponseClass::NetworkError => self.total_network_errors += 1,
            ResponseClass::Other => {}
        }
        match sample.status {
            401 => self.total_unauthorized += 1,
            403 => self.total_forbidden += 1,
            429 => self.total_rate_limited += 1,
            503 => self.total_unavailable += 1,
            _ => {}
        }

        // Response-time statistics describe actual HTTP responses only.
        // Transport failures (status 0), including timeouts, are counted
        // separately and do not distort HTTP latency percentiles.
        if sample.status != 0 && sample.latency_ms.is_finite() && sample.latency_ms >= 0.0 {
            self.http_response_count += 1;
            self.latency_sum_ms += sample.latency_ms;
            self.latency_min_ms = Some(
                self.latency_min_ms
                    .map_or(sample.latency_ms, |current| current.min(sample.latency_ms)),
            );
            self.latency_max_ms = Some(
                self.latency_max_ms
                    .map_or(sample.latency_ms, |current| current.max(sample.latency_ms)),
            );

            let micros = (sample.latency_ms * 1000.0).max(1.0) as u64;
            let _ = self.histogram.record(micros);

            if self.recent_latencies.len() == MAX_UI_SAMPLES {
                self.recent_latencies.pop_front();
            }
            self.recent_latencies
                .push_back((sample.elapsed_us as f64 / 1_000_000.0, sample.latency_ms));
        }

        if self.samples.len() == MAX_UI_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    fn poll_engine(&mut self) {
        // Drain the channel before mutating application state. This keeps the
        // immutable borrow of TestHandle separate from mutable UI updates.
        let events: Vec<EngineEvent> = self
            .running
            .as_ref()
            .map(|handle| handle.rx.try_iter().collect())
            .unwrap_or_default();

        let mut finished = false;
        for event in events {
            match event {
                EngineEvent::OriginsResolved { addresses } => {
                    self.resolved_addresses = addresses.clone();
                    if let Some(run) = self.last_run.as_mut() {
                        run.resolved_addresses = addresses;
                    }
                }
                EngineEvent::Started { at } => {
                    self.started = Some(at);
                    self.status = "Running".into();
                }
                EngineEvent::Stats(stats) => self.engine_stats = stats,
                EngineEvent::Samples(samples) => {
                    for sample in samples {
                        self.record_sample(sample);
                    }
                }
                EngineEvent::Hardware(sample) => {
                    if self.hardware_samples.len() == MAX_HARDWARE_SAMPLES {
                        self.hardware_samples.pop_front();
                    }
                    self.hardware_samples.push_back(sample);
                }
                EngineEvent::Finished {
                    stopped,
                    elapsed_secs,
                    stats,
                } => {
                    self.engine_stats = stats;
                    self.elapsed_seconds = elapsed_secs;
                    if let Some(run) = self.last_run.as_mut() {
                        run.actual_elapsed_secs = elapsed_secs;
                    }
                    self.started = None;
                    self.status = if stopped {
                        "Stopped".into()
                    } else {
                        "Completed".into()
                    };
                    finished = true;
                }
                EngineEvent::Fatal(message) => {
                    self.error = Some(message);
                    self.status = "Failed".into();
                    finished = true;
                }
            }
        }

        if finished {
            if let Some(mut handle) = self.running.take() {
                handle.join();
            }
            self.started = None;
            self.record_completed_traffic_run();
        }

        if let Some(started) = self.started {
            self.elapsed_seconds = started.elapsed().as_secs_f64();
        }
    }

    fn metrics(&self) -> (f64, f64, f64, f64, f64, f64) {
        if self.http_response_count == 0 {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }

        (
            self.latency_sum_ms / self.http_response_count as f64,
            self.latency_min_ms.unwrap_or(0.0),
            self.histogram.value_at_quantile(0.50) as f64 / 1000.0,
            self.histogram.value_at_quantile(0.95) as f64 / 1000.0,
            self.histogram.value_at_quantile(0.99) as f64 / 1000.0,
            self.latency_max_ms.unwrap_or(0.0),
        )
    }

    fn calculate_rps(requests: u64, elapsed_seconds: f64) -> f64 {
        if !elapsed_seconds.is_finite() || elapsed_seconds <= 0.0 {
            0.0
        } else {
            requests as f64 / elapsed_seconds
        }
    }

    fn observed_rps(&self) -> f64 {
        Self::calculate_rps(self.total_received, self.elapsed_seconds)
    }

    fn export(&mut self, csv: bool) {
        let path = PathBuf::from(if csv {
            "redpulse-current-run.csv"
        } else {
            "redpulse-session.json"
        });
        let result = if csv {
            let all_samples: Vec<Sample> = self.samples.iter().cloned().collect();
            report::export_csv(&path, &all_samples)
        } else {
            report::export_session_json(&path, &self.session_history, &self.session_started_at)
        };
        match result {
            Ok(()) => self.last_export = Some(format!("Saved {}", path.display())),
            Err(error) => self.error = Some(format!("Export failed: {error}")),
        }
    }

    fn metric_card(ui: &mut egui::Ui, label: &str, value: impl ToString, unit: &str) {
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(25, 34, 48))
            .stroke(egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgb(48, 65, 86),
            ))
            .corner_radius(12)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(68.0);
                ui.label(
                    egui::RichText::new(label)
                        .size(10.5)
                        .strong()
                        .color(egui::Color32::from_rgb(145, 163, 185)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.heading(
                        egui::RichText::new(value.to_string())
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::from_rgb(232, 241, 251)),
                    );
                    ui.label(
                        egui::RichText::new(unit)
                            .small()
                            .color(egui::Color32::from_rgb(145, 163, 185)),
                    );
                });
            });
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("◈")
                    .size(24.0)
                    .strong()
                    .color(egui::Color32::from_rgb(68, 180, 245)),
            );
            ui.heading(
                egui::RichText::new("REDPULSE")
                    .size(21.0)
                    .strong()
                    .color(egui::Color32::from_rgb(230, 239, 250)),
            );
            ui.label(
                egui::RichText::new("HTTP PERFORMANCE ENGINE")
                    .size(10.0)
                    .strong()
                    .color(egui::Color32::from_rgb(130, 151, 176)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (color, status) = if self.running.is_some() && self.recon.is_running() {
                    (egui::Color32::from_rgb(73, 205, 145), "LOAD + AUDIT")
                } else if self.running.is_some() {
                    (egui::Color32::from_rgb(73, 205, 145), "LOAD RUNNING")
                } else if self.recon.is_running() {
                    (egui::Color32::from_rgb(73, 205, 145), "AUDIT RUNNING")
                } else if self.status == "Starting" {
                    (egui::Color32::from_rgb(105, 174, 238), "STARTING")
                } else if self.status == "Completed" {
                    (egui::Color32::from_rgb(76, 181, 132), "COMPLETED")
                } else if self.status == "Failed" {
                    (egui::Color32::from_rgb(235, 103, 110), "FAILED")
                } else if self.status == "Stopped" {
                    (egui::Color32::from_rgb(237, 183, 87), "STOPPED")
                } else {
                    (egui::Color32::from_rgb(148, 164, 184), "READY")
                };
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(31, 42, 57))
                    .corner_radius(egui::CornerRadius::same(20))
                    .inner_margin(egui::Margin::symmetric(11, 5))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("● {status}"))
                                .size(10.5)
                                .strong()
                                .color(color),
                        );
                    });
            });
        });
    }

    fn tester_ui(&mut self, ui: &mut egui::Ui) {
        let muted = egui::Color32::from_rgb(139, 157, 180);
        let accent = egui::Color32::from_rgb(91, 183, 238);
        let card_fill = egui::Color32::from_rgb(22, 30, 43);
        let card_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(43, 59, 79));

        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading(
                    egui::RichText::new("Traffic Lab")
                        .size(25.0)
                        .strong()
                        .color(egui::Color32::from_rgb(238, 244, 252)),
                );
                ui.label(
                    egui::RichText::new(
                        "Configure a controlled HTTP run and inspect the results as they happen.",
                    )
                    .color(muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(29, 43, 59))
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        egui::Color32::from_rgb(54, 78, 102),
                    ))
                    .corner_radius(egui::CornerRadius::same(18))
                    .inner_margin(egui::Margin::symmetric(11, 6))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  ·  {}",
                                self.status.to_uppercase(),
                                env!("CARGO_PKG_VERSION")
                            ))
                            .size(10.5)
                            .strong()
                            .color(accent),
                        );
                    });
            });
        });

        ui.add_space(14.0);
        egui::Frame::group(ui.style())
            .fill(card_fill)
            .stroke(card_stroke)
            .corner_radius(14)
            .inner_margin(egui::Margin::same(15))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("TEST CONFIGURATION")
                            .size(11.0)
                            .strong()
                            .color(egui::Color32::from_rgb(158, 180, 205)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(if self.running.is_some() { "CONFIGURATION LOCKED WHILE RUNNING" } else { "READY TO CONFIGURE" })
                                .size(9.5)
                                .color(muted),
                        );
                    });
                });

                ui.add_space(9.0);
                ui.label(
                    egui::RichText::new("Test profile")
                        .strong()
                        .color(egui::Color32::from_rgb(216, 227, 240)),
                );
                ui.add_enabled_ui(self.running.is_none(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let mut selected_profile = self.profile;
                        let mut changed = false;
                        if ui
                            .selectable_label(selected_profile == TestProfile::Performance, "  Performance  ")
                            .clicked()
                        {
                            selected_profile = TestProfile::Performance;
                            changed = true;
                        }
                        if ui
                            .selectable_label(selected_profile == TestProfile::LoadBalancer, "  Load Balancer  ")
                            .clicked()
                        {
                            selected_profile = TestProfile::LoadBalancer;
                            changed = true;
                        }
                        if ui
                            .selectable_label(selected_profile == TestProfile::Protection, "  Protection  ")
                            .clicked()
                        {
                            selected_profile = TestProfile::Protection;
                            changed = true;
                        }
                        if changed {
                            self.profile = selected_profile;
                            self.apply_profile_defaults();
                        }
                    });
                });
                ui.label(
                    egui::RichText::new(self.profile.description())
                        .small()
                        .color(muted),
                );

                if self.profile == TestProfile::LoadBalancer {
                    ui.add_space(3.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Destination routing: {}   ·   Backend response header: {}",
                            self.dns_strategy.label(),
                            if self.backend_id_header.trim().is_empty() { "not configured" } else { self.backend_id_header.trim() }
                        ))
                        .small()
                        .color(accent),
                    );
                } else if self.profile == TestProfile::Protection {
                    ui.add_space(3.0);
                    ui.label(
                        egui::RichText::new(
                            "Observes access-control responses and passive edge-service signals. It does not bypass protections.",
                        )
                        .small()
                        .color(muted),
                    );
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("TARGET ENDPOINT")
                        .size(10.5)
                        .strong()
                        .color(egui::Color32::from_rgb(158, 180, 205)),
                );
                ui.add_enabled_ui(self.running.is_none(), |ui| {
                    ui.add_sized(
                        [ui.available_width(), 36.0],
                        egui::TextEdit::singleline(&mut self.target)
                            .hint_text("https://your-authorized-target.example")
                            .font(egui::TextStyle::Monospace),
                    );
                });
                ui.label(
                    egui::RichText::new("Run tests only against infrastructure you own or have explicit permission to assess.")
                        .small()
                        .color(muted),
                );

                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("LOAD PARAMETERS")
                        .size(10.5)
                        .strong()
                        .color(egui::Color32::from_rgb(158, 180, 205)),
                );
                ui.add_space(5.0);
                ui.columns(4, |columns| {
                    for (column, label, hint) in [
                        (0, "Workers", "Concurrent requests"),
                        (1, "Duration", "Seconds"),
                        (2, "Arrival rate", "Requests / second"),
                        (3, "Request budget", "Total scheduled requests"),
                    ] {
                        columns[column].vertical(|ui| {
                            ui.label(
                                egui::RichText::new(label)
                                    .strong()
                                    .color(egui::Color32::from_rgb(220, 230, 242)),
                            );
                            let response = match column {
                                0 => ui.add_enabled(
                                    self.running.is_none(),
                                    egui::TextEdit::singleline(&mut self.workers_input)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                ),
                                1 => ui.add_enabled(
                                    self.running.is_none(),
                                    egui::TextEdit::singleline(&mut self.duration_input)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                ),
                                2 => ui.add_enabled(
                                    self.running.is_none() && self.load_model == LoadModel::ConstantArrivalRate,
                                    egui::TextEdit::singleline(&mut self.rps_input)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                ),
                                _ => ui.add_enabled(
                                    self.running.is_none(),
                                    egui::TextEdit::singleline(&mut self.max_requests_input)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                ),
                            };
                            let _ = response;
                            ui.label(egui::RichText::new(hint).small().color(muted));
                        });
                    }
                });

                ui.add_space(9.0);
                egui::CollapsingHeader::new("Advanced request settings")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(self.running.is_none(), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new("Load model").strong());
                                    egui::ComboBox::from_id_salt("load_model")
                                        .selected_text(self.load_model.label())
                                        .width(190.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.load_model, LoadModel::ConstantArrivalRate, "Constant Arrival Rate");
                                            ui.selectable_value(&mut self.load_model, LoadModel::ClosedLoop, "Closed Loop");
                                        });
                                });
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new("HTTP method").strong());
                                    egui::ComboBox::from_id_salt("method")
                                        .selected_text(&self.method)
                                        .width(110.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.method, "GET".into(), "GET");
                                            ui.selectable_value(&mut self.method, "POST".into(), "POST");
                                        });
                                });
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new("Timeout (seconds)").strong());
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.timeout_input)
                                            .desired_width(100.0)
                                            .font(egui::TextStyle::Monospace),
                                    );
                                });
                            });
                            ui.label(
                                egui::RichText::new(self.load_model.description())
                                    .small()
                                    .color(muted),
                            );

                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new("DNS routing").strong());
                                    egui::ComboBox::from_id_salt("dns_strategy")
                                        .selected_text(self.dns_strategy.label())
                                        .width(190.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.dns_strategy, DnsStrategy::System, DnsStrategy::System.label());
                                            ui.selectable_value(&mut self.dns_strategy, DnsStrategy::RoundRobin, DnsStrategy::RoundRobin.label());
                                        });
                                });
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new("Backend ID response header").strong());
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.backend_id_header)
                                            .hint_text("x-backend-id")
                                            .desired_width(220.0),
                                    );
                                });
                            });
                            ui.label(
                                egui::RichText::new(self.dns_strategy.description())
                                    .small()
                                    .color(muted),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "A backend identifier is counted only when your application returns the configured response header.",
                                )
                                .small()
                                .color(muted),
                            );

                            if self.method == "POST" {
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("JSON request body").strong());
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.body)
                                        .desired_rows(4)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "POST body measurement limit: {} KiB.",
                                        MAX_REQUEST_BODY_BYTES / 1024
                                    ))
                                    .small()
                                    .color(muted),
                                );
                            }
                        });
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(9.0);
                ui.horizontal_wrapped(|ui| {
                    let can_start = self.running.is_none() && !self.target.trim().is_empty();
                    if ui
                        .add_enabled(
                            can_start,
                            egui::Button::new(
                                egui::RichText::new("▶  Start test").strong(),
                            )
                            .min_size(egui::vec2(142.0, 36.0))
                            .fill(egui::Color32::from_rgb(29, 137, 96)),
                        )
                        .clicked()
                    {
                        self.start_test();
                    }
                    if ui
                        .add_enabled(
                            self.running.is_some(),
                            egui::Button::new(
                                egui::RichText::new("■  Stop run").strong(),
                            )
                            .min_size(egui::vec2(120.0, 36.0))
                            .fill(egui::Color32::from_rgb(151, 62, 70)),
                        )
                        .clicked()
                    {
                        self.stop_test();
                    }
                    if ui
                        .add_enabled(
                            self.running.is_none(),
                            egui::Button::new("Clear live results")
                                .min_size(egui::vec2(132.0, 36.0)),
                        )
                        .clicked()
                    {
                        self.reset_results();
                        self.error = None;
                    }
                    ui.label(
                        egui::RichText::new(&self.status)
                            .strong()
                            .color(if self.running.is_some() {
                                egui::Color32::from_rgb(91, 183, 238)
                            } else if self.status == "Failed" {
                                egui::Color32::LIGHT_RED
                            } else if self.status == "Completed" {
                                egui::Color32::from_rgb(92, 202, 151)
                            } else {
                                muted
                            }),
                    );
                });
            });

        if let Some(error) = &self.error {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(54, 29, 35))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(133, 65, 76),
                ))
                .corner_radius(10)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.colored_label(egui::Color32::from_rgb(255, 163, 169), error);
                });
        }

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.heading(
                egui::RichText::new("Live measurements")
                    .size(20.0)
                    .strong()
                    .color(egui::Color32::from_rgb(235, 242, 250)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.2}s elapsed", self.elapsed_seconds))
                        .small()
                        .color(muted),
                );
            });
        });
        ui.label(
            egui::RichText::new("Aggregate counters cover the complete run; the on-screen sample table retains the most recent 10,000 responses.")
                .small()
                .color(muted),
        );
        ui.add_space(8.0);

        ui.columns(4, |columns| {
            Self::metric_card(
                &mut columns[0],
                "COMPLETED REQUESTS",
                self.total_received,
                "",
            );
            Self::metric_card(
                &mut columns[1],
                "OBSERVED THROUGHPUT",
                format!("{:.2}", self.observed_rps()),
                "RPS",
            );
            Self::metric_card(&mut columns[2], "HTTP 2XX", self.total_success, "");
            Self::metric_card(
                &mut columns[3],
                "NETWORK ERRORS",
                self.total_network_errors,
                "",
            );
        });
        ui.add_space(8.0);
        ui.columns(4, |columns| {
            Self::metric_card(&mut columns[0], "HTTP 3XX", self.total_redirects, "");
            Self::metric_card(&mut columns[1], "HTTP 4XX", self.total_http_4xx, "");
            Self::metric_card(&mut columns[2], "HTTP 5XX", self.total_http_5xx, "");
            let (avg, min, p50, p95, p99, max) = self.metrics();
            Self::metric_card(
                &mut columns[3],
                "AVERAGE LATENCY",
                format!("{avg:.1}"),
                "ms",
            );
            let _ = (min, p50, p95, p99, max);
        });

        let (_, min, p50, p95, p99, max) = self.metrics();
        ui.label(
            egui::RichText::new(format!(
                "HTTP latency: min {min:.1} ms   ·   P50 {p50:.1} ms   ·   P95 {p95:.1} ms   ·   P99 {p99:.1} ms   ·   max {max:.1} ms"
            ))
            .small()
            .color(muted),
        );
        ui.label(
            egui::RichText::new("HTTP latency includes 2xx–5xx responses and excludes transport failures and timeouts.")
                .small()
                .color(muted),
        );

        if self.profile == TestProfile::Protection {
            ui.add_space(10.0);
            egui::Frame::group(ui.style())
                .fill(card_fill)
                .stroke(card_stroke)
                .corner_radius(12)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("PROTECTION RESPONSES").strong().color(egui::Color32::from_rgb(158, 180, 205)));
                    ui.add_space(5.0);
                    ui.columns(4, |columns| {
                        Self::metric_card(&mut columns[0], "401 UNAUTHORIZED", self.total_unauthorized, "");
                        Self::metric_card(&mut columns[1], "403 FORBIDDEN", self.total_forbidden, "");
                        Self::metric_card(&mut columns[2], "429 RATE LIMITED", self.total_rate_limited, "");
                        Self::metric_card(&mut columns[3], "503 UNAVAILABLE", self.total_unavailable, "");
                    });
                    ui.label(egui::RichText::new("These are observed HTTP statuses, not proof of a particular WAF rule or protection action.")
                        .small().color(muted));
                });
        }

        ui.add_space(10.0);
        egui::Frame::group(ui.style())
            .fill(card_fill)
            .stroke(card_stroke)
            .corner_radius(12)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("REQUEST ACCOUNTING")
                        .strong()
                        .color(egui::Color32::from_rgb(158, 180, 205)),
                );
                ui.add_space(5.0);
                ui.horizontal_wrapped(|ui| {
                    for (label, value) in [
                        ("Scheduled", self.engine_stats.scheduled),
                        ("Started", self.engine_stats.started),
                        ("Completed", self.engine_stats.completed),
                        ("Queued", self.engine_stats.queued),
                        ("In flight", self.engine_stats.in_flight),
                        ("Cancelled", self.engine_stats.cancelled),
                        ("Dropped", self.engine_stats.dropped),
                    ] {
                        ui.label(
                            egui::RichText::new(format!("{label}: {value}"))
                                .color(egui::Color32::from_rgb(218, 230, 243)),
                        );
                        ui.add_space(7.0);
                    }
                });
                if self.engine_stats.limit_reached {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(238, 178, 82),
                        format!(
                            "Configured request budget of {} reached.",
                            self.max_requests
                        ),
                    );
                }
            });

        ui.add_space(10.0);
        egui::Frame::group(ui.style())
            .fill(card_fill)
            .stroke(card_stroke)
            .corner_radius(12)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("GENERATOR & HOST TELEMETRY").strong().color(egui::Color32::from_rgb(158, 180, 205)));
                if let Some(hardware) = self.hardware_samples.back() {
                    let process_memory_mb = hardware.process_memory_bytes.map(|bytes| bytes as f64 / 1_048_576.0);
                    let process_cpu_capacity = normalized_process_cpu_percent(hardware.process_cpu_percent, hardware.logical_cores);
                    ui.add_space(6.0);
                    ui.columns(3, |columns| {
                        Self::metric_card(&mut columns[0], "HOST CPU", format!("{:.1}", hardware.system_cpu_percent), "%");
                        Self::metric_card(&mut columns[1], "REDPULSE CPU", format!("{process_cpu_capacity:.1}"), "% of total PC capacity");
                        Self::metric_card(&mut columns[2], "PROCESS RAM", process_memory_mb.map(|value| format!("{value:.1}")).unwrap_or_else(|| "N/A".into()), "MB");
                    });
                    ui.add_space(7.0);
                    ui.columns(2, |columns| {
                        Self::metric_card(&mut columns[0], "HOST NETWORK RX", format!("{:.2}", hardware.network_rx_bytes_per_sec as f64 / 1_048_576.0), "MB/s");
                        Self::metric_card(&mut columns[1], "HOST NETWORK TX", format!("{:.2}", hardware.network_tx_bytes_per_sec as f64 / 1_048_576.0), "MB/s");
                    });
                    let host_memory_percent = if hardware.system_memory_total_bytes == 0 {
                        0.0
                    } else {
                        hardware.system_memory_used_bytes as f64 / hardware.system_memory_total_bytes as f64 * 100.0
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "{} logical cores   ·   Host RAM {:.1}% used   ·   Network throughput is host-wide, not process-only.",
                            hardware.logical_cores, host_memory_percent
                        ))
                        .small()
                        .color(muted),
                    );
                    if hardware.system_cpu_percent >= 95.0 || process_cpu_capacity >= 85.0 {
                        ui.add_space(4.0);
                        ui.colored_label(
                            egui::Color32::from_rgb(238, 178, 82),
                            "Low CPU headroom: the generator or host may be limiting this run. Interpret target results with caution.",
                        );
                    }
                    let cpu_points: PlotPoints = self.hardware_samples.iter().map(|sample| {
                        [
                            sample.elapsed_ms as f64 / 1000.0,
                            normalized_process_cpu_percent(sample.process_cpu_percent, sample.logical_cores) as f64,
                        ]
                    }).collect();
                    Plot::new("hardware_cpu_plot")
                        .show_x(false)
                        .show_y(false)
                        .cursor_color(egui::Color32::TRANSPARENT)
                        .height(125.0)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .allow_drag(false)
                        .allow_boxed_zoom(false)
                        .allow_double_click_reset(false)
                        .x_axis_label("Test time (s)")
                        .y_axis_label("RedPulse CPU (% of PC)")
                        .include_y(0.0)
                        .show(ui, |plot_ui| {
                            plot_ui.line(Line::new("RedPulse CPU", cpu_points)
                                .color(egui::Color32::from_rgb(83, 183, 246))
                                .width(2.0_f32));
                        });
                } else {
                    ui.label(egui::RichText::new("CPU, memory and network telemetry appears after the first half-second sample.")
                        .small().color(muted));
                }
            });

        if self.dns_strategy == DnsStrategy::RoundRobin {
            ui.add_space(10.0);
            egui::Frame::group(ui.style())
                .fill(card_fill)
                .stroke(card_stroke)
                .corner_radius(12)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("DESTINATION IP DISTRIBUTION").strong().color(egui::Color32::from_rgb(158, 180, 205)));
                    if self.resolved_addresses.is_empty() {
                        ui.label(egui::RichText::new("Resolving target addresses…").small().color(muted));
                    } else {
                        let total: u64 = self.origin_metrics.values().map(|metric| metric.requests).sum();
                        for address in &self.resolved_addresses {
                            let metric = self.origin_metrics.get(address).cloned().unwrap_or_default();
                            let share = if total == 0 { 0.0 } else { metric.requests as f64 * 100.0 / total as f64 };
                            let average = metric.average_latency_ms().map(|value| format!("{value:.1} ms")).unwrap_or_else(|| "—".into());
                            ui.label(format!("{address}   ·   {} requests ({share:.1}%)   ·   average {average}   ·   {} errors", metric.requests, metric.errors));
                        }
                        ui.label(egui::RichText::new("These are destination IPs selected by RedPulse. CDN anycast addresses do not necessarily represent separate origin servers.")
                            .small().color(muted));
                        if self.resolved_addresses.len() <= 1 {
                            ui.colored_label(egui::Color32::from_rgb(238, 178, 82), "Only one destination IP was resolved; this run cannot compare multi-IP distribution.");
                        }
                    }
                });
        }

        if !self.backend_metrics.is_empty() {
            ui.add_space(10.0);
            egui::Frame::group(ui.style())
                .fill(card_fill)
                .stroke(card_stroke)
                .corner_radius(12)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("BACKEND DISTRIBUTION").strong().color(egui::Color32::from_rgb(158, 180, 205)));
                    let total: u64 = self.backend_metrics.values().map(|metric| metric.requests).sum();
                    for (backend, metric) in &self.backend_metrics {
                        let share = if total == 0 { 0.0 } else { metric.requests as f64 * 100.0 / total as f64 };
                        let average = metric.average_latency_ms().map(|value| format!("{value:.1} ms")).unwrap_or_else(|| "—".into());
                        ui.label(format!("{backend}   ·   {} requests ({share:.1}%)   ·   average {average}   ·   {} errors", metric.requests, metric.errors));
                    }
                    ui.label(egui::RichText::new(format!("Identified {total} of {} completed responses. Backend identity depends on a stable response header emitted by the application.", self.total_received))
                        .small().color(muted));
                });
        } else if self.profile == TestProfile::LoadBalancer && self.total_received > 0 {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("No backend ID was observed. Destination IP distribution is measured, but private backend distribution cannot be confirmed without a response header such as X-Backend-ID.")
                .small().color(muted));
        }

        ui.add_space(10.0);
        egui::Frame::group(ui.style())
            .fill(card_fill)
            .stroke(card_stroke)
            .corner_radius(12)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("RESPONSE LATENCY").strong().color(egui::Color32::from_rgb(158, 180, 205)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("{} retained responses", self.recent_latencies.len())).small().color(muted));
                    });
                });
                if self.http_response_count > 0 {
                    ui.label(
                        egui::RichText::new(format!("P50 {p50:.1} ms   ·   P95 {p95:.1} ms"))
                            .strong()
                            .color(egui::Color32::from_rgb(214, 228, 244)),
                    );
                } else {
                    ui.label(egui::RichText::new("Waiting for HTTP response measurements…").small().color(muted));
                }
                ui.label(egui::RichText::new("Each point represents one completed HTTP response. P50 and P95 are summary values.")
                    .small().color(muted));
                let points: PlotPoints = downsample_min_max(&self.recent_latencies, MAX_PLOT_POINTS).into();
                if self.recent_latencies.is_empty() {
                    ui.add_space(7.0);
                    ui.label(egui::RichText::new("No response samples yet.").color(muted));
                } else {
                    Plot::new("latency_plot")
                        .show_x(false)
                        .show_y(false)
                        .cursor_color(egui::Color32::TRANSPARENT)
                        .height(190.0)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .allow_drag(false)
                        .allow_boxed_zoom(false)
                        .allow_double_click_reset(false)
                        .x_axis_label("Test time (s)")
                        .y_axis_label("Response latency (ms)")
                        .include_x(0.0)
                        .include_y(0.0)
                        .show(ui, |plot_ui| {
                            plot_ui.line(Line::new("Response latency", points)
                                .color(egui::Color32::from_rgb(83, 183, 246))
                                .width(2.0_f32));
                        });
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Engine elapsed: {:.3}s", self.elapsed_seconds)).small().color(muted));
                    ui.label(egui::RichText::new(format!("Retained samples: {}", self.samples.len())).small().color(muted));
                });
            });
    }

    fn results_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("Test results")
                .size(23.0)
                .strong()
                .color(egui::Color32::from_rgb(235, 242, 250)),
        );
        if let Some(run) = &self.last_run {
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(22, 29, 41))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(45, 61, 81),
                ))
                .corner_radius(12)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("RUN CONFIGURATION")
                            .small()
                            .strong()
                            .color(egui::Color32::from_rgb(139, 159, 184)),
                    );
                    ui.label(
                        egui::RichText::new(&run.target)
                            .strong()
                            .color(egui::Color32::from_rgb(105, 190, 250)),
                    );
                    let rate_description = run
                        .request_rate
                        .map(|rate| format!("{rate} req/s arrival target"))
                        .unwrap_or_else(|| "throughput determined by workers".into());
                    ui.label(format!(
                        "{} profile · {} · {} · {} · {} workers · {} s configured · {} · {} request budget · {} s timeout · {:.3} s measured · backend header {}",
                        run.profile.label(),
                        run.method,
                        run.load_model.label(),
                        run.dns_strategy.label(),
                        run.workers,
                        run.duration_secs,
                        rate_description,
                        run.max_total_requests,
                        run.timeout_secs,
                        run.actual_elapsed_secs,
                        if run.backend_id_header.is_empty() { "disabled" } else { &run.backend_id_header }
                    ));
                    if !run.resolved_addresses.is_empty() {
                        ui.label(format!("Pinned DNS addresses: {}", run.resolved_addresses.join(", ")));
                    }
                });
        }
        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(22, 29, 41))
            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(45, 61, 81)))
            .corner_radius(12)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("SESSION HISTORY")
                            .strong()
                            .color(egui::Color32::from_rgb(145, 183, 219)),
                    );
                    ui.label(format!(
                        "{} runs · {:.2} / 64 MiB · RAM only",
                        self.session_history.len(),
                        self.session_history.memory_bytes() as f64 / 1_048_576.0
                    ));
                    if self.session_history.evicted_runs() > 0 {
                        ui.colored_label(
                            egui::Color32::from_rgb(238, 178, 82),
                            format!("{} runs not retained due to the memory limit", self.session_history.evicted_runs()),
                        );
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Completed Performance, Load Balancer and Protection tests stay in memory only. Nothing is saved automatically; the history disappears when RedPulse closes. Manual JSON export creates a separate file that remains until you delete it.",
                    )
                    .small()
                    .color(egui::Color32::from_rgb(137, 157, 181)),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.session_history.is_empty(), egui::Button::new("Export session JSON"))
                        .clicked()
                    {
                        self.export(false);
                    }
                    if ui
                        .add_enabled(!self.samples.is_empty(), egui::Button::new("Export current CSV"))
                        .clicked()
                    {
                        self.export(true);
                    }
                    if ui
                        .add_enabled(!self.session_history.is_empty(), egui::Button::new("Clear session history"))
                        .clicked()
                    {
                        self.session_history.clear();
                        self.next_session_run_id = 1;
                        self.session_started_at = chrono::Utc::now().to_rfc3339();
                        self.last_export = None;
                    }
                    if let Some(message) = &self.last_export {
                        ui.label(message);
                    }
                });
                egui::CollapsingHeader::new(format!("Previous tests ({})", self.session_history.len()))
                    .default_open(false)
                    .show(ui, |ui| {
                        for entry in self.session_history.entries().iter().rev().take(12) {
                            ui.label(format!(
                                "#{} · {} · {} · {} · {} · {:.1} KB · {}",
                                entry.id,
                                entry.profile,
                                entry.kind,
                                entry.outcome,
                                entry.target,
                                entry.payload_bytes() as f64 / 1024.0,
                                entry.completed_at
                            ));
                        }
                        if self.session_history.len() > 12 {
                            ui.label(format!("{} earlier runs are retained in the session JSON.", self.session_history.len() - 12));
                        }
                    });
            });

        ui.add_space(8.0);
        let (avg, min, p50, p95, p99, max) = self.metrics();
        ui.label(format!(
            "{} completed requests · {} HTTP responses · {} HTTP 2xx · {} HTTP 3xx · {} HTTP 4xx · {} HTTP 5xx · {} network errors",
            self.total_received,
            self.http_response_count,
            self.total_success,
            self.total_redirects,
            self.total_http_4xx,
            self.total_http_5xx,
            self.total_network_errors
        ));
        if self.profile == TestProfile::Protection && self.last_run.is_some() {
            ui.label(format!(
                "Protection status counts: 401 {} · 403 {} · 429 {} · 503 {}",
                self.total_unauthorized,
                self.total_forbidden,
                self.total_rate_limited,
                self.total_unavailable
            ));
        }
        if !self.backend_metrics.is_empty() {
            ui.heading("Backend distribution");
            let total: u64 = self
                .backend_metrics
                .values()
                .map(|metric| metric.requests)
                .sum();
            for (backend, metric) in &self.backend_metrics {
                let share = if total == 0 {
                    0.0
                } else {
                    metric.requests as f64 * 100.0 / total as f64
                };
                ui.label(format!(
                    "{backend}: {} requests ({share:.1}%) · {} errors",
                    metric.requests, metric.errors
                ));
            }
        }
        if self.dns_strategy == DnsStrategy::RoundRobin {
            let total: u64 = self
                .origin_metrics
                .values()
                .map(|metric| metric.requests)
                .sum();
            ui.heading("Destination IP distribution");
            for address in &self.resolved_addresses {
                let metric = self
                    .origin_metrics
                    .get(address)
                    .cloned()
                    .unwrap_or_default();
                let share = if total == 0 {
                    0.0
                } else {
                    metric.requests as f64 * 100.0 / total as f64
                };
                ui.label(format!(
                    "{address}: {} requests ({share:.1}%) · {} errors",
                    metric.requests, metric.errors
                ));
            }
        }
        ui.label(format!(
            "Scheduled {} · Started {} · Completed {} · Queued {} · In-flight {} · Cancelled {} · Dropped {}",
            self.engine_stats.scheduled,
            self.engine_stats.started,
            self.engine_stats.completed,
            self.engine_stats.queued,
            self.engine_stats.in_flight,
            self.engine_stats.cancelled,
            self.engine_stats.dropped
        ));
        ui.label(format!(
            "Min {min:.2} ms · Average {avg:.2} ms · P50 {p50:.2} ms · P95 {p95:.2} ms · P99 {p99:.2} ms · Max {max:.2} ms"
        ));
        ui.label(format!(
            "Observed throughput: {:.2} RPS over {:.3} seconds",
            self.observed_rps(),
            self.elapsed_seconds
        ));
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("sample_table")
                .striped(true)
                .show(ui, |ui| {
                    for heading in [
                        "Elapsed",
                        "Status",
                        "Protocol",
                        "Latency",
                        "Origin IP",
                        "Bytes",
                        "Result",
                    ] {
                        ui.strong(heading);
                    }
                    ui.end_row();

                    for sample in self.samples.iter().rev().take(500) {
                        ui.label(format!("{:.2}s", sample.elapsed_ms as f64 / 1000.0));
                        let color = match classify_status(sample.status) {
                            ResponseClass::NetworkError | ResponseClass::ServerError => {
                                egui::Color32::LIGHT_RED
                            }
                            ResponseClass::ClientError => egui::Color32::YELLOW,
                            ResponseClass::Redirect => egui::Color32::from_rgb(105, 174, 238),
                            ResponseClass::Success => egui::Color32::LIGHT_GREEN,
                            ResponseClass::Other => egui::Color32::GRAY,
                        };
                        ui.colored_label(
                            color,
                            if sample.status == 0 {
                                "ERR".into()
                            } else {
                                sample.status.to_string()
                            },
                        );
                        ui.label(sample.http_version.as_deref().unwrap_or("—"));
                        ui.label(format!("{:.1} ms", sample.latency_ms));
                        ui.label(sample.endpoint_ip.as_deref().unwrap_or("System DNS"));
                        ui.label(sample.bytes.to_string());
                        let details = sample.error.as_deref().unwrap_or(if sample.ok {
                            "HTTP response received"
                        } else {
                            "HTTP error response"
                        });
                        let response = ui.label(details);
                        if sample.error.is_some() {
                            response.on_hover_text(details);
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn about_ui(&self, ui: &mut egui::Ui) {
        ui.heading(
            egui::RichText::new("About RedPulse")
                .size(23.0)
                .strong()
                .color(egui::Color32::from_rgb(235, 242, 250)),
        );
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.label("A native Rust Windows desktop HTTP load-testing and infrastructure resilience workbench.");
        ui.add_space(8.0);
        ui.label("Engine: Tokio + Reqwest + Rustls");
        ui.label("Interface: egui / eframe");
        ui.label(
            "Protection Audit: read-only HTTP surface, edge-provider and security-header checks",
        );
        ui.label("Load testing: Constant Arrival Rate and Closed Loop");
        ui.label("DNS routing: system resolver or round-robin across resolved IP addresses");
        ui.label("Metrics: response status, throughput, latency percentiles and generator/host hardware telemetry");
        ui.label("Reports: JSON and CSV for benchmark runs; JSON retains hardware samples");
        ui.label("Protection Audit: six sequential read-only HTTP checks, no redirects, five-second timeout");
        ui.separator();
        ui.label(
            "Constant Arrival Rate schedules requests independently from HTTP worker completion.",
        );
        ui.label("Closed Loop lets each worker repeat only after its previous request completes.");
        ui.label("A bounded queue reports constant-rate overload as dropped arrivals instead of delaying future slots.");
        ui.label("Protection Audit uses a fixed low-volume probe set: GET, HEAD, OPTIONS, security.txt, robots.txt and sitemap.xml.");
        ui.label("No credential guessing, exploit payloads, destructive HTTP methods or recursive wordlist scans are part of this read-only audit.");
        ui.label("Redirect following is disabled; each sample measures the configured endpoint response.");
    }
}

impl eframe::App for RedPulseApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_engine();
        self.recon.poll();
        if let Some((target, result)) = self.recon.take_completed_result() {
            self.record_protection_result(target, result);
        }
        if self.running.is_some() || self.recon.is_running() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        egui::TopBottomPanel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(14, 19, 28))
                    .inner_margin(egui::Margin::symmetric(16, 11)),
            )
            .show(ctx, |ui| self.header(ui));
        egui::SidePanel::left("navigation")
            .resizable(false)
            .default_width(158.0)
            .frame(egui::Frame::new().fill(egui::Color32::from_rgb(15, 20, 29)))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                if ui
                    .selectable_label(self.tab == Tab::Recon, "◈  Protection")
                    .clicked()
                {
                    self.tab = Tab::Recon;
                }
                if ui
                    .selectable_label(self.tab == Tab::Tester, "◉  Traffic Lab")
                    .clicked()
                {
                    self.tab = Tab::Tester;
                }
                if ui
                    .selectable_label(self.tab == Tab::Results, "▤  Results")
                    .clicked()
                {
                    self.tab = Tab::Results;
                }
                if ui
                    .selectable_label(self.tab == Tab::About, "ⓘ  About")
                    .clicked()
                {
                    self.tab = Tab::About;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(concat!("REDPULSE ", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Recon => self.recon.ui(ui),
                Tab::Tester => self.tester_ui(ui),
                Tab::Results => self.results_ui(ui),
                Tab::About => self.about_ui(ui),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DistributionMetrics, LoadModel, RedPulseApp, TestProfile, normalized_process_cpu_percent,
        parse_arrival_rate_input, parse_numeric_input,
    };

    #[test]
    fn distribution_metrics_track_requests_errors_and_response_latency() {
        let mut metrics = DistributionMetrics::default();
        metrics.record(200, 10.0);
        metrics.record(503, 20.0);
        metrics.record(0, 0.0);

        assert_eq!(metrics.requests, 3);
        assert_eq!(metrics.errors, 2);
        assert_eq!(metrics.latency_samples, 2);
        assert_eq!(metrics.average_latency_ms(), Some(15.0));
    }

    #[test]
    fn test_profiles_have_clear_distinct_purposes() {
        assert_ne!(
            TestProfile::Performance.description(),
            TestProfile::LoadBalancer.description()
        );
        assert_ne!(
            TestProfile::LoadBalancer.description(),
            TestProfile::Protection.description()
        );
        assert_eq!(TestProfile::LoadBalancer.label(), "Load Balancer");
    }

    #[test]
    fn process_cpu_percentage_is_normalized_to_total_pc_capacity() {
        assert!((normalized_process_cpu_percent(101.5, 8) - 12.6875).abs() < 0.0001);
        assert_eq!(normalized_process_cpu_percent(200.0, 0), 100.0);
        assert_eq!(normalized_process_cpu_percent(f32::NAN, 8), 0.0);
        assert_eq!(normalized_process_cpu_percent(-5.0, 8), 0.0);
    }

    #[test]
    fn numeric_text_fields_accept_large_user_supplied_values() {
        assert_eq!(parse_numeric_input::<usize>(" 4096 ", "Workers"), Ok(4096));
        assert_eq!(
            parse_numeric_input::<u64>("2000000", "Arrival rate"),
            Ok(2_000_000)
        );
        assert_eq!(
            parse_numeric_input::<u64>("5000000000", "Request budget"),
            Ok(5_000_000_000)
        );
    }

    #[test]
    fn closed_loop_ignores_the_unused_arrival_rate_field() {
        assert_eq!(
            parse_arrival_rate_input("not-used", LoadModel::ClosedLoop, 5),
            Ok(5)
        );
        assert_eq!(
            parse_arrival_rate_input("12", LoadModel::ConstantArrivalRate, 5),
            Ok(12)
        );
        assert!(parse_arrival_rate_input("invalid", LoadModel::ConstantArrivalRate, 5).is_err());
    }

    #[test]
    fn numeric_text_fields_reject_empty_and_fractional_values() {
        assert!(parse_numeric_input::<u64>("", "Duration").is_err());
        assert!(parse_numeric_input::<u64>("3.5", "Timeout").is_err());
        assert!(parse_numeric_input::<usize>("many", "Workers").is_err());
    }

    #[test]
    fn observed_rps_uses_measured_elapsed_time() {
        assert!((RedPulseApp::calculate_rps(150, 30.0) - 5.0).abs() < 0.000_001);
        assert!((RedPulseApp::calculate_rps(10, 2.0) - 5.0).abs() < 0.000_001);
    }

    #[test]
    fn observed_rps_handles_zero_or_invalid_elapsed_time() {
        assert_eq!(RedPulseApp::calculate_rps(10, 0.0), 0.0);
        assert_eq!(RedPulseApp::calculate_rps(10, -1.0), 0.0);
        assert_eq!(RedPulseApp::calculate_rps(10, f64::NAN), 0.0);
        assert_eq!(RedPulseApp::calculate_rps(10, f64::INFINITY), 0.0);
    }
}

#[cfg(test)]
mod response_class_tests {
    use super::{ResponseClass, classify_status};

    #[test]
    fn classifies_http_success_responses() {
        assert_eq!(classify_status(200), ResponseClass::Success);
        assert_eq!(classify_status(204), ResponseClass::Success);
    }

    #[test]
    fn classifies_redirect_responses_separately() {
        assert_eq!(classify_status(301), ResponseClass::Redirect);
        assert_eq!(classify_status(302), ResponseClass::Redirect);
        assert_eq!(classify_status(304), ResponseClass::Redirect);
    }

    #[test]
    fn classifies_client_and_server_errors() {
        assert_eq!(classify_status(405), ResponseClass::ClientError);
        assert_eq!(classify_status(429), ResponseClass::ClientError);
        assert_eq!(classify_status(500), ResponseClass::ServerError);
        assert_eq!(classify_status(503), ResponseClass::ServerError);
    }

    #[test]
    fn classifies_transport_errors_and_unusual_statuses() {
        assert_eq!(classify_status(0), ResponseClass::NetworkError);
        assert_eq!(classify_status(101), ResponseClass::Other);
    }
}

#[cfg(test)]
mod load_model_ui_tests {
    use crate::engine::LoadModel;

    #[test]
    fn load_models_have_clear_user_facing_names() {
        assert_eq!(
            LoadModel::ConstantArrivalRate.label(),
            "Constant Arrival Rate"
        );
        assert_eq!(LoadModel::ClosedLoop.label(), "Closed Loop");
    }
}

#[cfg(test)]
mod latency_histogram_tests {
    use super::Histogram;

    #[test]
    fn latency_histogram_preserves_samples_beyond_one_minute() {
        let mut histogram = Histogram::<u64>::new(3).expect("valid histogram configuration");
        histogram
            .record(120_000_000)
            .expect("two-minute latency should be representable");
        assert!(
            histogram.value_at_quantile(1.0) > 60_000_000,
            "latency percentiles must not be silently capped at one minute"
        );
    }
}

#[cfg(test)]
mod chart_downsampling_tests {
    use super::downsample_min_max;
    use std::collections::VecDeque;

    #[test]
    fn chart_downsampling_retains_full_time_range() {
        let points: VecDeque<(f64, f64)> = (0..10_000)
            .map(|index| (index as f64 / 100.0, 50.0))
            .collect();

        let reduced = downsample_min_max(&points, 2_000);
        assert!(reduced.len() <= 2_000);
        assert_eq!(reduced.first().expect("first point")[0], 0.0);
        assert_eq!(reduced.last().expect("last point")[0], 99.99);
        assert!(reduced.windows(2).all(|pair| pair[0][0] <= pair[1][0]));
    }

    #[test]
    fn chart_downsampling_preserves_minimum_and_maximum_spikes() {
        let mut points: VecDeque<(f64, f64)> =
            (0..10_000).map(|index| (index as f64, 50.0)).collect();
        points[4_321].1 = 900.0;
        points[7_654].1 = 1.0;

        let reduced = downsample_min_max(&points, 2_000);
        let values: Vec<f64> = reduced.iter().map(|point| point[1]).collect();
        assert!(values.contains(&900.0));
        assert!(values.contains(&1.0));
        assert!(reduced.len() <= 2_000);
    }

    #[test]
    fn short_chart_series_is_not_modified() {
        let points = VecDeque::from([(0.0, 10.0), (0.5, 15.0), (1.0, 8.0)]);
        assert_eq!(
            downsample_min_max(&points, 2_000),
            vec![[0.0, 10.0], [0.5, 15.0], [1.0, 8.0]]
        );
    }
}
