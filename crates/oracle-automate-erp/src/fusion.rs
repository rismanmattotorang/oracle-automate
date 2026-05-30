//! Live Oracle Fusion Cloud ERP transport over REST/JSON.
//!
//! Oracle Fusion is REST/JSON-homogeneous, so a single client replaces the
//! two heterogeneous SAP live clients (SOAP + OData) the
//! original platform shipped:
//!
//! - [`HttpFusionClient`] implements [`ErpClient`] against the Fusion REST
//!   API (`/fscmRestApi/...`).  Read-only metadata / search / structure are
//!   served from the curated catalogue (offline-safe, deterministic); live
//!   HTTP is used for `system_info` and `call_operation` (REST dispatch by
//!   operation id).  Bulk tabular extracts go through BI Publisher
//!   (`fusion.bip.runReport`) rather than a generic table read.
//! - [`FusionPartyClient`] reads Trading Community Architecture parties
//!   (suppliers / customer accounts) for the `oracle.party.*` tools.
//!
//! Auth: OAuth2 client-credentials (IDCS/IAM) bearer, or HTTP Basic — chosen
//! by `ORACLE_FUSION_AUTH`.  Credentials never appear in logs (only the
//! `auth` label).

use crate::client::{
    BulkMetadata, ErpCallRequest, ErpClient, ErpOperationMeta, ErpSearchResult, ReadTableRequest,
    SystemInfo, TableRow, TableStructure,
};
use crate::error::{ErpError, ErpResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

const REST_BASE: &str = "/fscmRestApi/resources/11.13.18.05";

// ===========================================================================
// Config + auth
// ===========================================================================

#[derive(Clone)]
pub enum FusionAuth {
    /// OAuth2 client-credentials bearer (IDCS/IAM); the resolved access token.
    Bearer(String),
    /// HTTP Basic (integration/technical user).
    Basic { user: String, password: String },
}

impl FusionAuth {
    pub fn label(&self) -> &'static str {
        match self {
            FusionAuth::Bearer(_) => "oauth2",
            FusionAuth::Basic { .. } => "basic",
        }
    }
    fn apply(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            FusionAuth::Bearer(t) => rb.bearer_auth(t),
            FusionAuth::Basic { user, password } => rb.basic_auth(user, Some(password)),
        }
    }
}

impl std::fmt::Debug for FusionAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FusionAuth::{}", self.label())
    }
}

/// Default per-request timeout for the live Fusion transport.  A real pod can
/// hang; without a timeout the client would wait forever.
pub const DEFAULT_FUSION_TIMEOUT_MS: u64 = 30_000;

#[derive(Clone, Debug)]
pub struct FusionConfig {
    /// Pod base URL, e.g. `https://gaussian.fa.ocs.oraclecloud.com`.
    pub base_url: String,
    pub auth: FusionAuth,
    /// Per-request timeout (ms).  Override via `ORACLE_FUSION_TIMEOUT_MS`.
    pub timeout_ms: u64,
}

impl FusionConfig {
    pub fn new(base_url: impl Into<String>, auth: FusionAuth) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth,
            timeout_ms: DEFAULT_FUSION_TIMEOUT_MS,
        }
    }

    /// Override the per-request timeout (ms).
    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Build from `ORACLE_FUSION_*` env vars.  Returns `None` when no base
    /// URL is configured (the server then falls back to the offline mock).
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("ORACLE_FUSION_BASE_URL").ok()?;
        let auth = match std::env::var("ORACLE_FUSION_AUTH").as_deref() {
            Ok("basic") => FusionAuth::Basic {
                user: std::env::var("ORACLE_FUSION_USER").unwrap_or_default(),
                password: std::env::var("ORACLE_FUSION_PASSWORD").unwrap_or_default(),
            },
            // default: OAuth2 bearer (token resolved out-of-band / injected)
            _ => {
                FusionAuth::Bearer(std::env::var("ORACLE_FUSION_ACCESS_TOKEN").unwrap_or_default())
            }
        };
        let mut cfg = Self::new(base, auth);
        if let Some(ms) = std::env::var("ORACLE_FUSION_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse().ok())
        {
            cfg.timeout_ms = ms;
        }
        Some(cfg)
    }

    pub fn redacted(&self) -> Value {
        json!({ "base_url": self.base_url, "auth": self.auth.label(), "timeout_ms": self.timeout_ms })
    }
}

fn map_http_err(e: reqwest::Error) -> ErpError {
    if e.is_timeout() || e.is_connect() {
        ErpError::DestinationDown {
            destination: "oracle-fusion".into(),
            reason: format!("Fusion REST transport error: {e}"),
        }
    } else {
        ErpError::Internal(format!("Fusion REST error: {e}"))
    }
}

// ===========================================================================
// HttpFusionClient — live ErpClient over Fusion REST
// ===========================================================================

