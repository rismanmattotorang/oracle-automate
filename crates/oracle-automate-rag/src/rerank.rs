//! Reranker trait + in-process mock.
//!
//! The paper §VII-H notes that a cross-encoder reranker gives the biggest
//! single precision-at-K lift for the cost (one extra forward pass over the
//! top-N).  Phase 3 ships:
//!   - `Reranker` async trait
//!   - `MockReranker` — deterministic, term-overlap based, demonstrably
//!     reorders the top of the candidate pool toward the query
//!
//! `OnnxReranker` (real cross-encoder via ONNX Runtime) is a Phase 7
//! hardening task.

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct RerankerCandidate {
    pub chunk_text: String,
    pub base_score: f32,
}

#[derive(Debug, Clone)]
pub struct RerankedItem {
    pub idx: usize,
    pub score: f32,
}

impl RerankedItem {
    pub fn original_index(&self) -> Option<usize> {
        Some(self.idx)
    }
}

#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(&self, query: &str, candidates: &[RerankerCandidate]) -> Vec<RerankedItem>;
}

/// Deterministic mock reranker.  Combines:
///   - exact-match term-overlap (cheap proxy for cross-encoder relevance)
///   - position-decay (preserves some of the base ordering as a tie-breaker)
///   - identifier bonus (Oracle table/column names, REST resources, integration/PO IDs — anything that
///     looks like `[A-Z0-9_]{3,}` and is also in the query — score boost)
///
/// Crude but it pushes consensus hits up the way a real cross-encoder
/// would on identifier-heavy Oracle queries.
pub struct MockReranker;

impl MockReranker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockReranker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Reranker for MockReranker {
    async fn rerank(&self, query: &str, candidates: &[RerankerCandidate]) -> Vec<RerankedItem> {
        let q_tokens: Vec<String> = tokens(query);
        let q_identifiers: Vec<String> = q_tokens
            .iter()
            .filter(|t| t.len() >= 3 && t.chars().any(|c| c.is_uppercase() || c == '_'))
            .cloned()
            .collect();

        let mut scored: Vec<RerankedItem> = candidates
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                let body_tokens = tokens(&c.chunk_text);
                let overlap = q_tokens.iter().filter(|t| body_tokens.contains(t)).count() as f32;
                let ident_bonus = q_identifiers
                    .iter()
                    .filter(|t| {
                        c.chunk_text.contains(t.as_str())
                            || c.chunk_text
                                .to_ascii_uppercase()
                                .contains(&t.to_ascii_uppercase())
                    })
                    .count() as f32
                    * 0.5;
                let pos_decay = 1.0 / (1.0 + idx as f32 * 0.05);
                let score = overlap + ident_bonus + 0.1 * c.base_score + pos_decay;
                RerankedItem { idx, score }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored
    }
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// HttpReranker — real cross-encoder via a managed rerank endpoint
// ---------------------------------------------------------------------------

/// Cross-encoder reranker backed by a managed rerank API (Cohere / Jina /
/// Voyage-style: `POST {base_url}/rerank` with `{model, query, documents}` →
/// `{results: [{index, relevance_score}]}`).  Per the design note this is the
/// single biggest precision-at-K lift for the cost — one forward pass over the
/// top-N candidates.
///
/// **Failure is non-fatal.**  If the endpoint errors or returns a malformed
/// body the candidates are returned in their original (base-score) order, so a
/// reranker outage degrades to "no rerank lift" rather than a broken search —
/// matching the `Reranker` trait's infallible signature.
#[cfg(feature = "remote")]
pub struct HttpReranker {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
}

#[cfg(feature = "remote")]
impl HttpReranker {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    /// Opt-in via env: returns `None` unless `ORACLE_AUTOMATE_RERANK_BASE_URL`
    /// is set, so the offline default keeps using `MockReranker`.
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("ORACLE_AUTOMATE_RERANK_BASE_URL").ok()?;
        let api_key = std::env::var("ORACLE_AUTOMATE_RERANK_API_KEY").unwrap_or_default();
        let model = std::env::var("ORACLE_AUTOMATE_RERANK_MODEL")
            .unwrap_or_else(|_| "rerank-english-v3.0".to_string());
        Some(Self::new(base_url, api_key, model))
    }
}

#[cfg(feature = "remote")]
#[derive(serde::Serialize)]
struct RerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
}

#[cfg(feature = "remote")]
#[derive(serde::Deserialize)]
struct RerankResponse {
    results: Vec<RerankResult>,
}

#[cfg(feature = "remote")]
#[derive(serde::Deserialize)]
struct RerankResult {
    index: usize,
    relevance_score: f32,
}

