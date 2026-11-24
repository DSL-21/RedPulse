use std::{
    collections::BTreeSet,
    io::Read,
    net::ToSocketAddrs,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use eframe::egui;
use reqwest::{
    blocking::{Client, Response},
    header::{HeaderMap, HeaderValue, ALLOW, SET_COOKIE},
    Method, StatusCode,
};
use serde::Serialize;
use url::Url;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const MAX_PROBES: usize = 6;
const MAX_FINGERPRINT_BYTES: u64 = 128 * 1024;
const MAX_DISPLAYED_IPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
            Self::Info => "INFO",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Critical => egui::Color32::from_rgb(255, 75, 85),
            Self::High => egui::Color32::from_rgb(238, 112, 88),
            Self::Medium => egui::Color32::from_rgb(235, 174, 75),
            Self::Low => egui::Color32::from_rgb(95, 169, 231),
            Self::Info => egui::Color32::from_rgb(151, 166, 186),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub category: String,
    pub title: String,
    pub evidence: String,
    pub recommendation: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeResult {
    pub method: String,
    pub path: String,
    pub status: Option<u16>,
    pub elapsed_ms: f64,
    pub bytes_read: usize,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReconReport {
    pub target: String,
    pub host: String,
    pub resolved_addresses: Vec<String>,
    pub generated_at: DateTime<Utc>,
    pub probes: Vec<ProbeResult>,
    pub findings: Vec<Finding>,
    pub stopped_early: bool,
}

struct ProbeExecution {
    result: ProbeResult,
    headers: Option<HeaderMap>,
    body_sample: String,
}

#[derive(Default)]
pub struct ReconState {
    target: String,
    running: bool,
    receiver: Option<Receiver<Result<ReconReport, String>>>,
    report: Option<ReconReport>,
    error: Option<String>,
    status: String,
}

impl ReconState {
    pub fn new() -> Self {
        Self {
            target: "https://example.com/".into(),
            running: false,
            receiver: None,
            report: None,
            error: None,
            status: "READY".into(),
        }
    }

    fn start(&mut self) {
        self.report = None;
        self.error = None;
        let target = self.target.trim().to_owned();
        let (tx, rx) = mpsc::channel();
        self.running = true;
        self.status = "RUNNING RECON".into();
        self.receiver = Some(rx);

        let spawn = thread::Builder::new()
            .name("redpulse-recon".into())
            .spawn(move || {
                let result = run_assessment(&target);
                let _ = tx.send(result);
            });

        if let Err(error) = spawn {
            self.running = false;
            self.status = "FAILED".into();
            self.error = Some(format!("Could not start recon worker: {error}"));
            self.receiver = None;
        }
    }

    fn poll(&mut self) {
        let result = self.receiver.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Recon worker ended without returning a report.".into(),
            )),
        });

        if let Some(result) = result {
            self.receiver = None;
            self.running = false;
            match result {
                Ok(report) => {
                    self.status = if report.stopped_early {
                        "COMPLETED WITH EARLY STOP".into()
                    } else {
                        "COMPLETED".into()
                    };
                    self.report = Some(report);
                }
                Err(error) => {
                    self.status = "FAILED".into();
                    self.error = Some(error);
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll();

        ui.add_space(4.0);
        ui.heading(
            egui::RichText::new("Web Recon & Audit")
                .size(23.0)
                .strong()
                .color(egui::Color32::from_rgb(235, 242, 250)),
        );
        ui.label(
            egui::RichText::new(
                "Map the exposed HTTP surface and inspect common security controls.",
            )
            .color(egui::Color32::from_rgb(143, 160, 181)),
        );
        ui.add_space(12.0);

        egui::Frame::group(ui.style())
            .fill(egui::Color32::from_rgb(22, 29, 41))
            .stroke(egui::Stroke::new(
                1.0,
                egui::Color32::from_rgb(48, 65, 86),
            ))
            .corner_radius(12)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.label("TARGET");
                ui.add_enabled(
                    !self.running,
                    egui::TextEdit::singleline(&mut self.target)
                        .hint_text("https://your-lab.example")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.running && !self.target.trim().is_empty(),
                            egui::Button::new("◈  Run recon")
                                .fill(egui::Color32::from_rgb(150, 48, 61)),
                        )
                        .clicked()
                    {
                        self.start();
                    }
                    if ui
                        .add_enabled(!self.running, egui::Button::new("Clear findings"))
                        .clicked()
                    {
                        self.report = None;
                        self.error = None;
                        self.status = "READY".into();
                    }
                    ui.label(
                        egui::RichText::new(&self.status)
                            .strong()
                            .small()
                            .color(if self.running {
                                egui::Color32::from_rgb(235, 174, 75)
                            } else {
                                egui::Color32::from_rgb(95, 190, 145)
                            }),
                    );
                });

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "6 fixed probes · sequential · 250 ms interval · 5 s timeout · no redirect following",
                    )
                    .small()
                    .color(egui::Color32::from_rgb(137, 157, 181)),
                );
            });

        if self.running {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Collecting HTTP metadata and security configuration…");
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        if let Some(error) = &self.error {
            ui.add_space(12.0);
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }

        if let Some(report) = &self.report {
            render_report(ui, report);
        }
    }
}

