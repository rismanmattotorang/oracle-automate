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
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
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

/// Manual `Debug` so a stray `tracing::debug!(?creds)` or `{:?}` can never leak
/// the password — the derived `Debug` would have printed it verbatim.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("base_url", &self.base_url)
            .field("instance", &self.instance)
            .field("client", &self.client)
            .field("user", &self.user)
            .field("password", &"***")
            .field("language", &self.language)
            .field("proxy_url", &self.proxy_url)
            .field("source", &self.source)
            .finish()
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
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CredentialProvider for EnvCredentialProvider {
    async fn fetch(&self) -> ErpResult<Option<Credentials>> {
        let needed = [
            "ORACLE_FUSION_BASE_URL",
            "ORACLE_FUSION_INSTANCE",
            "ORACLE_FUSION_CLIENT",
            "ORACLE_FUSION_USER",
            "ORACLE_FUSION_PASSWORD",
        ];
        let present: Vec<_> = needed
            .iter()
            .filter(|k| std::env::var(*k).is_ok())
            .collect();
        if present.is_empty() {
            return Ok(None);
        }
        if present.len() < needed.len() {
            return Err(ErpError::AuthFailed(format!(
                "partial SAP env vars: missing {:?}",
                needed
                    .iter()
                    .filter(|k| std::env::var(*k).is_err())
                    .collect::<Vec<_>>(),
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
    pub fn new(creds: Credentials) -> Self {
        Self { creds }
    }
}

#[async_trait]
impl CredentialProvider for StaticCredentialProvider {
    async fn fetch(&self) -> ErpResult<Option<Credentials>> {
        Ok(Some(self.creds.clone()))
    }
}

// ---------------------------------------------------------------------------
// File provider (secrets manager / mounted secret)
// ---------------------------------------------------------------------------

/// Reads credentials from a JSON file.  This is the standard integration point
/// for a secrets manager that projects a secret as a file: a Kubernetes Secret
/// mounted as a volume, a HashiCorp Vault Agent / OCI Vault sidecar that syncs
/// the secret to a path, etc.  Preferred over `EnvCredentialProvider` in
/// production because the secret never lives in the process environment (which
/// leaks via `/proc/<pid>/environ`, crash dumps, and child processes).
///
/// The file is re-read on **every** `fetch`, so rotating the secret on disk is
/// picked up without a restart.  Expected JSON:
/// ```json
/// { "base_url": "...", "client": "100", "user": "SVC_AGENT", "password": "..." }
/// ```
pub struct FileCredentialProvider {
    path: PathBuf,
}

impl FileCredentialProvider {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Opt-in via env: returns `None` unless `ORACLE_AUTOMATE_CREDENTIALS_FILE`
    /// points at a file, so the offline/default path is unaffected.
    pub fn from_env() -> Option<Self> {
        std::env::var("ORACLE_AUTOMATE_CREDENTIALS_FILE")
            .ok()
            .filter(|s| !s.is_empty())
            .map(Self::new)
    }
}

#[derive(Deserialize)]
struct FileCreds {
    base_url: String,
    #[serde(default = "default_instance")]
    instance: String,
    client: String,
    user: String,
    password: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    proxy_url: Option<String>,
}

fn default_instance() -> String {
    "00".into()
}

fn default_language() -> String {
    "EN".into()
}

/// Best-effort warning if a secrets file is group/world-readable (Unix).
#[cfg(unix)]
fn warn_if_loose_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            tracing::warn!(
                path = %path.display(),
                mode = format!("{mode:o}"),
                "credentials file is group/world-accessible; tighten to 0600"
            );
        }
    }
}
#[cfg(not(unix))]
fn warn_if_loose_perms(_path: &Path) {}

#[async_trait]
impl CredentialProvider for FileCredentialProvider {
    async fn fetch(&self) -> ErpResult<Option<Credentials>> {
        let raw = match tokio::fs::read_to_string(&self.path).await {
            Ok(r) => r,
            // A not-yet-mounted secret is "no credentials here", not a hard error,
            // so the chain can fall through to the next provider.
            Err(_) => return Ok(None),
        };
        warn_if_loose_perms(&self.path);
        let fc: FileCreds = serde_json::from_str(&raw).map_err(|e| {
            ErpError::AuthFailed(format!(
                "credentials file {} is not valid JSON: {e}",
                self.path.display()
            ))
        })?;
        Ok(Some(Credentials {
            base_url: fc.base_url,
            instance: fc.instance,
            client: fc.client,
            user: fc.user,
            password: fc.password,
            language: fc.language,
            proxy_url: fc.proxy_url,
            source: CredentialSource::EncryptedFile,
        }))
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
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn with_provider(mut self, p: Arc<dyn CredentialProvider>) -> Self {
        self.providers.push(p);
        self
    }
}

impl Default for LayeredCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
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
            .with_provider(Arc::new(EnvCredentialProvider::new())) // unset env => None
            .with_provider(Arc::new(StaticCredentialProvider::new(Credentials {
                base_url: "fallback.example".into(),
                instance: "01".into(),
                client: "100".into(),
                user: "DEMO".into(),
                password: "x".into(),
                language: "EN".into(),
                proxy_url: None,
                source: CredentialSource::Static,
            })));
        let creds = layered.fetch().await.unwrap().unwrap();
        assert_eq!(creds.base_url, "fallback.example");
    }

    #[test]
    fn debug_never_leaks_password() {
        let creds = Credentials {
            base_url: "oracle.example".into(),
            instance: "00".into(),
            client: "100".into(),
            user: "SVC".into(),
            password: "super-secret-value".into(),
            language: "EN".into(),
            proxy_url: None,
            source: CredentialSource::Static,
        };
        let dbg = format!("{creds:?}");
        assert!(
            !dbg.contains("super-secret-value"),
            "password leaked via Debug: {dbg}"
        );
        assert!(dbg.contains("***"));
    }

    #[tokio::test]
    async fn file_provider_reads_json_secret() {
        let mut path = std::env::temp_dir();
        path.push(format!("oracle-automate-creds-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{ "base_url": "https://gaussian.fa.ocs.oraclecloud.com", "client": "100", "user": "SVC_AGENT", "password": "file-secret" }"#,
        )
        .unwrap();
        let p = FileCredentialProvider::new(&path);
        let creds = p.fetch().await.unwrap().unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(creds.user, "SVC_AGENT");
        assert_eq!(creds.password, "file-secret");
        assert_eq!(creds.language, "EN"); // serde default
        assert_eq!(creds.source, CredentialSource::EncryptedFile);
    }

    #[tokio::test]
    async fn file_provider_absent_file_falls_through() {
        let p = FileCredentialProvider::new("/nonexistent/oracle-automate/secret.json");
        assert!(p.fetch().await.unwrap().is_none());
    }
}
