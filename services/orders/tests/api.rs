use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use domain::events::{Event, OrderCreated};
use http_body_util::BodyExt;
use orders::app::{router, AppState};
use orders::catalog_client::{CatalogClient, CatalogResilience};
use resilience::RetryPolicy;
use serde_json::{json, Value};
use shared::{AmqpConfig, AppConfig, EventBus};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

type FakeProduct = (Uuid, i64, bool);

fn product_json((id, price, active): FakeProduct) -> Value {
    json!({
        "id": id,
        "seller_id": Uuid::new_v4(),
        "name": "Produto",
        "description": null,
        "price": price,
        "active": active,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    })
}

async fn serve(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn fake_catalog(products: Vec<FakeProduct>) -> String {
    async fn get_product(
        State(products): State<Arc<Vec<FakeProduct>>>,
        Path(id): Path<Uuid>,
    ) -> Result<Json<Value>, StatusCode> {
        products
            .iter()
            .find(|(pid, _, _)| *pid == id)
            .map(|product| Json(product_json(*product)))
            .ok_or(StatusCode::NOT_FOUND)
    }

    serve(
        Router::new()
            .route("/products/{id}", get(get_product))
            .with_state(Arc::new(products)),
    )
    .await
}

#[derive(Clone)]
struct Scripted {
    product: FakeProduct,
    statuses: Arc<Vec<StatusCode>>,
    delay: Duration,
    hits: Arc<AtomicUsize>,
}

async fn scripted_catalog(
    product: FakeProduct,
    statuses: Vec<StatusCode>,
    delay: Duration,
) -> (String, Arc<AtomicUsize>) {
    async fn get_product(State(script): State<Scripted>) -> Response {
        let hit = script.hits.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(script.delay).await;
        match script.statuses[hit.min(script.statuses.len() - 1)] {
            StatusCode::OK => Json(product_json(script.product)).into_response(),
            status => status.into_response(),
        }
    }

    let hits = Arc::new(AtomicUsize::new(0));
    let url = serve(
        Router::new()
            .route("/products/{id}", get(get_product))
            .with_state(Scripted {
                product,
                statuses: Arc::new(statuses),
                delay,
                hits: hits.clone(),
            }),
    )
    .await;
    (url, hits)
}

fn fast_resilience() -> CatalogResilience {
    CatalogResilience {
        timeout: Duration::from_millis(300),
        retry: RetryPolicy::new(3, Duration::from_millis(10), Duration::from_millis(50)),
        breaker_failure_threshold: 5,
        breaker_open_timeout: Duration::from_secs(30),
        cache_ttl: Duration::from_secs(60),
        cache_max_entries: 100,
    }
}

fn no_retry() -> RetryPolicy {
    RetryPolicy::new(1, Duration::ZERO, Duration::ZERO)
}

async fn event_bus() -> EventBus {
    let config = AmqpConfig::from_env().expect("AMQP_URL ausente (suba o docker-compose)");
    EventBus::connect(&config)
        .await
        .expect("broker inacessível")
}

async fn app(pool: PgPool, catalog_url: &str) -> Router {
    app_with(pool, catalog_url, fast_resilience()).await
}

async fn app_with(pool: PgPool, catalog_url: &str, resilience: CatalogResilience) -> Router {
    let config = AppConfig::from_source(&HashMap::new(), "orders").unwrap();
    router(
        AppState::new(
            config,
            pool,
            CatalogClient::new(catalog_url, resilience),
            event_bus().await,
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

async fn create_order(app: &Router, items: Value) -> (StatusCode, Value) {
    send(
        app,
        Method::POST,
        "/orders",
        Some(json!({ "buyer_id": Uuid::new_v4(), "items": items })),
    )
    .await
}

async fn metrics_text(app: &Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn has_series(metrics: &str, name: &str, labels: &[&str]) -> bool {
    metrics
        .lines()
        .any(|line| line.starts_with(name) && labels.iter().all(|label| line.contains(label)))
}

#[sqlx::test]
async fn cria_pedido_com_precos_do_catalogo(pool: PgPool) {
    let (teclado, mouse) = (Uuid::new_v4(), Uuid::new_v4());
    let catalog = fake_catalog(vec![(teclado, 1_000, true), (mouse, 500, true)]).await;
    let app = app(pool, &catalog).await;

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
    let app = app(pool, &catalog).await;

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
    let app = app(pool, "http://127.0.0.1:1").await;

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
    let app = app(pool, &catalog).await;

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
async fn publica_order_created_ao_criar_pedido(pool: PgPool) {
    let product = Uuid::new_v4();
    let catalog = fake_catalog(vec![(product, 1_000, true)]).await;
    let app = app(pool, &catalog).await;

    let bus = event_bus().await;
    let service = format!("test-orders-{}", Uuid::new_v4());
    let (tx, mut rx) = tokio::sync::mpsc::channel::<OrderCreated>(8);
    let consumer = bus
        .spawn_consumer::<OrderCreated, _, _>(&service, move |event| {
            let tx = tx.clone();
            async move {
                tx.send(event).await.unwrap();
                Ok(())
            }
        })
        .await
        .unwrap();

    let (status, order) =
        create_order(&app, json!([{ "product_id": product, "quantity": 3 }])).await;
    assert_eq!(status, StatusCode::CREATED, "{order}");
    let order_id = Uuid::parse_str(order["id"].as_str().unwrap()).unwrap();

    let event = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("order.created não chegou em 5s")
            .unwrap();
        if event.order_id == order_id {
            break event;
        }
    };

    assert_eq!(event.total.cents(), 3_000);
    assert_eq!(event.items.len(), 1);
    assert_eq!(event.items[0].product_id, product);
    assert_eq!(event.items[0].quantity, 3);

    consumer.abort();
    let amqp = AmqpConfig::from_env().unwrap();
    let conn = lapin::Connection::connect(&amqp.url, lapin::ConnectionProperties::default())
        .await
        .unwrap();
    let channel = conn.create_channel().await.unwrap();
    let queue = format!("{service}.{}", OrderCreated::ROUTING_KEY);
    for name in [
        queue.clone(),
        format!("{queue}.retry"),
        format!("{queue}.dead"),
    ] {
        channel
            .queue_delete(&name, lapin::options::QueueDeleteOptions::default())
            .await
            .unwrap();
    }
}

#[sqlx::test]
async fn pedido_inexistente_da_404(pool: PgPool) {
    let app = app(pool, "http://127.0.0.1:1").await;
    let id = Uuid::new_v4();

    let (status, _) = send(&app, Method::GET, &format!("/orders/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn retry_recupera_de_falha_transitoria_do_catalogo(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, hits) = scripted_catalog(
        (product, 1_000, true),
        vec![StatusCode::SERVICE_UNAVAILABLE, StatusCode::OK],
        Duration::ZERO,
    )
    .await;
    let app = app(pool, &catalog).await;

    let (status, order) =
        create_order(&app, json!([{ "product_id": product, "quantity": 1 }])).await;

    assert_eq!(status, StatusCode::CREATED, "{order}");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[sqlx::test]
async fn nao_repete_a_chamada_quando_o_produto_nao_existe(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, hits) = scripted_catalog(
        (product, 1_000, true),
        vec![StatusCode::NOT_FOUND],
        Duration::ZERO,
    )
    .await;
    let app = app(pool, &catalog).await;

    let (status, _) = create_order(&app, json!([{ "product_id": product, "quantity": 1 }])).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[sqlx::test]
async fn timeout_do_catalogo_da_503_sem_esperar_a_resposta(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, _) = scripted_catalog(
        (product, 1_000, true),
        vec![StatusCode::OK],
        Duration::from_secs(2),
    )
    .await;
    let resilience = CatalogResilience {
        retry: no_retry(),
        ..fast_resilience()
    };
    let app = app_with(pool, &catalog, resilience).await;

    let started = Instant::now();
    let (status, body) =
        create_order(&app, json!([{ "product_id": product, "quantity": 1 }])).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "esperou o catálogo em vez de respeitar o timeout"
    );
}

#[sqlx::test]
async fn circuito_abre_falha_rapido_e_fecha_quando_o_catalogo_volta(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, hits) = scripted_catalog(
        (product, 1_000, true),
        vec![
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::OK,
        ],
        Duration::ZERO,
    )
    .await;
    let resilience = CatalogResilience {
        retry: no_retry(),
        breaker_failure_threshold: 2,
        breaker_open_timeout: Duration::from_secs(1),
        ..fast_resilience()
    };
    let app = app_with(pool, &catalog, resilience).await;
    let items = json!([{ "product_id": product, "quantity": 1 }]);

    for _ in 0..2 {
        let (status, _) = create_order(&app, items.clone()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let (status, body) = create_order(&app, items.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        body["error"].as_str().unwrap().contains("circuito aberto"),
        "{body}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        2,
        "chamou o catálogo com o circuito aberto"
    );

    tokio::time::sleep(Duration::from_millis(1_200)).await;

    let (status, order) = create_order(&app, items).await;
    assert_eq!(status, StatusCode::CREATED, "{order}");
    assert_eq!(hits.load(Ordering::SeqCst), 3);

    let metrics = metrics_text(&app).await;
    for to in ["open", "half_open", "closed"] {
        assert!(
            has_series(
                &metrics,
                "circuit_breaker_transitions_total",
                &["breaker=\"catalog\"", &format!("to=\"{to}\"")],
            ),
            "transição para {to} não exposta"
        );
    }
    assert!(has_series(
        &metrics,
        "circuit_breaker_state",
        &["breaker=\"catalog\""]
    ));
}

#[sqlx::test]
async fn fallback_usa_o_produto_em_cache_quando_o_catalogo_falha(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, hits) = scripted_catalog(
        (product, 1_000, true),
        vec![StatusCode::OK, StatusCode::INTERNAL_SERVER_ERROR],
        Duration::ZERO,
    )
    .await;
    let resilience = CatalogResilience {
        retry: no_retry(),
        ..fast_resilience()
    };
    let app = app_with(pool, &catalog, resilience).await;
    let items = json!([{ "product_id": product, "quantity": 2 }]);

    let (status, _) = create_order(&app, items.clone()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, order) = create_order(&app, items).await;
    assert_eq!(status, StatusCode::CREATED, "{order}");
    assert_eq!(order["total"], 2_000);
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let metrics = metrics_text(&app).await;
    assert!(has_series(
        &metrics,
        "fallback_activations_total",
        &["source=\"catalog\""]
    ));
}

#[sqlx::test]
async fn latencia_http_e_exposta_como_histograma_com_buckets(pool: PgPool) {
    let app = app(pool, "http://127.0.0.1:1").await;
    send(
        &app,
        Method::GET,
        "/orders/00000000-0000-0000-0000-000000000000",
        None,
    )
    .await;

    let metrics = metrics_text(&app).await;

    assert!(
        has_series(
            &metrics,
            "http_request_duration_seconds_bucket",
            &["path=\"/orders/{id}\"", "le=\""]
        ),
        "latência exposta sem buckets (summary?):\n{metrics}"
    );
}

#[sqlx::test]
async fn fallback_atende_com_o_circuito_aberto_sem_chamar_o_catalogo(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, hits) = scripted_catalog(
        (product, 1_000, true),
        vec![StatusCode::OK, StatusCode::INTERNAL_SERVER_ERROR],
        Duration::ZERO,
    )
    .await;
    let resilience = CatalogResilience {
        retry: no_retry(),
        breaker_failure_threshold: 1,
        ..fast_resilience()
    };
    let app = app_with(pool, &catalog, resilience).await;
    let items = json!([{ "product_id": product, "quantity": 1 }]);

    for _ in 0..3 {
        let (status, order) = create_order(&app, items.clone()).await;
        assert_eq!(status, StatusCode::CREATED, "{order}");
    }

    assert_eq!(
        hits.load(Ordering::SeqCst),
        2,
        "com o circuito aberto o pedido deveria sair do cache"
    );
}

#[sqlx::test]
async fn produto_removido_do_catalogo_nao_volta_pelo_cache(pool: PgPool) {
    let product = Uuid::new_v4();
    let (catalog, _) = scripted_catalog(
        (product, 1_000, true),
        vec![
            StatusCode::OK,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
        ],
        Duration::ZERO,
    )
    .await;
    let resilience = CatalogResilience {
        retry: no_retry(),
        ..fast_resilience()
    };
    let app = app_with(pool, &catalog, resilience).await;
    let items = json!([{ "product_id": product, "quantity": 1 }]);

    let (status, _) = create_order(&app, items.clone()).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = create_order(&app, items.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = create_order(&app, items).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
}