fn validate_target(target: &str) -> Result<Url, String> {
    let url = Url::parse(target).map_err(|error| format!("Invalid target URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Only absolute HTTP and HTTPS target URLs are supported.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Credentials in target URLs are not supported.".into());
    }
    Ok(url)
}

fn resolve_addresses(url: &Url) -> Vec<String> {
    let Some(host) = url.host_str() else {
        return Vec::new();
    };
    let Some(port) = url.port_or_known_default() else {
        return Vec::new();
    };
    (host, port)
        .to_socket_addrs()
        .map(|addresses| {
            addresses
                .map(|address| address.ip().to_string())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(MAX_DISPLAYED_IPS)
                .collect()
        })
        .unwrap_or_default()
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn add_finding(
    findings: &mut Vec<Finding>,
    severity: Severity,
    category: &str,
    title: &str,
    evidence: impl Into<String>,
    recommendation: &str,
) {
    findings.push(Finding {
        severity,
        category: category.into(),
        title: title.into(),
        evidence: evidence.into(),
        recommendation: recommendation.into(),
    });
}

fn analyze_cors(headers: &HeaderMap, findings: &mut Vec<Finding>) {
    let allow_origin = header_text(headers, "access-control-allow-origin");
    let allow_credentials = header_text(headers, "access-control-allow-credentials");
    if allow_origin.as_deref() == Some("*")
        && allow_credentials
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        add_finding(
            findings,
            Severity::Medium,
            "CORS",
            "Wildcard origin combined with credentials",
            "Access-Control-Allow-Origin: * and Access-Control-Allow-Credentials: true were both observed.",
            "Review the CORS policy and return explicit trusted origins instead of a wildcard.",
        );
    }
}