pub struct HttpFusionClient {
    http: reqwest::Client,
    config: FusionConfig,
    /// Curated catalogue (the mock) backing read-only metadata + the
    /// read-only safety gate.  Live HTTP is used for state + system info.
    catalogue: Arc<dyn ErpClient>,
}

impl HttpFusionClient {
    pub fn new(config: FusionConfig, catalogue: Arc<dyn ErpClient>) -> ErpResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| ErpError::Internal(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            http,
            config,
            catalogue,
        })
    }

    pub fn config(&self) -> &FusionConfig {
        &self.config
    }

    /// Derive `(method, resource-collection)` from a catalogue operation id
    /// like `fusion.gl.journalEntries.post` → `("POST", "journalEntries")`.
    fn dispatch(op: &str) -> (reqwest::Method, String) {
        let verb = op.rsplit('.').next().unwrap_or("get");
        let resource = op.rsplit('.').nth(1).unwrap_or("").to_string();
        let method = match verb {
            "post" => reqwest::Method::POST,
            "patch" => reqwest::Method::PATCH,
            "delete" => reqwest::Method::DELETE,
            _ => reqwest::Method::GET,
        };
        (method, resource)
    }
}

#[async_trait]
impl ErpClient for HttpFusionClient {
    async fn system_info(&self) -> ErpResult<SystemInfo> {
        // Touch the REST catalog root to confirm reachability + identity.
        let url = format!("{}{}", self.config.base_url, REST_BASE);
        let resp = self
            .config
            .auth
            .apply(self.http.get(&url))
            .header("REST-Framework-Version", "9")
            .send()
            .await
            .map_err(map_http_err)?;
        let release = resp
            .headers()
            .get("X-ORACLE-DMS-ECID")
            .and_then(|v| v.to_str().ok())
            .map(|_| "Oracle Fusion Cloud ERP (live)".to_string())
            .unwrap_or_else(|| "Oracle Fusion Cloud ERP (live)".to_string());
        let host = self
            .config
            .base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string();
        Ok(SystemInfo {
            sid: host.split('.').next().unwrap_or("FA").to_uppercase(),
            client: "LIVE".into(),
            release,
            system_role: "LIVE".into(),
            host,
            instance: "fa".into(),
            identity: self.config.redacted(),
        })
    }

    async fn search_operations(&self, query: &str, limit: usize) -> ErpResult<ErpSearchResult> {
        self.catalogue.search_operations(query, limit).await
    }

    async fn operation_metadata(
        &self,
        function: &str,
        language: &str,
    ) -> ErpResult<ErpOperationMeta> {
        self.catalogue.operation_metadata(function, language).await
    }

    async fn bulk_operation_metadata(
        &self,
        functions: &[String],
        language: &str,
    ) -> ErpResult<BulkMetadata> {
        self.catalogue
            .bulk_operation_metadata(functions, language)
            .await
    }

    async fn call_operation(
        &self,
        request: ErpCallRequest,
        read_only_mode: bool,
    ) -> ErpResult<Value> {
        // Fail-closed read-only gate via the curated catalogue.
        match self
            .catalogue
            .operation_metadata(&request.function, "EN")
            .await
        {
            Ok(meta) => {
                if read_only_mode && !meta.read_only {
                    return Err(ErpError::PermissionDenied(format!(
                        "operation '{}' modifies state; not callable in read-only mode",
                        request.function
                    )));
                }
            }
            Err(ErpError::NotFound(_)) if read_only_mode => {
                return Err(ErpError::PermissionDenied(format!(
                    "operation '{}' is not in the curated read-only catalogue; refusing in read-only mode",
                    request.function
                )));
            }
            Err(ErpError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let (method, resource) = Self::dispatch(&request.function);
        let url = format!("{}{}/{}", self.config.base_url, REST_BASE, resource);
        let mut rb = self
            .config
            .auth
            .apply(self.http.request(method.clone(), &url))
            .header("REST-Framework-Version", "9");
        if matches!(method, reqwest::Method::POST | reqwest::Method::PATCH) {
            rb = rb.json(&request.parameters);
        }
        let resp = rb.send().await.map_err(map_http_err)?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok(json!({
            "function": request.function,
            "executed_on": self.config.base_url,
            "http_status": status.as_u16(),
            "outputs": body,
        }))
    }

    async fn read_table(&self, request: ReadTableRequest) -> ErpResult<Vec<TableRow>> {
        // Oracle Fusion has no generic table read; tabular extracts go through
        // BI Publisher (fusion.bip.runReport).  We serve the curated fixtures
        // for the modelled objects and direct callers to BI Publisher
        // otherwise — never a silent unbounded pull.
        self.catalogue.read_table(request).await
    }

    async fn table_structure(&self, table: &str) -> ErpResult<TableStructure> {
        self.catalogue.table_structure(table).await
    }
}

// ===========================================================================
// FusionPartyClient — TCA parties (suppliers / customer accounts)
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party {
    pub id: String,
    pub name: String,
    /// `supplier` | `customer`.
    pub party_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

pub struct FusionPartyClient {
    http: reqwest::Client,
    config: FusionConfig,
}

impl FusionPartyClient {
    pub fn new(config: FusionConfig) -> ErpResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| ErpError::Internal(format!("failed to build HTTP client: {e}")))?;
        Ok(Self { http, config })
    }

    pub fn from_env() -> Option<ErpResult<Self>> {
        FusionConfig::from_env().map(Self::new)
    }

    pub fn config(&self) -> &FusionConfig {
        &self.config
    }

    /// Search suppliers by name substring (`suppliers?q=Supplier LIKE '%q%'`).
    pub async fn search_parties(&self, query: &str, top: usize) -> ErpResult<Vec<Party>> {
        let q = format!("Supplier LIKE '%{}%'", query.replace('\'', ""));
        let url = format!("{}{}/suppliers", self.config.base_url, REST_BASE);
        let resp = self
            .config
            .auth
            .apply(self.http.get(&url))
            .header("REST-Framework-Version", "9")
            .query(&[("q", q.as_str()), ("limit", &top.to_string())])
            .send()
            .await
            .map_err(map_http_err)?;
        let body: Value = resp.json().await.map_err(map_http_err)?;
        Ok(parse_parties(&body))
    }

    /// Fetch a single supplier by SupplierId.
    pub async fn get_party(&self, id: &str) -> ErpResult<Party> {
        let url = format!("{}{}/suppliers/{}", self.config.base_url, REST_BASE, id);
        let resp = self
            .config
            .auth
            .apply(self.http.get(&url))
            .header("REST-Framework-Version", "9")
            .send()
            .await
            .map_err(map_http_err)?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ErpError::NotFound(id.to_string()));
        }
        let body: Value = resp.json().await.map_err(map_http_err)?;
        Ok(party_from_obj(&body).unwrap_or(Party {
            id: id.to_string(),
            name: String::new(),
            party_type: "supplier".into(),
            party_number: None,
            status: None,
        }))
    }
}

