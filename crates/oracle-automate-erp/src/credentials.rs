//! Layered credential provider.
//!
//! Mirrors the priority chain from `a reference REST-metadata-cache design`
//! (env → keyring → encrypted file → .env), but the chain itself is the
//! configurable artefact — callers compose any number of providers in any
//! order, and the first one that yields credentials wins.
//!
//! For Phase 2 we ship `EnvCredentialProvider` (env vars) and
//! `StaticCredentialProvider` (literal values, useful for tests).  Keyring
//! and encrypted-file providers will follow in Phase 7 (security hardening)
//! when the OAuth flow is also finalised.

use crate::error::{ErpError, ErpResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub base_url: String,
    pub instance: String,
    pub client: String,
    pub user: String,
    /// Stored only for the lifetime of the running process.  Never logged.
    #[serde(skip_serializing)]
    pub password: String,
    pub language: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    /// Where this credential came from (for audit logs).
    pub source: CredentialSource,
}

impl Credentials {
    /// Redacted summary safe for logs and the `sap.system.info` resource.
    pub fn redacted(&self) -> serde_json::Value {
        serde_json::json!({
            "base_url": self.base_url,
            "instance": self.instance,
            "client": self.client,
            "user": self.user,
            "language": self.language,
            "proxy_url": self.proxy_url,
            "source": format!("{:?}", self.source),
            "password": "***",
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CredentialSource {
    Env,
    Keyring,
    EncryptedFile,
    DotEnv,
    Static,
    None,
}

#[async_trait]
pub trait CredentialProvider: Send + Sync {
    /// Returns `Ok(None)` if this provider has no credentials configured
    /// (so the caller can move to the next in the chain).
    async fn fetch(&self) -> ErpResult<Option<Credentials>>;
}

// ---------------------------------------------------------------------------
// Environment provider
// ---------------------------------------------------------------------------

pub struct EnvCredentialProvider;

impl EnvCredentialProvider {
    pub fn new() -> Self { Self }
}

impl Default for EnvCredentialProvider {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl CredentialProvider for EnvCredentialProvider {
    async fn fetch(&self) -> ErpResult<Option<Credentials>> {
        let needed = ["ORACLE_FUSION_BASE_URL", "ORACLE_FUSION_INSTANCE", "ORACLE_FUSION_CLIENT", "ORACLE_FUSION_USER", "ORACLE_FUSION_PASSWORD"];
        let present: Vec<_> = needed.iter().filter(|k| std::env::var(*k).is_ok()).collect();
        if present.is_empty() { return Ok(None); }
        if present.len() < needed.len() {
            return Err(ErpError::AuthFailed(format!(
                "partial SAP env vars: missing {:?}",
                needed.iter().filter(|k| std::env::var(*k).is_err()).collect::<Vec<_>>(),
            )));
        }
        Ok(Some(Credentials {
            base_url: std::env::var("ORACLE_FUSION_BASE_URL").unwrap(),
            instance: std::env::var("ORACLE_FUSION_INSTANCE").unwrap(),
            client: std::env::var("ORACLE_FUSION_CLIENT").unwrap(),
            user: std::env::var("ORACLE_FUSION_USER").unwrap(),
            password: std::env::var("ORACLE_FUSION_PASSWORD").unwrap(),
            language: std::env::var("ORACLE_FUSION_LANGUAGE").unwrap_or_else(|_| "EN".to_string()),
            proxy_url: std::env::var("ORACLE_FUSION_PROXY").ok(),
            source: CredentialSource::Env,
        }))
    }
}

// ---------------------------------------------------------------------------
// Static provider (tests, demos)
// ---------------------------------------------------------------------------

pub struct StaticCredentialProvider {
    creds: Credentials,
}

impl StaticCredentialProvider {
    pub fn new(creds: Credentials) -> Self { Self { creds } }
}

#[async_trait]
impl CredentialProvider for StaticCredentialProvider {
    async fn fetch(&self) -> ErpResult<Option<Credentials>> {
        Ok(Some(self.creds.clone()))
    }
}

// ---------------------------------------------------------------------------
// Layered chain
// ---------------------------------------------------------------------------

/// Tries each underlying provider in order; the first that returns
/// `Some(creds)` wins.  Returns `Ok(None)` only if every provider was empty.
pub struct LayeredCredentialProvider {
    providers: Vec<Arc<dyn CredentialProvider>>,
}

impl LayeredCredentialProvider {
    pub fn new() -> Self { Self { providers: Vec::new() } }

    pub fn add(mut self, p: Arc<dyn CredentialProvider>) -> Self {
        self.providers.push(p);
        self
    }
}

impl Default for LayeredCredentialProvider {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl CredentialProvider for LayeredCredentialProvider {
    async fn fetch(&self) -> ErpResult<Option<Credentials>> {
        for p in &self.providers {
            match p.fetch().await {
                Ok(Some(c)) => return Ok(Some(c)),
                Ok(None) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn static_provider_returns_credentials() {
        let p = StaticCredentialProvider::new(Credentials {
            base_url: "oracle.example".into(),
            instance: "00".into(),
            client: "100".into(),
            user: "DEMO".into(),
            password: "x".into(),
            language: "EN".into(),
            proxy_url: None,
            source: CredentialSource::Static,
        });
        let creds = p.fetch().await.unwrap().unwrap();
        assert_eq!(creds.client, "100");
        let r = creds.redacted();
        assert_eq!(r["password"], "***");
    }

    #[tokio::test]
    async fn layered_falls_through() {
        let layered = LayeredCredentialProvider::new()
            .add(Arc::new(EnvCredentialProvider::new())) // unset env => None
            .add(Arc::new(StaticCredentialProvider::new(Credentials {
                base_url: "fallback.example".into(), instance: "01".into(), client: "100".into(),
                user: "DEMO".into(), password: "x".into(), language: "EN".into(),
                proxy_url: None, source: CredentialSource::Static,
            })));
        let creds = layered.fetch().await.unwrap().unwrap();
        assert_eq!(creds.base_url, "fallback.example");
    }
}