fn analyze_headers(url: &Url, headers: &HeaderMap, body: &str, findings: &mut Vec<Finding>) {
    let is_https = url.scheme() == "https";
    let csp = header_text(headers, "content-security-policy");

    if is_https && headers.get("strict-transport-security").is_none() {
        add_finding(
            findings,
            Severity::Medium,
            "Transport security",
            "HSTS header not observed",
            "HTTPS response did not include Strict-Transport-Security.",
            "Consider enabling HSTS after confirming HTTPS works across the intended host scope.",
        );
    }

    if csp.is_none() {
        add_finding(
            findings,
            Severity::Low,
            "Browser security",
            "Content Security Policy not observed",
            "The response did not include Content-Security-Policy.",
            "Define a restrictive Content-Security-Policy appropriate for the application's resources.",
        );
    }

    if headers.get("x-content-type-options").is_none() {
        add_finding(
            findings,
            Severity::Low,
            "Browser security",
            "MIME sniffing protection not observed",
            "X-Content-Type-Options was absent.",
            "Set X-Content-Type-Options: nosniff where appropriate.",
        );
    }

    let csp_blocks_frames = csp
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().contains("frame-ancestors"));
    if headers.get("x-frame-options").is_none() && !csp_blocks_frames {
        add_finding(
            findings,
            Severity::Low,
            "Browser security",
            "Framing protection not observed",
            "Neither X-Frame-Options nor a CSP frame-ancestors directive was observed.",
            "Define an appropriate frame-ancestors directive or X-Frame-Options policy.",
        );
    }

    if headers.get("referrer-policy").is_none() {
        add_finding(
            findings,
            Severity::Info,
            "Browser security",
            "Referrer policy not explicit",
            "Referrer-Policy was absent.",
            "Set a Referrer-Policy that matches the application's privacy requirements.",
        );
    }

    if headers.get("permissions-policy").is_none() {
        add_finding(
            findings,
            Severity::Info,
            "Browser security",
            "Permissions Policy not observed",
            "Permissions-Policy was absent.",
            "Restrict browser features that the application does not need.",
        );
    }

    for cookie in headers.get_all(SET_COOKIE).iter() {
        let Ok(cookie) = cookie.to_str() else { continue };
        let lower = cookie.to_ascii_lowercase();
        let name = cookie.split(';').next().unwrap_or("cookie").trim();
        for (flag, label) in [
            ("; secure", "Secure"),
            ("; httponly", "HttpOnly"),
            ("; samesite=", "SameSite"),
        ] {
            if !lower.contains(flag) {
                add_finding(
                    findings,
                    Severity::Low,
                    "Session security",
                    &format!("Cookie missing {label} attribute"),
                    format!("Set-Cookie: {name} …"),
                    "Review the cookie's purpose and set Secure, HttpOnly and an appropriate SameSite value where applicable.",
                );
            }
        }
    }

    analyze_cors(headers, findings);

    for name in ["server", "x-powered-by", "x-aspnet-version", "x-generator"] {
        if let Some(value) = header_text(headers, name) {
            if !value.trim().is_empty() {
                add_finding(
                    findings,
                    Severity::Info,
                    "Fingerprinting",
                    "Technology header disclosed",
                    format!("{name}: {value}"),
                    "Review whether this technology detail needs to be exposed publicly.",
                );
            }
        }
    }

    let body_lower = body.to_ascii_lowercase();
    for (marker, technology) in [
        ("wp-content", "WordPress"),
        ("drupal.settings", "Drupal"),
        ("joomla!", "Joomla"),
        ("__next_data__", "Next.js"),
        ("_nuxt/", "Nuxt"),
    ] {
        if body_lower.contains(marker) {
            add_finding(
                findings,
                Severity::Info,
                "Fingerprinting",
                "Possible web technology identified",
                format!("HTML marker matched: {marker}"),
                &format!("Verify the {technology} fingerprint manually; content markers can produce false positives."),
            );
        }
    }
}

fn analyze_allowed_methods(headers: &HeaderMap, findings: &mut Vec<Finding>) {
    let allow = header_text(headers, ALLOW.as_str())
        .or_else(|| header_text(headers, "access-control-allow-methods"));
    let Some(allow) = allow else { return };

    let methods: Vec<String> = allow
        .split(',')
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();

    add_finding(
        findings,
        Severity::Info,
        "HTTP surface",
        "HTTP methods advertised",
        format!("Allowed methods observed: {}", methods.join(", ")),
        "Review the advertised methods and ensure each is intentionally supported.",
    );

    let unusual: Vec<&str> = methods
        .iter()
        .map(String::as_str)
        .filter(|method| matches!(*method, "TRACE" | "PUT" | "DELETE" | "CONNECT"))
        .collect();
    if !unusual.is_empty() {
        add_finding(
            findings,
            Severity::Medium,
            "HTTP surface",
            "Sensitive HTTP methods advertised",
            format!("Advertised methods include: {}", unusual.join(", ")),
            "Confirm these methods are required and protected by the intended authorization controls.",
        );
    }
}