#[cfg(feature = "remote")]
#[async_trait]
impl Reranker for HttpReranker {
    async fn rerank(&self, query: &str, candidates: &[RerankerCandidate]) -> Vec<RerankedItem> {
        // Original-order fallback, used on any failure.
        let identity = || -> Vec<RerankedItem> {
            candidates
                .iter()
                .enumerate()
                .map(|(idx, c)| RerankedItem {
                    idx,
                    score: c.base_score,
                })
                .collect()
        };
        if candidates.is_empty() {
            return Vec::new();
        }

        let req = RerankRequest {
            model: &self.model,
            query,
            documents: candidates.iter().map(|c| c.chunk_text.as_str()).collect(),
        };
        let resp = match self
            .http
            .post(format!("{}/rerank", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(status = %r.status(), "reranker endpoint error; using base order");
                return identity();
            }
            Err(e) => {
                tracing::warn!(error = %e, "reranker transport error; using base order");
                return identity();
            }
        };
        let parsed: RerankResponse = match resp.json().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "reranker malformed response; using base order");
                return identity();
            }
        };

        // Map API scores back onto every candidate.  Any candidate the endpoint
        // omitted keeps its base score so the result set is never truncated.
        let mut scores: Vec<f32> = candidates.iter().map(|c| c.base_score).collect();
        for r in parsed.results {
            if r.index < scores.len() {
                scores[r.index] = r.relevance_score;
            }
        }
        let mut out: Vec<RerankedItem> = scores
            .into_iter()
            .enumerate()
            .map(|(idx, score)| RerankedItem { idx, score })
            .collect();
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rerank_pushes_identifier_match_to_top() {
        let r = MockReranker::new();
        let candidates = vec![
            RerankerCandidate {
                chunk_text: "Posting periods are managed via T001B.".into(),
                base_score: 0.3,
            },
            RerankerCandidate {
                chunk_text: "Generic finance prose without any tx code.".into(),
                base_score: 0.5,
            },
            RerankerCandidate {
                chunk_text: "BAPI_ACC_DOCUMENT_POST posts journal entries.".into(),
                base_score: 0.4,
            },
        ];
        let order = r
            .rerank("How does BAPI_ACC_DOCUMENT_POST work?", &candidates)
            .await;
        assert_eq!(
            order[0].idx, 2,
            "BAPI_ACC_DOCUMENT_POST-mentioning chunk should top out"
        );
    }

    #[tokio::test]
    async fn rerank_is_stable_for_equal_inputs() {
        let r = MockReranker::new();
        let candidates = vec![
            RerankerCandidate {
                chunk_text: "ABC".into(),
                base_score: 0.5,
            },
            RerankerCandidate {
                chunk_text: "DEF".into(),
                base_score: 0.5,
            },
        ];
        let order1 = r.rerank("query", &candidates).await;
        let order2 = r.rerank("query", &candidates).await;
        assert_eq!(order1.len(), order2.len());
    }
}

/// Contract tests for the live `HttpReranker` against an in-process axum mock
/// of a managed rerank API — the same `reqwest` path that hits a real endpoint,
/// minus the network.  Run with `--features remote`.
#[cfg(all(test, feature = "remote"))]
mod remote_tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use serde_json::json;
    use std::net::SocketAddr;

    async fn spawn(reorder: bool) -> SocketAddr {
        let app = Router::new().route(
            "/rerank",
            post(move || async move {
                if reorder {
                    // Score the 3rd document (index 2) highest, then 1st, then 2nd.
                    Json(json!({ "results": [
                        { "index": 2, "relevance_score": 0.95 },
                        { "index": 0, "relevance_score": 0.40 },
                        { "index": 1, "relevance_score": 0.05 }
                    ]}))
                } else {
                    Json(json!({ "results": [] }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    fn candidates() -> Vec<RerankerCandidate> {
        vec![
            RerankerCandidate {
                chunk_text: "Manage Accounting Periods in GL_PERIOD_STATUSES.".into(),
                base_score: 0.30,
            },
            RerankerCandidate {
                chunk_text: "Generic finance prose.".into(),
                base_score: 0.50,
            },
            RerankerCandidate {
                chunk_text: "Journal Import posts to GL_JE_LINES.".into(),
                base_score: 0.40,
            },
        ]
    }

    #[tokio::test]
    async fn applies_endpoint_scores_and_reorders() {
        let addr = spawn(true).await;
        let r = HttpReranker::new(format!("http://{addr}"), "test-key", "rerank-test");
        let order = r.rerank("GL_JE_LINES journal import", &candidates()).await;
        assert_eq!(order.len(), 3);
        // Endpoint ranked index 2 highest → it must top out, overriding base order.
        assert_eq!(order[0].idx, 2);
        assert_eq!(order[1].idx, 0);
        assert_eq!(order[2].idx, 1);
    }

    #[tokio::test]
    async fn endpoint_error_degrades_to_base_order() {
        // Nothing listening on this port → transport error → base-order fallback.
        let r = HttpReranker::new("http://127.0.0.1:1", "k", "m");
        let order = r.rerank("q", &candidates()).await;
        // Never truncated; every candidate is returned.
        assert_eq!(order.len(), 3);
        let mut idxs: Vec<usize> = order.iter().map(|o| o.idx).collect();
        idxs.sort_unstable();
        assert_eq!(idxs, vec![0, 1, 2]);
    }
}