fn parse_parties(body: &Value) -> Vec<Party> {
    body.get("items")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(party_from_obj).collect())
        .unwrap_or_default()
}

fn party_from_obj(o: &Value) -> Option<Party> {
    let id = o
        .get("SupplierId")
        .map(value_to_string)
        .or_else(|| o.get("PartyId").map(value_to_string))?;
    let name = o
        .get("Supplier")
        .or_else(|| o.get("PartyName"))
        .map(value_to_string)
        .unwrap_or_default();
    Some(Party {
        id,
        name,
        party_type: "supplier".into(),
        party_number: o.get("SupplierNumber").map(value_to_string),
        status: o.get("Status").map(value_to_string),
    })
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockErpClient;

    #[test]
    fn dispatch_maps_op_id_to_method_and_resource() {
        let (m, r) = HttpFusionClient::dispatch("fusion.gl.journalEntries.post");
        assert_eq!(m, reqwest::Method::POST);
        assert_eq!(r, "journalEntries");
        let (m, r) = HttpFusionClient::dispatch("fusion.scm.itemsV2.get");
        assert_eq!(m, reqwest::Method::GET);
        assert_eq!(r, "itemsV2");
    }

    #[test]
    fn config_from_env_requires_base_url() {
        std::env::remove_var("ORACLE_FUSION_BASE_URL");
        assert!(FusionConfig::from_env().is_none());
    }

    #[test]
    fn auth_label_never_leaks_secret() {
        let a = FusionAuth::Basic {
            user: "u".into(),
            password: "secret".into(),
        };
        assert_eq!(a.label(), "basic");
        assert!(!format!("{a:?}").contains("secret"));
    }

    #[tokio::test]
    async fn read_only_gate_blocks_writes_via_catalogue() {
        let cat = MockErpClient::new(2, json!({}));
        let cfg = FusionConfig::new(
            "https://gaussian.fa.ocs.oraclecloud.com",
            FusionAuth::Bearer("t".into()),
        );
        let client = HttpFusionClient::new(cfg, cat).unwrap();
        let req = ErpCallRequest {
            function: "fusion.gl.journalEntries.post".into(),
            parameters: json!({ "JOURNAL_ENTRY": {} }),
            timeout_ms: 1000,
            require_read_only_safe: true,
        };
        let err = client.call_operation(req, true).await.unwrap_err();
        assert!(matches!(err, ErpError::PermissionDenied(_)));
    }

    #[test]
    fn parse_parties_reads_items_collection() {
        let body = json!({ "items": [
            { "SupplierId": 300, "Supplier": "PT Sumber Daya Komputasi", "SupplierNumber": "S-300", "Status": "ACTIVE" }
        ]});
        let parties = parse_parties(&body);
        assert_eq!(parties.len(), 1);
        assert_eq!(parties[0].id, "300");
        assert_eq!(parties[0].name, "PT Sumber Daya Komputasi");
    }
}