fn read_bounded_body(response: Response) -> (usize, String) {
    let mut body = Vec::new();
    let result = response
        .take(MAX_FINGERPRINT_BYTES)
        .read_to_end(&mut body);
    if result.is_err() {
        return (body.len(), String::from_utf8_lossy(&body).into_owned());
    }
    (body.len(), String::from_utf8_lossy(&body).into_owned())
}

fn run_probe(
    client: &Client,
    method: Method,
    url: &Url,
    read_body: bool,
) -> ProbeExecution {
    let start = Instant::now();
    let method_name = method.as_str().to_owned();
    let path = url.path().to_owned();

    match client.request(method, url.clone()).send() {
        Ok(response) => {
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let (bytes_read, body_sample) = if read_body {
                read_bounded_body(response)
            } else {
                (0, String::new())
            };
            let location = header_text(&headers, "location");
            let content_type = header_text(&headers, "content-type");
            let note = if let Some(location) = location {
                format!("Location: {location}")
            } else if let Some(content_type) = content_type {
                format!("{status} · {content_type}")
            } else {
                format!(
                    "{} response",
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK)
                )
            };

            ProbeExecution {
                result: ProbeResult {
                    method: method_name,
                    path,
                    status: Some(status),
                    elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    bytes_read,
                    note,
                },
                headers: Some(headers),
                body_sample,
            }
        }
        Err(error) => ProbeExecution {
            result: ProbeResult {
                method: method_name,
                path,
                status: None,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                bytes_read: 0,
                note: format!("Request failed: {error}"),
            },
            headers: None,
            body_sample: String::new(),
        },
    }
}

pub fn run_assessment(target: &str) -> Result<ReconReport, String> {
    let target_url = validate_target(target)?;
    let host = target_url.host_str().unwrap_or_default().to_owned();
    let addresses = resolve_addresses(&target_url);

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("RedPulse/", env!("CARGO_PKG_VERSION"), " Recon"))
        .build()
        .map_err(|error| format!("Could not create HTTP client: {error}"))?;

    let mut root = target_url.clone();
    root.set_path("/");
    root.set_query(None);
    root.set_fragment(None);

    let security_txt = root
        .join("/.well-known/security.txt")
        .map_err(|error| format!("Could not build security.txt URL: {error}"))?;
    let robots = root
        .join("/robots.txt")
        .map_err(|error| format!("Could not build robots.txt URL: {error}"))?;
    let sitemap = root
        .join("/sitemap.xml")
        .map_err(|error| format!("Could not build sitemap URL: {error}"))?;

    let probes = [
        (Method::GET, target_url.clone(), true),
        (Method::HEAD, target_url.clone(), false),
        (Method::OPTIONS, target_url.clone(), false),
        (Method::GET, security_txt, false),
        (Method::GET, robots, false),
        (Method::GET, sitemap, false),
    ];

    let mut results = Vec::with_capacity(MAX_PROBES);
    let mut findings = Vec::new();
    let mut stopped_early = false;

    for (index, (method, url, read_body)) in probes.into_iter().enumerate() {
        if index > 0 {
            thread::sleep(PROBE_INTERVAL);
        }

        let execution = run_probe(&client, method, &url, read_body);
        let result = execution.result;

        if index == 0 {
            if result.status.is_none() {
                return Err(format!("Primary target request failed: {}", result.note));
            }

            if let Some(headers) = execution.headers.as_ref() {
                analyze_headers(&target_url, headers, &execution.body_sample, &mut findings);
            }

            if result.status.is_some_and(|status| (300..400).contains(&status)) {
                add_finding(
                    &mut findings,
                    Severity::Info,
                    "HTTP surface",
                    "Target returned a redirect",
                    result.note.clone(),
                    "Review the redirect destination manually; RedPulse does not follow redirects during recon.",
                );
            }
        }

        if index == 2 {
            if let Some(headers) = execution.headers.as_ref() {
                analyze_allowed_methods(headers, &mut findings);
                analyze_cors(headers, &mut findings);
            }
        }

        if result.status == Some(429) || result.status == Some(503) {
            add_finding(
                &mut findings,
                Severity::Info,
                "Probe control",
                "Server requested reduced probing",
                format!(
                    "{} {} returned HTTP {}. Remaining probes were skipped.",
                    result.method,
                    result.path,
                    result.status.unwrap_or_default()
                ),
                "Retry only after the service is ready; the assessment stopped to avoid adding unnecessary load.",
            );
            results.push(result);
            stopped_early = true;
            break;
        }

        results.push(result);
    }

    let mut safe_target = target_url.clone();
    safe_target.set_query(None);
    safe_target.set_fragment(None);

    let mut report = ReconReport {
        target: safe_target.to_string(),
        host,
        resolved_addresses: addresses,
        generated_at: Utc::now(),
        probes: results,
        findings,
        stopped_early,
    };

    analyze_probe_results(&target_url, &report.probes, &mut report.findings);
    Ok(report)
}

