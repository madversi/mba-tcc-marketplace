use std::collections::HashMap;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use catalog::app::{router, AppState};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use shared::AppConfig;
use sqlx::PgPool;
use tower::ServiceExt;

fn app(pool: PgPool) -> Router {
    let config = AppConfig::from_source(&HashMap::new(), "catalog").unwrap();
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

async fn create_seller(app: &Router) -> Value {
    let email = format!("{}@loja.com", uuid::Uuid::new_v4());
    let (status, body) = send(
        app,
        Method::POST,
        "/sellers",
        Some(json!({ "name": "Loja da Ana", "email": email })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

#[sqlx::test]
async fn cria_busca_e_lista_vendedores(pool: PgPool) {
    let app = app(pool);

    let created = create_seller(&app).await;
    let id = created["id"].as_str().unwrap();
    assert_eq!(created["name"], "Loja da Ana");

    let (status, found) = send(&app, Method::GET, &format!("/sellers/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found, created);

    let (status, list) = send(&app, Method::GET, "/sellers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn vendedor_com_email_duplicado_da_409(pool: PgPool) {
    let app = app(pool);
    let first = create_seller(&app).await;

    let (status, body) = send(
        &app,
        Method::POST,
        "/sellers",
        Some(json!({ "name": "Outra", "email": first["email"] })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "email já cadastrado");
}

#[sqlx::test]
async fn vendedor_invalido_da_422(pool: PgPool) {
    let app = app(pool);

    let (status, body) = send(
        &app,
        Method::POST,
        "/sellers",
        Some(json!({ "name": "  ", "email": "a@b.com" })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("nome do vendedor"));
}

#[sqlx::test]
async fn vendedor_inexistente_da_404(pool: PgPool) {
    let app = app(pool);
    let id = uuid::Uuid::new_v4();

    let (status, _) = send(&app, Method::GET, &format!("/sellers/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(&app, Method::GET, &format!("/sellers/{id}/products"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn ciclo_completo_de_produto(pool: PgPool) {
    let app = app(pool);
    let seller = create_seller(&app).await;
    let seller_id = seller["id"].as_str().unwrap();

    let (status, product) = send(
        &app,
        Method::POST,
        "/products",
        Some(json!({
            "seller_id": seller_id,
            "name": "Teclado",
            "description": "Mecânico",
            "price_cents": 25000
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{product}");
    assert_eq!(product["price"], 25000);
    assert_eq!(product["active"], true);
    let id = product["id"].as_str().unwrap();

    let (status, list) = send(
        &app,
        Method::GET,
        &format!("/sellers/{seller_id}/products"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);

    let (status, updated) = send(
        &app,
        Method::PATCH,
        &format!("/products/{id}"),
        Some(json!({ "price_cents": 30000, "name": "Teclado Mecânico" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["price"], 30000);
    assert_eq!(updated["name"], "Teclado Mecânico");
    assert_eq!(updated["description"], "Mecânico");

    let (status, _) = send(&app, Method::DELETE, &format!("/products/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, found) = send(&app, Method::GET, &format!("/products/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["active"], false);
}

#[sqlx::test]
async fn produto_rejeita_entradas_invalidas(pool: PgPool) {
    let app = app(pool);
    let seller = create_seller(&app).await;

    let (status, body) = send(
        &app,
        Method::POST,
        "/products",
        Some(json!({
            "seller_id": uuid::Uuid::new_v4(),
            "name": "Órfão",
            "price_cents": 100
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "vendedor inexistente");

    let (status, _) = send(
        &app,
        Method::POST,
        "/products",
        Some(json!({ "seller_id": seller["id"], "name": "Grátis", "price_cents": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let id = uuid::Uuid::new_v4();
    let (status, _) = send(&app, Method::GET, &format!("/products/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app,
        Method::PATCH,
        &format!("/products/{id}"),
        Some(json!({ "active": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
