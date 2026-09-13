use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

const LATENCY_BUCKETS: [f64; 12] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

const REPROCESSING_BUCKETS: [f64; 10] = [0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0];

pub fn init() -> PrometheusHandle {
    HANDLE
        .get_or_init(|| {
            PrometheusBuilder::new()
                .set_buckets(&LATENCY_BUCKETS)
                .and_then(|builder| {
                    builder.set_buckets_for_metric(
                        Matcher::Full("failure_reprocessing_duration_seconds".to_owned()),
                        &REPROCESSING_BUCKETS,
                    )
                })
                .expect("buckets de histograma válidos")
                .install_recorder()
                .expect("falha ao instalar o recorder do Prometheus")
        })
        .clone()
}

pub async fn render(handle: PrometheusHandle) -> impl IntoResponse {
    handle.render()
}

pub async fn track_http(matched_path: Option<MatchedPath>, req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = matched_path
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());

    let start = Instant::now();
    let response = next.run(req).await;
    let elapsed = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "path" => path,
        "status" => status,
    )
    .record(elapsed);

    response
}