fn analyze_probe_results(url: &Url, probes: &[ProbeResult], findings: &mut Vec<Finding>) {
    for probe in probes {
        if let Some(status) = probe.status {
            if status == 200 && probe.path.ends_with("security.txt") {
                add_finding(
                    findings,
                    Severity::Info,
                    "Disclosure",
                    "security.txt is published",
                    "The well-known security.txt resource returned HTTP 200.",
                    "Review its contact and disclosure-policy details during the assessment.",
                );
            } else if status == 200 && probe.path.ends_with("robots.txt") {
                add_finding(
                    findings,
                    Severity::Info,
                    "Disclosure",
                    "robots.txt is published",
                    "The root robots.txt resource returned HTTP 200.",
                    "Review publicly listed crawl rules and referenced locations.",
                );
            } else if status == 200 && probe.path.ends_with("sitemap.xml") {
                add_finding(
                    findings,
                    Severity::Info,
                    "Disclosure",
                    "sitemap.xml is published",
                    "The root sitemap.xml resource returned HTTP 200.",
                    "Review the public URL inventory and compare it with the intended application surface.",
                );
            }
        }
    }

    if url.scheme() == "https" {
        add_finding(
            findings,
            Severity::Info,
            "Transport security",
            "HTTPS endpoint observed",
            "The supplied target uses HTTPS. Certificate-chain details are not inspected by this first recon module.",
            "Review certificate validity, hostname coverage, chain and supported protocol versions separately.",
        );
    }
}

fn render_report(ui: &mut egui::Ui, report: &ReconReport) {
    ui.add_space(14.0);
    ui.heading("Recon summary");

    ui.columns(3, |columns| {
        metric(&mut columns[0], "PROBES COMPLETED", report.probes.len().to_string());
        metric(&mut columns[1], "FINDINGS", report.findings.len().to_string());
        metric(&mut columns[2], "RESOLVED ADDRESSES", report.resolved_addresses.len().to_string());
    });

    ui.add_space(8.0);
    ui.label(format!("Target: {}", report.target));
    ui.label(format!("Host: {}", report.host));
    ui.label(format!(
        "DNS: {}",
        if report.resolved_addresses.is_empty() {
            "No address returned".into()
        } else {
            report.resolved_addresses.join(", ")
        }
    ));
    ui.label(format!("Generated: {}", report.generated_at.to_rfc3339()));

    ui.add_space(12.0);
    ui.heading("Probe results");
    egui::Grid::new("recon_probe_table")
        .striped(true)
        .show(ui, |ui| {
            for heading in ["Method", "Path", "Status", "Time", "Bytes", "Detail"] {
                ui.strong(heading);
            }
            ui.end_row();
            for probe in &report.probes {
                ui.label(&probe.method);
                ui.label(&probe.path);
                let status_text = probe
                    .status
                    .map(|status| status.to_string())
                    .unwrap_or_else(|| "ERR".into());
                let color = match probe.status {
                    Some(status) if (200..300).contains(&status) => egui::Color32::LIGHT_GREEN,
                    Some(status) if (300..400).contains(&status) => egui::Color32::LIGHT_BLUE,
                    Some(status) if status >= 400 => egui::Color32::YELLOW,
                    _ => egui::Color32::LIGHT_RED,
                };
                ui.colored_label(color, status_text);
                ui.label(format!("{:.1} ms", probe.elapsed_ms));
                ui.label(probe.bytes_read.to_string());
                ui.label(&probe.note);
                ui.end_row();
            }
        });

    ui.add_space(12.0);
    ui.heading("Findings");
    if report.findings.is_empty() {
        ui.label("No findings were generated by the checks in this profile.");
    } else {
        for finding in &report.findings {
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(22, 29, 41))
                .stroke(egui::Stroke::new(1.0, finding.severity.color()))
                .corner_radius(8)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(finding.severity.color(), finding.severity.label());
                        ui.strong(&finding.title);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(&finding.category)
                                    .small()
                                    .color(egui::Color32::from_rgb(137, 157, 181)),
                            );
                        });
                    });
                    ui.label(&finding.evidence);
                    ui.label(
                        egui::RichText::new(format!("Recommendation: {}", finding.recommendation))
                            .small()
                            .color(egui::Color32::from_rgb(160, 180, 202)),
                    );
                });
        }
    }
}

