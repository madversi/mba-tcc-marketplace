use std::collections::HashMap;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use payments::app::{router, AppState};
use payments::gateway::SimulatedGateway;
use serde_json::{json, Value};
use shared::AppConfig;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn app(pool: PgPool) -> Router {
    let config = AppConfig::from_source(&HashMap::new(), "payments").unwrap();
    router(AppState::new(config, pool, SimulatedGateway))
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
