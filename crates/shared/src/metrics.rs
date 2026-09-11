use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

pub fn init() -> PrometheusHandle {
    HANDLE
        .get_or_init(|| {
            PrometheusBuilder::new()
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