fn metric(ui: &mut egui::Ui, title: &str, value: String) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(25, 34, 48))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 65, 86)))
        .corner_radius(10)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .small()
                    .color(egui::Color32::from_rgb(145, 163, 185)),
            );
            ui.heading(value);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn url() -> Url {
        Url::parse("https://test.example/").expect("test URL")
    }

    #[test]
    fn rejects_non_http_targets_and_credentials() {
        assert!(validate_target("file:///etc/passwd").is_err());
        assert!(validate_target("https://user:pass@example.com").is_err());
        assert!(validate_target("https://example.com").is_ok());
    }

    #[test]
    fn detects_missing_browser_security_headers() {
        let headers = HeaderMap::new();
        let mut findings = Vec::new();
        analyze_headers(&url(), &headers, "", &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("HSTS")));
        assert!(findings.iter().any(|f| f.title.contains("Content Security Policy")));
        assert!(findings.iter().any(|f| f.title.contains("MIME sniffing")));
        assert!(findings.iter().any(|f| f.title.contains("Framing protection")));
    }

    #[test]
    fn detects_wildcard_cors_with_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
        headers.insert("access-control-allow-credentials", HeaderValue::from_static("true"));
        let mut findings = Vec::new();
        analyze_headers(&url(), &headers, "", &mut findings);
        assert!(findings.iter().any(|f| f.category == "CORS" && f.severity == Severity::Medium));
    }

    #[test]
    fn detects_sensitive_methods_from_allow_header_without_invoking_them() {
        let mut headers = HeaderMap::new();
        headers.insert(ALLOW, HeaderValue::from_static("GET, HEAD, TRACE, PUT"));
        let mut findings = Vec::new();
        analyze_allowed_methods(&headers, &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("Sensitive HTTP methods")));
    }

    #[test]
    fn finds_cookie_flag_gaps() {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, HeaderValue::from_static("sessionid=abc; Path=/"));
        let mut findings = Vec::new();
        analyze_headers(&url(), &headers, "", &mut findings);
        assert_eq!(
            findings.iter().filter(|finding| finding.title.contains("Cookie missing")).count(),
            3
        );
    }

    #[test]
    fn records_no_hsts_finding_for_plain_http() {
        let http = Url::parse("http://test.example/").expect("http URL");
        let headers = HeaderMap::new();
        let mut findings = Vec::new();
        analyze_headers(&http, &headers, "", &mut findings);
        assert!(!findings.iter().any(|f| f.title.contains("HSTS")));
    }

    #[test]
    fn technology_fingerprint_is_informational_only() {
        let headers = HeaderMap::new();
        let mut findings = Vec::new();
        analyze_headers(&url(), &headers, "<link href='/wp-content/themes/demo.css'>", &mut findings);
        assert!(findings.iter().any(|f| f.title.contains("Possible web technology")));
        assert!(findings.iter().all(|f| f.severity != Severity::High && f.severity != Severity::Critical));
    }
}
