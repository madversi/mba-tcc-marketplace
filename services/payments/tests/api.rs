use std::collections::HashMap;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use payments::app::{router, AppState};
use payments::gateway::{GatewayClient, GatewayResilience, SimulatedGateway};
use serde_json::{json, Value};
use shared::AppConfig;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn app(pool: PgPool) -> Router {
    app_with_env(pool, &[])
}

fn app_with_env(pool: PgPool, env: &[(&str, &str)]) -> Router {
    let vars = env
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let config = AppConfig::from_source(&vars, "payments").unwrap();
    router(
        AppState::new(
            config,
            pool,
            GatewayClient::new(
                SimulatedGateway::default(),
                GatewayResilience::from_source(&HashMap::new()).unwrap(),
            ),
        ),
        shared::metrics::init(),
    )
}

async fn send(app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(json) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };

    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, json)
}

async fn pay(app: &Router, order_id: Uuid, amount_cents: i64) -> (StatusCode, Value) {
    send(
        app,
        Method::POST,
        "/payments",
        Some(json!({ "order_id": order_id, "amount_cents": amount_cents })),
    )
    .await
}

#[sqlx::test]
async fn cria_pagamento_aprovado_e_consulta(pool: PgPool) {
    let app = app(pool);
    let order_id = Uuid::new_v4();

    let (status, payment) = pay(&app, order_id, 2_500).await;
    assert_eq!(status, StatusCode::CREATED, "{payment}");
    assert_eq!(payment["status"], "APPROVED");
    assert_eq!(payment["amount"], 2500);
    assert_eq!(payment["failure_reason"], Value::Null);

    let id = payment["id"].as_str().unwrap();
    let (status, by_id) = send(&app, Method::GET, &format!("/payments/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(by_id, payment);

    let (status, by_order) = send(
        &app,
        Method::GET,
        &format!("/payments/order/{order_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(by_order, payment);
}

#[sqlx::test]
async fn segundo_pagamento_do_mesmo_pedido_da_409(pool: PgPool) {
    let app = app(pool);
    let order_id = Uuid::new_v4();
    pay(&app, order_id, 100).await;

    let (status, body) = pay(&app, order_id, 100).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "pedido já possui pagamento");
}

#[sqlx::test]
async fn valor_nao_positivo_da_422(pool: PgPool) {
    let app = app(pool);

    let (status, _) = pay(&app, Uuid::new_v4(), 0).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = pay(&app, Uuid::new_v4(), -10).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn pagamento_inexistente_da_404(pool: PgPool) {
    let app = app(pool);
    let id = Uuid::new_v4();

    let (status, _) = send(&app, Method::GET, &format!("/payments/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(&app, Method::GET, &format!("/payments/order/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn admin_le_e_atualiza_configuracao_do_gateway(pool: PgPool) {
    let app = app(pool);

    let (status, config) = send(&app, Method::GET, "/admin/gateway", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["failure_rate"], 0.0);
    assert_eq!(config["latency_ms"], 0);
    assert_eq!(config["unavailable"], false);

    let (status, updated) = send(
        &app,
        Method::PATCH,
        "/admin/gateway",
        Some(json!({ "unavailable": true, "latency_ms": 50 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["unavailable"], true);
    assert_eq!(updated["latency_ms"], 50);

    let (status, _) = pay(&app, Uuid::new_v4(), 1_000).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[sqlx::test]
async fn admin_rejeita_failure_rate_fora_do_intervalo(pool: PgPool) {
    let app = app(pool);

    let (status, body) = send(
        &app,
        Method::PATCH,
        "/admin/gateway",
        Some(json!({ "failure_rate": 1.5 })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("failure_rate"));
}

#[sqlx::test]
async fn gateway_indisponivel_mantem_pagamento_pending(pool: PgPool) {
    let app = app(pool);
    send(
        &app,
        Method::PATCH,
        "/admin/gateway",
        Some(json!({ "unavailable": true })),
    )
    .await;

    let order_id = Uuid::new_v4();
    let (status, _) = pay(&app, order_id, 1_000).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    let (status, payment) = send(
        &app,
        Method::GET,
        &format!("/payments/order/{order_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payment["status"], "PENDING");
}

#[sqlx::test]
async fn requisicao_que_estoura_o_timeout_do_servidor_da_504_e_entra_nas_metricas(pool: PgPool) {
    let app = app_with_env(pool, &[("HTTP_REQUEST_TIMEOUT_MS", "200")]);
    send(
        &app,
        Method::PATCH,
        "/admin/gateway",
        Some(json!({ "latency_ms": 2_000 })),
    )
    .await;

    let started = std::time::Instant::now();
    let (status, _) = pay(&app, Uuid::new_v4(), 1_000).await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "o servidor esperou o gateway em vez de cortar a requisição"
    );

    let metrics = shared::metrics::init().render();
    assert!(
        metrics.lines().any(|line| {
            line.starts_with("http_request_duration_seconds_count")
                && line.contains("path=\"/payments\"")
                && line.contains("status=\"504\"")
        }),
        "timeout não registrado nas métricas HTTP"
    );
}
