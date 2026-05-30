//! Embedding clients.
//!
//! Phase 1A ships two implementations:
//!   - `MockEmbedder` — deterministic hash-based embeddings.  No network, no
//!     dependencies; used by tests, CI, and offline demos.  Cosine similarity
//!     between two MockEmbedder outputs is meaningful enough to validate the
//!     end-to-end ingestion + retrieval pipeline.
//!   - `OpenAiEmbedder` — `text-embedding-3-large` (or any
//!     OpenAI-compatible endpoint) via HTTP.  Used by production deployments.
//!
//! The `EmbeddingClient` trait is the single abstraction the pipeline depends
//! on, so additional backends (voyage-3, bge-m3 local, etc.) plug in without
//! rewiring callers.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("http: {0}")]
    Http(String),
    #[error("api: {0}")]
    Api(String),
    #[error("malformed response: {0}")]
    Malformed(String),
}

#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    fn dim(&self) -> usize;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}

// ---------------------------------------------------------------------------
// Mock embedder
// ---------------------------------------------------------------------------

/// Deterministic hash-based embedder.  Produces token-overlap-sensitive
/// vectors: two texts that share many words score higher than two that share
/// few.  Not semantically aware, but good enough for end-to-end pipeline
/// validation.
pub struct MockEmbedder {
    dim: usize,
}

impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new(256)
    }
}

#[async_trait]
impl EmbeddingClient for MockEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts
            .iter()
            .map(|t| token_bag_vector(t, self.dim))
            .collect())
    }
}

/// Token-bag embedding: lowercases, splits on non-alphanumeric, hashes each
/// token to a bucket, accumulates counts, normalises.  Result: cosine
/// similarity ≈ overlap of token vocabularies.
fn token_bag_vector(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    for token in tokenise(text) {
        let mut h = Sha256::new();
        h.update(token.as_bytes());
        let bytes = h.finalize();
        let bucket = (u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize) % dim;
        v[bucket] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    for x in &mut v {
        *x /= norm;
    }
    v
}

fn tokenise(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_lowercase())
}

// ---------------------------------------------------------------------------
// OpenAI / OpenAI-compatible embedder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct OpenAiEmbedder {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    dim: usize,
}

impl OpenAiEmbedder {
    /// `base_url` is the API root (e.g. https://api.openai.com/v1).  `dim`
    /// must match the model's output dimension; for text-embedding-3-large
    /// this is 3072, for -3-small it is 1536.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        dim: usize,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key: api_key.into(),
            dim,
        }
    }

    /// Opt-in via env: returns `None` unless `ORACLE_AUTOMATE_EMBEDDINGS_BASE_URL`
    /// is set, so the offline default keeps using `MockEmbedder`.  Defaults
    /// match `text-embedding-3-small` (1536 dims).
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("ORACLE_AUTOMATE_EMBEDDINGS_BASE_URL").ok()?;
        let api_key = std::env::var("ORACLE_AUTOMATE_EMBEDDINGS_API_KEY").unwrap_or_default();
        let model = std::env::var("ORACLE_AUTOMATE_EMBEDDINGS_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".to_string());
        let dim = std::env::var("ORACLE_AUTOMATE_EMBEDDINGS_DIM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1536);
        Some(Self::new(base_url, api_key, model, dim))
    }
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    input: &'a [String],
    model: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

#[async_trait]
impl EmbeddingClient for OpenAiEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let url = format!("{}/embeddings", self.base_url);
        let body = EmbedRequest {
            input: texts,
            model: &self.model,
        };
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbeddingError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            let s = resp.text().await.unwrap_or_default();
            return Err(EmbeddingError::Api(s));
        }
        let parsed: EmbedResponse = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::Malformed(e.to_string()))?;
        debug!(count = parsed.data.len(), "openai embed ok");
        let mut out: Vec<Vec<f32>> = parsed.data.into_iter().map(|d| d.embedding).collect();
        for v in &mut out {
            if v.len() != self.dim {
                return Err(EmbeddingError::Malformed(format!(
                    "expected dim {}, got {}",
                    self.dim,
                    v.len(),
                )));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_embedder_similarity() {
        let e = MockEmbedder::new(128);
        let v = e
            .embed(&[
                "period close in Oracle General Ledger".into(),
                "period-end close FI module".into(),
                "receiving inventory transaction".into(),
            ])
            .await
            .unwrap();
        let cos = |a: &Vec<f32>, b: &Vec<f32>| -> f32 {
            a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>()
        };
        // First two share more vocabulary than first and third.
        assert!(
            cos(&v[0], &v[1]) > cos(&v[0], &v[2]),
            "expected nearer cosine"
        );
    }

    #[tokio::test]
    async fn mock_embedder_dim_consistent() {
        let e = MockEmbedder::new(64);
        let v = e.embed(&["hello world".into()]).await.unwrap();
        assert_eq!(v[0].len(), 64);
        // Normalised.
        let norm: f32 = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "norm was {norm}");
    }

    // --- OpenAiEmbedder contract tests against an in-process axum mock of the
    // OpenAI-compatible `/embeddings` endpoint (same reqwest path, no network).

    async fn spawn_embeddings_mock(dim: usize) -> std::net::SocketAddr {
        use axum::{routing::post, Json, Router};
        let app = Router::new().route(
            "/v1/embeddings",
            post(move |body: Json<serde_json::Value>| async move {
                // Echo one embedding per input, each `dim` long.
                let n = body
                    .0
                    .get("input")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let data: Vec<serde_json::Value> = (0..n)
                    .map(|i| {
                        serde_json::json!({
                            "embedding": vec![0.1f32; dim],
                            "index": i,
                        })
                    })
                    .collect();
                Json(serde_json::json!({ "data": data }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn openai_embedder_parses_response_shape() {
        let addr = spawn_embeddings_mock(4).await;
        let e = OpenAiEmbedder::new(format!("http://{addr}/v1"), "test-key", "model", 4);
        let v = e
            .embed(&["period close".into(), "journal import".into()])
            .await
            .unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 4);
    }

    #[tokio::test]
    async fn openai_embedder_rejects_dim_mismatch() {
        // Server returns 8-dim vectors; client expects 4 → Malformed.
        let addr = spawn_embeddings_mock(8).await;
        let e = OpenAiEmbedder::new(format!("http://{addr}/v1"), "test-key", "model", 4);
        let err = e.embed(&["x".into()]).await.unwrap_err();
        assert!(matches!(err, EmbeddingError::Malformed(_)));
    }
}
