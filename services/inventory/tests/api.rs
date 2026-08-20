use std::collections::HashMap;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use inventory::app::{router, AppState};
use serde_json::{json, Value};
use shared::AppConfig;
use sqlx::PgPool;
use tokio::task::JoinSet;
use tower::ServiceExt;
use uuid::Uuid;

fn app(pool: PgPool) -> Router {
    let config = AppConfig::from_source(&HashMap::new(), "inventory").unwrap();
    router(AppState::new(config, pool))
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

async fn load(app: &Router, product_id: Uuid, quantity: u32) -> Value {
    let (status, body) = send(
        app,
        Method::PUT,
        &format!("/stock/{product_id}"),
        Some(json!({ "quantity": quantity })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn op(app: &Router, product_id: Uuid, op: &str, quantity: u32) -> (StatusCode, Value) {
    send(
        app,
        Method::POST,
        &format!("/stock/{product_id}/{op}"),
        Some(json!({ "quantity": quantity })),
    )
    .await
}

#[sqlx::test]
async fn carga_reserva_liberacao_e_commit(pool: PgPool) {
    let app = app(pool);
    let product_id = Uuid::new_v4();

    let loaded = load(&app, product_id, 10).await;
    assert_eq!(loaded["available"], 10);
    assert_eq!(loaded["reserved"], 0);

    let (status, body) = op(&app, product_id, "reserve", 3).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["available"].as_u64(), body["reserved"].as_u64()),
        (Some(7), Some(3))
    );

    let (status, body) = op(&app, product_id, "release", 1).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["available"].as_u64(), body["reserved"].as_u64()),
        (Some(8), Some(2))
    );

    let (status, body) = op(&app, product_id, "commit", 2).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["available"].as_u64(), body["reserved"].as_u64()),
        (Some(8), Some(0))
    );

    let (status, found) = send(&app, Method::GET, &format!("/stock/{product_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found, body);
}

#[sqlx::test]
async fn estoque_insuficiente_da_409_sem_alterar_saldo(pool: PgPool) {
    let app = app(pool);
    let product_id = Uuid::new_v4();
    load(&app, product_id, 2).await;

    let (status, body) = op(&app, product_id, "reserve", 5).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("estoque insuficiente"));

    let (_, found) = send(&app, Method::GET, &format!("/stock/{product_id}"), None).await;
    assert_eq!(found["available"], 2);
}

#[sqlx::test]
async fn liberar_mais_que_o_reservado_da_409(pool: PgPool) {
    let app = app(pool);
    let product_id = Uuid::new_v4();
    load(&app, product_id, 5).await;
    op(&app, product_id, "reserve", 1).await;

    let (status, _) = op(&app, product_id, "release", 2).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn quantidade_zero_da_422(pool: PgPool) {
    let app = app(pool);
    let product_id = Uuid::new_v4();
    load(&app, product_id, 5).await;

    let (status, _) = op(&app, product_id, "reserve", 0).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn produto_sem_estoque_da_404(pool: PgPool) {
    let app = app(pool);
    let product_id = Uuid::new_v4();

    let (status, _) = send(&app, Method::GET, &format!("/stock/{product_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = op(&app, product_id, "reserve", 1).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn reservas_concorrentes_nao_ultrapassam_o_saldo(pool: PgPool) {
    let app = app(pool);
    let product_id = Uuid::new_v4();
    load(&app, product_id, 5).await;

    let mut tasks = JoinSet::new();
    for _ in 0..20 {
        let app = app.clone();
        tasks.spawn(async move { op(&app, product_id, "reserve", 1).await.0 });
    }

    let mut ok = 0;
    let mut conflict = 0;
    while let Some(status) = tasks.join_next().await {
        match status.unwrap() {
            StatusCode::OK => ok += 1,
            StatusCode::CONFLICT => conflict += 1,
            other => panic!("status inesperado: {other}"),
        }
    }

    assert_eq!((ok, conflict), (5, 15));
    let (_, found) = send(&app, Method::GET, &format!("/stock/{product_id}"), None).await;
    assert_eq!(found["available"], 0);
    assert_eq!(found["reserved"], 5);
}
