use reqwest::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogProduct {
    pub id: Uuid,
    pub price: i64,
    pub active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catálogo indisponível: {0}")]
    Unavailable(#[from] reqwest::Error),
    #[error("resposta inesperada do catálogo: {0}")]
    Unexpected(StatusCode),
}

#[derive(Clone)]
pub struct CatalogClient {
    base_url: String,
    http: reqwest::Client,
}

impl CatalogClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn get_product(&self, id: Uuid) -> Result<Option<CatalogProduct>, CatalogError> {
        let response = self
            .http
            .get(format!("{}/products/{id}", self.base_url))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => Ok(Some(response.json().await?)),
            StatusCode::NOT_FOUND => Ok(None),
            other => Err(CatalogError::Unexpected(other)),
        }
    }
}
