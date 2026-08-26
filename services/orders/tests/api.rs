use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use http_body_util::BodyExt;
use orders::app::{router, AppState};
use orders::catalog_client::CatalogClient;
use serde_json::{json, Value};
use shared::AppConfig;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

type FakeProduct = (Uuid, i64, bool);

async fn fake_catalog(products: Vec<FakeProduct>) -> String {
    async fn get_product(
        State(products): State<Arc<Vec<FakeProduct>>>,
        Path(id): Path<Uuid>,
    ) -> Result<Json<Value>, StatusCode> {
        products
            .iter()
            .find(|(pid, _, _)| *pid == id)
            .map(|(pid, price, active)| {
                Json(json!({
                    "id": pid,
                    "seller_id": Uuid::new_v4(),
                    "name": "Produto",
                    "description": null,
                    "price": price,
                    "active": active,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                }))
            })
            .ok_or(StatusCode::NOT_FOUND)
    }

    let app = Router::new()
        .route("/products/{id}", get(get_product))
        .with_state(Arc::new(products));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn app(pool: PgPool, catalog_url: &str) -> Router {
    let config = AppConfig::from_source(&HashMap::new(), "orders").unwrap();
    router(AppState::new(config, pool, CatalogClient::new(catalog_url)))
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

async fn create_order(app: &Router, items: Value) -> (StatusCode, Value) {
    send(
        app,
        Method::POST,
        "/orders",
        Some(json!({ "buyer_id": Uuid::new_v4(), "items": items })),
    )
    .await
}

#[sqlx::test]
async fn cria_pedido_com_precos_do_catalogo(pool: PgPool) {
    let (teclado, mouse) = (Uuid::new_v4(), Uuid::new_v4());
    let catalog = fake_catalog(vec![(teclado, 1_000, true), (mouse, 500, true)]).await;
    let app = app(pool, &catalog);

    let (status, order) = create_order(
        &app,
        json!([
            { "product_id": teclado, "quantity": 2 },
            { "product_id": mouse, "quantity": 1 }
        ]),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{order}");
    assert_eq!(order["status"], "PENDING");
    assert_eq!(order["total"], 2_500);
    let items = order["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let teclado_item = items
        .iter()
        .find(|i| i["product_id"] == json!(teclado))
        .unwrap();
    assert_eq!(teclado_item["unit_price"], 1_000);
    assert_eq!(teclado_item["quantity"], 2);

    let id = order["id"].as_str().unwrap();
    let (status, found) = send(&app, Method::GET, &format!("/orders/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["total"], 2_500);
    assert_eq!(found["items"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn produto_inexistente_ou_inativo_da_422(pool: PgPool) {
    let inativo = Uuid::new_v4();
    let catalog = fake_catalog(vec![(inativo, 100, false)]).await;
    let app = app(pool, &catalog);

    let (status, body) = create_order(
        &app,
        json!([{ "product_id": Uuid::new_v4(), "quantity": 1 }]),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("não encontrado"));

    let (status, body) =
        create_order(&app, json!([{ "product_id": inativo, "quantity": 1 }])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("inativo"));
}

#[sqlx::test]
async fn catalogo_fora_do_ar_da_503(pool: PgPool) {
    let app = app(pool, "http://127.0.0.1:1");

    let (status, body) = create_order(
        &app,
        json!([{ "product_id": Uuid::new_v4(), "quantity": 1 }]),
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("catálogo indisponível"));
}

#[sqlx::test]
async fn pedido_invalido_da_422(pool: PgPool) {
    let product = Uuid::new_v4();
    let catalog = fake_catalog(vec![(product, 100, true)]).await;
    let app = app(pool, &catalog);

    let (status, _) = create_order(&app, json!([])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = create_order(&app, json!([{ "product_id": product, "quantity": 0 }])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = create_order(
        &app,
        json!([
            { "product_id": product, "quantity": 1 },
            { "product_id": product, "quantity": 1 }
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("mais de uma vez"));
}

#[sqlx::test]
async fn pedido_inexistente_da_404(pool: PgPool) {
    let app = app(pool, "http://127.0.0.1:1");
    let id = Uuid::new_v4();

    let (status, _) = send(&app, Method::GET, &format!("/orders/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
