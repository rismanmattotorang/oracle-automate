//! ErpClient trait and offline mock implementation.
//!
//! The trait is the central abstraction the MCP server depends on.  Two
//! concrete backends are envisioned:
//!   - `MockErpClient` — ships now; deterministic in-memory fixtures so the
//!     full MCP tool surface (system info / REST operation search / REST operation metadata / REST operation
//!     call / table read / table structure / bulk metadata) is callable
//!     offline and in CI.  This is what makes Phase 2 demonstrable without
//!     a live Oracle pod.
//!   - `NetweaverErpClient` (Phase 2 finalisation): wraps a real REST operation SDK
//!     binding behind the same trait.  Adoption needs no MCP server change.
//!
//! Pattern note: every method takes `&self` and returns a `ErpResult`.
//! The pool / circuit-breaker / retry helpers wrap calls externally so
//! individual backends stay simple.

use crate::error::{ErpError, ErpResult};
use crate::pool::ConnectionPool;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

// ===========================================================================
// Shared types
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Fusion environment / pod identifier, e.g. "GAUSSIAN-FA-PROD".
    pub sid: String,
    /// Enterprise / data-scope hint (Ledger or Business Unit), e.g. "GAUSSIAN_PRIMARY_LEDGER".
    pub client: String,
    /// Fusion Applications release, e.g. "Oracle Fusion Cloud ERP 24D (11.13.24.10.0)".
    pub release: String,
    /// Environment role, e.g. "PROD" / "TEST" / "DEV".
    pub system_role: String,
    pub host: String,
    /// Fusion service edition / data-centre instance.
    pub instance: String,
    /// `Credentials::redacted()`.
    pub identity: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ErpParamDirection {
    Import,
    Export,
    Changing,
    Tables,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpParameter {
    pub name: String,
    pub direction: ErpParamDirection,
    /// OIC/custom-code type token (e.g. `CHAR(10)`, `MATNR`, `STRUCT(BAPIMATHEAD)`).
    #[serde(rename = "type")]
    pub type_token: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpOperationMeta {
    pub function: String,
    pub description: String,
    /// e.g. "FBAS" / "MM" / "SD"
    pub function_group: String,
    /// Devclass / package, e.g. "ZFIN".
    #[serde(default)]
    pub package: Option<String>,
    pub parameters: Vec<ErpParameter>,
    #[serde(default)]
    pub deprecated: bool,
    /// Whether the function is safe to call read-only.  Surfaces the
    /// MDK-inspired read-only-mode safety property (CData pattern).
    pub read_only: bool,
    /// Whether the operation is a *bulk* write that lands in an interface
    /// staging area and only persists after a follow-up import job
    /// (FBDI `importBulkData`), or — on the EBS backend — requires an
    /// explicit `p_commit => FND_API.G_TRUE`.  Synchronous Fusion REST
    /// writes auto-commit per request and leave this `false`.
    #[serde(default)]
    pub commit_required: bool,
    /// Oracle RBAC privileges required to invoke this operation.  Used by
    /// the server to advise the agent (and the SoD audit) before a call
    /// goes out.
    #[serde(default)]
    pub authorization: Vec<RequiredPrivilege>,
    /// Oracle-specific note: REST resource version, the underlying GL/SLA
    /// objects a write lands in, FBDI template name, or a Fusion-vs-EBS
    /// caveat.
    #[serde(default)]
    pub erp_note: Option<String>,
}

/// One Oracle RBAC privilege required to invoke an operation.
///
/// Oracle Fusion secures REST/SOAP endpoints with *function security
/// privileges* aggregated into duty roles, which roll up into job and
/// data roles.  This is an Oracle RBAC privilege grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredPrivilege {
    /// Privilege code, e.g. `GL_RUN_JOURNAL_IMPORT_PRIV`.
    pub privilege: String,
    /// Duty role that aggregates the privilege, e.g.
    /// `General Accounting Manager`.
    pub duty_role: String,
    /// Action the privilege grants: `VIEW`, `MANAGE`, `RUN`, `SUBMIT`.
    pub action: String,
}

impl RequiredPrivilege {
    /// Convenience for the common "run/submit a privilege via a duty role" case.
    pub fn run(privilege: &str, duty_role: &str) -> Self {
        Self {
            privilege: privilege.into(),
            duty_role: duty_role.into(),
            action: "RUN".into(),
        }
    }
    pub fn view(privilege: &str, duty_role: &str) -> Self {
        Self {
            privilege: privilege.into(),
            duty_role: duty_role.into(),
            action: "VIEW".into(),
        }
    }
    pub fn manage(privilege: &str, duty_role: &str) -> Self {
        Self {
            privilege: privilege.into(),
            duty_role: duty_role.into(),
            action: "MANAGE".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpOperationSummary {
    pub function: String,
    pub description: String,
    pub function_group: String,
    pub read_only: bool,
    /// Rank score from the search; higher = better match.
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpSearchResult {
    pub query: String,
    pub hits: Vec<ErpOperationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpCallRequest {
    pub function: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// If true, the call will be rejected when the client is in read-only
    /// mode AND the function is not declared `read_only` in its metadata.
    #[serde(default = "default_true")]
    pub require_read_only_safe: bool,
}

fn default_timeout_ms() -> u64 {
    30_000
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkMetadata {
    pub language: String,
    pub functions: Vec<ErpOperationMeta>,
    /// Functions that were requested but not found.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableField {
    pub name: String,
    pub data_element: String,
    /// OIC/custom-code-side type (e.g. `CHAR`, `NUMC`, `DEC`, `DATS`).
    #[serde(rename = "type")]
    pub type_token: String,
    pub length: u32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStructure {
    pub table: String,
    pub description: String,
    pub fields: Vec<TableField>,
    pub key_fields: Vec<String>,
    /// Oracle data-security policy group.  Sensitive
    /// tables (BSEG, PA0008) carry restricted groups; open tables carry
    /// `&NC&` ("not classified").  Empty string for views.
    #[serde(default)]
    pub authorization_group: String,
    /// Oracle Fusion Cloud ERP storage note.  Empty for tables that are unchanged
    /// between ECC and Oracle Fusion Cloud ERP; populated for compatibility views.
    #[serde(default)]
    pub storage_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadTableRequest {
    pub table: String,
    /// Column projection; empty = all fields.
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub where_conditions: Vec<String>,
    /// Hard cap.  We default to 100 and refuse more than 1000 (buffer
    /// overflow safety, matching the Python reference project).
    #[serde(default = "default_max_rows")]
    pub max_rows: usize,
}

fn default_max_rows() -> usize {
    100
}

pub const MAX_ROWS_HARD_CAP: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    pub values: serde_json::Map<String, serde_json::Value>,
}

// ===========================================================================
// ErpClient trait
// ===========================================================================

#[async_trait]
pub trait ErpClient: Send + Sync {
    async fn system_info(&self) -> ErpResult<SystemInfo>;

    async fn search_operations(&self, query: &str, limit: usize) -> ErpResult<ErpSearchResult>;

    async fn operation_metadata(
        &self,
        function: &str,
        language: &str,
    ) -> ErpResult<ErpOperationMeta>;

    async fn bulk_operation_metadata(
        &self,
        functions: &[String],
        language: &str,
    ) -> ErpResult<BulkMetadata>;

    async fn call_operation(
        &self,
        request: ErpCallRequest,
        read_only_mode: bool,
    ) -> ErpResult<serde_json::Value>;

    async fn read_table(&self, request: ReadTableRequest) -> ErpResult<Vec<TableRow>>;

    async fn table_structure(&self, table: &str) -> ErpResult<TableStructure>;

    /// Pool snapshot for the TUI / Prometheus dashboards.
    fn pool_status(&self) -> PoolStatus {
        PoolStatus {
            cap: 0,
            available: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PoolStatus {
    pub cap: usize,
    pub available: usize,
}

// ===========================================================================
// MockErpClient — offline reference implementation
// ===========================================================================

/// Mock client backed by realistic Oracle-shaped fixtures.
///
/// The fixture set is intentionally small but covers FI, MM, SD, and HR
/// canon: ATC-relevant BAPIs, common tables, expected error shapes.  Lets
/// the MCP server be exercised end-to-end without a live Oracle pod.
pub struct MockErpClient {
    pool: ConnectionPool,
    functions: HashMap<String, ErpOperationMeta>,
    tables: HashMap<String, MockTable>,
    identity: serde_json::Value,
}

struct MockTable {
    structure: TableStructure,
    rows: Vec<serde_json::Map<String, serde_json::Value>>,
}

impl MockErpClient {
    pub fn new(pool_size: usize, identity: serde_json::Value) -> Arc<Self> {
        let mut s = Self {
            pool: ConnectionPool::new(pool_size),
            functions: HashMap::new(),
            tables: HashMap::new(),
            identity,
        };
        s.seed_functions();
        s.seed_tables();
        Arc::new(s)
    }

    fn seed_functions(&mut self) {
        for f in seed_functions() {
            self.functions.insert(f.function.clone(), f);
        }
    }

    fn seed_tables(&mut self) {
        for t in seed_tables() {
            self.tables.insert(t.structure.table.clone(), t);
        }
    }
}

#[async_trait]
impl ErpClient for MockErpClient {
    async fn system_info(&self) -> ErpResult<SystemInfo> {
        let _p = self.pool.acquire().await?;
        Ok(SystemInfo {
            sid: "GAUSSIAN-FA-DEV".into(),
            client: "GAUSSIAN_PRIMARY_LEDGER".into(),
            release: "Oracle Fusion Cloud ERP 24D (11.13.24.10.0) (mock)".into(),
            system_role: "DEV".into(),
            host: "gaussian-dev.fa.ocs.oraclecloud.com".into(),
            instance: "fa-edpb".into(),
            identity: self.identity.clone(),
        })
    }

    async fn search_operations(&self, query: &str, limit: usize) -> ErpResult<ErpSearchResult> {
        let _p = self.pool.acquire().await?;
        let q = query.to_lowercase();
        let terms: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<ErpOperationSummary> = self
            .functions
            .values()
            .filter_map(|f| {
                let hay = format!(
                    "{} {} {}",
                    f.function.to_lowercase(),
                    f.description.to_lowercase(),
                    f.function_group.to_lowercase()
                );
                let score: usize = terms.iter().map(|t| hay.matches(t).count()).sum();
                if score == 0 {
                    None
                } else {
                    Some(ErpOperationSummary {
                        function: f.function.clone(),
                        description: f.description.clone(),
                        function_group: f.function_group.clone(),
                        read_only: f.read_only,
                        score: score as f32,
                    })
                }
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit.max(1));
        Ok(ErpSearchResult {
            query: query.into(),
            hits,
        })
    }

    async fn operation_metadata(
        &self,
        function: &str,
        _language: &str,
    ) -> ErpResult<ErpOperationMeta> {
        let _p = self.pool.acquire().await?;
        self.functions
            .get(function)
            .cloned()
            .ok_or_else(|| ErpError::NotFound(function.into()))
    }

    async fn bulk_operation_metadata(
        &self,
        functions: &[String],
        language: &str,
    ) -> ErpResult<BulkMetadata> {
        let _p = self.pool.acquire().await?;
        let mut out = Vec::new();
        let mut missing = Vec::new();
        for f in functions {
            match self.functions.get(f) {
                Some(meta) => out.push(meta.clone()),
                None => missing.push(f.clone()),
            }
        }
        Ok(BulkMetadata {
            language: language.into(),
            functions: out,
            missing,
        })
    }

    async fn call_operation(
        &self,
        request: ErpCallRequest,
        read_only_mode: bool,
    ) -> ErpResult<serde_json::Value> {
        let _p = self.pool.acquire().await?;
        let meta = self
            .functions
            .get(&request.function)
            .ok_or_else(|| ErpError::NotFound(request.function.clone()))?;
        if read_only_mode && !meta.read_only {
            return Err(ErpError::PermissionDenied(format!(
                "function '{}' modifies state; not callable in read-only mode",
                request.function,
            )));
        }

        // Validate that every required parameter is present.
        let args = match &request.parameters {
            serde_json::Value::Object(m) => m.clone(),
            serde_json::Value::Null => serde_json::Map::new(),
            other => {
                return Err(ErpError::InvalidParameter {
                    name: "parameters".into(),
                    reason: format!("expected object, got {}", other),
                })
            }
        };
        for p in &meta.parameters {
            if p.direction == ErpParamDirection::Import
                && !p.optional
                && !args.contains_key(&p.name)
            {
                return Err(ErpError::InvalidParameter {
                    name: p.name.clone(),
                    reason: "required import parameter missing".into(),
                });
            }
        }

        // Mock execution: echo + synthetic export.  Real backends invoke the REST operation.
        debug!(function = %request.function, "mock REST operation executed");
        Ok(serde_json::json!({
            "function": request.function,
            "executed_on": "mock.fa.oraclecloud.com",
            "inputs": args,
            "outputs": mock_outputs(meta, &args),
        }))
    }

    async fn read_table(&self, request: ReadTableRequest) -> ErpResult<Vec<TableRow>> {
        let _p = self.pool.acquire().await?;
        if request.max_rows == 0 {
            return Err(ErpError::InvalidParameter {
                name: "max_rows".into(),
                reason: "must be >= 1".into(),
            });
        }
        if request.max_rows > MAX_ROWS_HARD_CAP {
            return Err(ErpError::TableBufferOverflow {
                table: request.table.clone(),
                max_rows: request.max_rows,
            });
        }
        let table = self
            .tables
            .get(&request.table)
            .ok_or_else(|| ErpError::NotFound(request.table.clone()))?;

        // Field projection.
        let projection: Vec<String> = if request.fields.is_empty() {
            table
                .structure
                .fields
                .iter()
                .map(|f| f.name.clone())
                .collect()
        } else {
            for f in &request.fields {
                if !table
                    .structure
                    .fields
                    .iter()
                    .any(|tf| tf.name.eq_ignore_ascii_case(f))
                {
                    return Err(ErpError::InvalidParameter {
                        name: "fields".into(),
                        reason: format!("unknown field '{f}'"),
                    });
                }
            }
            request.fields.clone()
        };

        let mut conditions = parse_conditions(&request.where_conditions)?;

        // queries are scoped by ledger/BU.  If the caller didn't
        // specify a the ledger/BU scope / RCLNT clause and the table has one, restrict
        // to the connection's client number so cross-client leaks are
        // impossible by construction.  This matches the behaviour of
        // SE16/SM30 and the standard a BI Publisher extract convention.
        let client_field = table
            .structure
            .fields
            .first()
            .filter(|f| {
                (f.name == "the ledger/BU scope" || f.name == "RCLNT") && f.type_token == "CLNT"
            })
            .map(|f| f.name.clone());
        if let Some(field) = client_field.as_deref() {
            let has_client_filter = conditions
                .iter()
                .any(|(f, _, _)| f.eq_ignore_ascii_case(field));
            if !has_client_filter {
                conditions.push((
                    field.into(),
                    "=".into(),
                    self.identity
                        .get("client")
                        .and_then(|v| v.as_str())
                        .unwrap_or("100")
                        .to_string(),
                ));
            }
        }

        let mut rows: Vec<TableRow> = Vec::new();
        for row in &table.rows {
            if conditions
                .iter()
                .all(|(field, op, value)| match_row(row, field, op, value))
            {
                let projected: serde_json::Map<String, serde_json::Value> = projection
                    .iter()
                    .filter_map(|f| {
                        row.iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case(f))
                            .map(|(k, v)| (k.clone(), v.clone()))
                    })
                    .collect();
                rows.push(TableRow { values: projected });
                if rows.len() >= request.max_rows {
                    break;
                }
            }
        }
        Ok(rows)
    }

    async fn table_structure(&self, table: &str) -> ErpResult<TableStructure> {
        let _p = self.pool.acquire().await?;
        self.tables
            .get(table)
            .map(|t| t.structure.clone())
            .ok_or_else(|| ErpError::NotFound(table.into()))
    }

    fn pool_status(&self) -> PoolStatus {
        PoolStatus {
            cap: self.pool.cap(),
            available: self.pool.available(),
        }
    }
}

fn mock_outputs(
    meta: &ErpOperationMeta,
    _args: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for p in &meta.parameters {
        if p.direction == ErpParamDirection::Export {
            out.insert(
                p.name.clone(),
                serde_json::Value::String(format!("<mock {}>", p.type_token)),
            );
        }
    }
    serde_json::Value::Object(out)
}

/// Parse "FIELD = 'value'" / "FIELD LIKE 'pattern'" into (field, op, value).
fn parse_conditions(raw: &[String]) -> ErpResult<Vec<(String, String, String)>> {
    let mut out = Vec::new();
    for s in raw {
        let trimmed = s.trim();
        // Supported operators: = , LIKE
        let (field, op, val) = if let Some(idx) = trimmed.to_uppercase().find(" LIKE ") {
            let f = trimmed[..idx].trim().to_string();
            let v = trimmed[idx + 6..].trim().trim_matches('\'').to_string();
            (f, "LIKE".into(), v)
        } else if let Some(idx) = trimmed.find('=') {
            let f = trimmed[..idx].trim().to_string();
            let v = trimmed[idx + 1..].trim().trim_matches('\'').to_string();
            (f, "=".into(), v)
        } else {
            return Err(ErpError::InvalidParameter {
                name: "where_conditions".into(),
                reason: format!(
                    "unsupported clause '{s}' (expected FIELD = 'value' or FIELD LIKE 'pattern')"
                ),
            });
        };
        out.push((field, op, val));
    }
    Ok(out)
}

fn match_row(
    row: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    op: &str,
    value: &str,
) -> bool {
    let actual = row
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(field))
        .map(|(_, v)| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    match op {
        "=" => actual.eq_ignore_ascii_case(value),
        "LIKE" => sql_like(&actual, value),
        _ => false,
    }
}

fn sql_like(haystack: &str, pattern: &str) -> bool {
    let h = haystack.to_lowercase();
    let p = pattern.to_lowercase();
    // Translate '%' -> '.*' and '_' -> '.' minimally.
    let mut re = String::with_capacity(p.len() + 4);
    re.push('^');
    for c in p.chars() {
        match c {
            '%' => re.push_str(".*"),
            '_' => re.push('.'),
            c if "\\.+?^${}()|[]".contains(c) => {
                re.push('\\');
                re.push(c);
            }
            c => re.push(c),
        }
    }
    re.push('$');
    // Cheap substring fallback if pattern has no wildcards.
    if !p.contains('%') && !p.contains('_') {
        return h == p;
    }
    // Without a regex crate, we approximate %prefix% and prefix% / %suffix.
    let stripped: String = re
        .chars()
        .filter(|c| !matches!(c, '^' | '$' | '\\'))
        .collect();
    if let Some(rest) = stripped.strip_prefix(".*") {
        let rest = rest.strip_suffix(".*").unwrap_or(rest);
        h.contains(rest)
    } else if let Some(prefix) = stripped.strip_suffix(".*") {
        h.starts_with(prefix)
    } else {
        h == stripped
    }
}

// ===========================================================================
// Fixtures
// ===========================================================================

// ---------------------------------------------------------------------------
// Oracle operation signatures — sourced from the Oracle REST API catalog / DDIC.
// Every write REST operation carries `commit_required: true` because the standard Oracle
// convention is that BAPIs do NOT commit on their own; the caller must
// follow up with the EBS commit op to persist (paper §VII-F note;
// confirmed in Oracle Help documentation).
// ---------------------------------------------------------------------------

fn p_imp(name: &str, ty: &str, opt: bool, desc: &str) -> ErpParameter {
    ErpParameter {
        name: name.into(),
        direction: ErpParamDirection::Import,
        type_token: ty.into(),
        optional: opt,
        description: if desc.is_empty() {
            None
        } else {
            Some(desc.into())
        },
        default_value: None,
    }
}
fn p_exp(name: &str, ty: &str, opt: bool, desc: &str) -> ErpParameter {
    ErpParameter {
        name: name.into(),
        direction: ErpParamDirection::Export,
        type_token: ty.into(),
        optional: opt,
        description: if desc.is_empty() {
            None
        } else {
            Some(desc.into())
        },
        default_value: None,
    }
}
fn p_tab(name: &str, ty: &str, opt: bool, desc: &str) -> ErpParameter {
    ErpParameter {
        name: name.into(),
        direction: ErpParamDirection::Tables,
        type_token: ty.into(),
        optional: opt,
        description: if desc.is_empty() {
            None
        } else {
            Some(desc.into())
        },
        default_value: None,
    }
}
fn p_imp_default(name: &str, ty: &str, default: &str, desc: &str) -> ErpParameter {
    ErpParameter {
        name: name.into(),
        direction: ErpParamDirection::Import,
        type_token: ty.into(),
        optional: true,
        description: if desc.is_empty() {
            None
        } else {
            Some(desc.into())
        },
        default_value: Some(default.into()),
    }
}

fn seed_functions() -> Vec<ErpOperationMeta> {
    vec![
        // ---- System / diagnostics ----------------------------------------
        // Fusion REST exposes environment identity via the framework
        // `serverInformation` resource (the RFC_SYSTEM_INFO analog).
        ErpOperationMeta {
            function: "fusion.system.serverInformation".into(),
            description: "Retrieve Fusion environment identity (pod, release, server timezone).".into(),
            function_group: "REST Framework".into(),
            package: Some("FND".into()),
            parameters: vec![
                p_exp("SERVER_INFO", "STRUCT(ServerInformation)", false, "Environment identity (pod, release, timezone)"),
            ],
            deprecated: false, read_only: true, commit_required: false,
            authorization: vec![RequiredPrivilege::view("FND_VIEW_SERVER_INFORMATION_PRIV", "Application Diagnostics Viewer")],
            erp_note: None,
        },
        // ---- Product Hub: Item master read -------------------------------
        // GET /fscmRestApi/resources/11.13.18.05/itemsV2 — the
        // BAPI_MATERIAL_GET_DETAIL analog (read item / material master).
        ErpOperationMeta {
            function: "fusion.scm.itemsV2.get".into(),
            description: "Read Product Hub item master detail (item master). Read-only Fusion REST GET.".into(),
            function_group: "Product Management".into(),
            package: Some("EGP".into()),
            parameters: vec![
                p_imp("ITEM_NUMBER", "VARCHAR2(300)", false, "Item number — VARCHAR2(300) in Fusion"),
                p_imp("ORGANIZATION_CODE", "VARCHAR2(18)", true, "Inventory organization code (org-specific view)"),
                p_imp_default("EXPAND", "VARCHAR2(240)", "", "Child resources to expand (e.g. ItemEFF)"),
                p_exp("ITEM", "STRUCT(itemsV2-item-response)", false, "Item resource representation (EGP_SYSTEM_ITEMS_B)"),
            ],
            deprecated: false, read_only: true, commit_required: false,
            authorization: vec![RequiredPrivilege::view("EGP_VIEW_ITEM_PRIV", "Product Hub Inquiry")],
            erp_note: Some("Item number is VARCHAR2(300) in Fusion Product Hub (EGP_SYSTEM_ITEMS_B.ITEM_NUMBER)".into()),
        },
        // ---- Financials/GL: Post journal (synchronous REST) --------------
        // POST /fscmRestApi/resources/.../journalEntries — synchronous,
        // auto-commits per request (no separate commit call).
        ErpOperationMeta {
            function: "fusion.gl.journalEntries.post".into(),
            description: "Create and post a GL journal entry synchronously via Fusion REST.".into(),
            function_group: "Financials/General Ledger".into(),
            package: Some("GL".into()),
            parameters: vec![
                p_imp("JOURNAL_ENTRY", "STRUCT(journalEntries-request)", false, "Journal header + lines payload"),
                p_imp_default("POST_TO_LEDGER", "VARCHAR2(1)", "Y", "If Y, post immediately after create"),
                p_exp("JOURNAL", "STRUCT(journalEntries-response)", false, "Created journal (with JeHeaderId)"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "FND_API status S/E/U (Fusion REST mirrors via HTTP status + error payload)"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::manage("GL_CREATE_JOURNAL_PRIV", "General Accounting Manager")],
            erp_note: Some("Synchronous Fusion REST write — auto-commits per request. Posting lands in GL_JE_LINES (and, for subledger sources, XLA_AE_LINES). Oracle has no single 'universal journal'.".into()),
        },
        // ---- ERP Integration Service: FBDI Journal Import (bulk) ---------
        // POST /fscmRestApi/resources/.../erpintegrations (importBulkData)
        // — loads a FBDI zip to GL_INTERFACE then runs the Journal Import
        // ESS job. Two-step: stage to interface, then import (commit-like).
        ErpOperationMeta {
            function: "fusion.erpintegrations.importBulkData.journalImport".into(),
            description: "Bulk-load journals via FBDI (Journal Import). Stages to GL_INTERFACE then submits the Journal Import job.".into(),
            function_group: "ERP Integration Service".into(),
            package: Some("FUN".into()),
            parameters: vec![
                p_imp("DOCUMENT_CONTENT", "CLOB(base64)", false, "Base64 FBDI zip built from the JournalImportTemplate"),
                p_imp("JOB_NAME", "VARCHAR2(240)", false, "ESS job path/name, e.g. /oracle/apps/ess/financials/.../JournalImportLauncher"),
                p_imp_default("CALLBACK_URL", "VARCHAR2(2000)", "", "Optional async completion callback"),
                p_exp("REQUEST_ID", "NUMBER", false, "ESS request id for status polling"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Submission status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: true,
            authorization: vec![RequiredPrivilege::run("GL_RUN_JOURNAL_IMPORT_PRIV", "General Accounting Manager")],
            erp_note: Some("Two-step bulk write: FBDI document → GL_INTERFACE → Journal Import ESS job. Nothing posts until the import job completes — this is the interface-then-import two-phase write.".into()),
        },
        // ---- Procurement: Create purchase order --------------------------
        ErpOperationMeta {
            function: "fusion.po.purchaseOrders.post".into(),
            description: "Create a purchase order via Fusion REST (synchronous).".into(),
            function_group: "Procurement".into(),
            package: Some("PO".into()),
            parameters: vec![
                p_imp("PURCHASE_ORDER", "STRUCT(purchaseOrders-request)", false, "PO header + lines + distributions"),
                p_imp_default("DRAFT", "VARCHAR2(1)", "N", "If Y, create as draft (draftPurchaseOrders)"),
                p_exp("PURCHASE_ORDER_RESULT", "STRUCT(purchaseOrders-response)", false, "Created PO (with POHeaderId / OrderNumber)"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::manage("PO_MANAGE_PURCHASE_ORDER_PRIV", "Procurement Manager")],
            erp_note: Some("Synchronous REST; auto-commits. Charge-account distributions are validated against open GL periods (GL_PERIOD_STATUSES) before approval.".into()),
        },
        // ---- Order Management: Order import ------------------------------
        ErpOperationMeta {
            function: "fusion.doo.salesOrdersForOrderHub.post".into(),
            description: "Import a sales order into Order Management (Order Hub).".into(),
            function_group: "Order Management".into(),
            package: Some("DOO".into()),
            parameters: vec![
                p_imp("SALES_ORDER", "STRUCT(salesOrdersForOrderHub-request)", false, "Order header + lines"),
                p_exp("SALES_ORDER_RESULT", "STRUCT(salesOrdersForOrderHub-response)", false, "Created order (with HeaderId / OrderNumber)"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::manage("DOO_MANAGE_SALES_ORDER_PRIV", "Order Entry Specialist")],
            erp_note: Some("Sold-to is a TCA party (BUYING_PARTY_ID).".into()),
        },
        // ---- Inventory: Receiving (goods receipt) ------------------------
        ErpOperationMeta {
            function: "fusion.inv.receivingReceiptRequests.post".into(),
            description: "Create a receiving receipt (goods receipt against a PO).".into(),
            function_group: "Inventory Management".into(),
            package: Some("RCV".into()),
            parameters: vec![
                p_imp("RECEIPT_REQUEST", "STRUCT(receivingReceiptRequests-request)", false, "Receipt header + lines"),
                p_exp("RECEIPT_RESULT", "STRUCT(receivingReceiptRequests-response)", false, "Created receipt"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::manage("RCV_MANAGE_RECEIPT_PRIV", "Receiving Agent")],
            erp_note: Some("Receipt creates inventory + accrual accounting events in XLA that later transfer to GL_JE_LINES via Create Accounting.".into()),
        },
        // ---- Suppliers: Master change ------------------------------------
        // PATCH /fscmRestApi/resources/.../suppliers — the customer/vendor
        // master change analog (TCA-backed).
        ErpOperationMeta {
            function: "fusion.poz.suppliers.patch".into(),
            description: "Update supplier master data (TCA party + supplier profile).".into(),
            function_group: "Procurement".into(),
            package: Some("POZ".into()),
            parameters: vec![
                p_imp("SUPPLIER_ID", "NUMBER", false, "SupplierId (TCA-backed)"),
                p_imp("SUPPLIER_PATCH", "STRUCT(suppliers-request)", false, "Fields to change"),
                p_exp("SUPPLIER_RESULT", "STRUCT(suppliers-response)", false, "Updated supplier"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::manage("POZ_MANAGE_SUPPLIER_PROFILE_PRIV", "Supplier Administrator")],
            erp_note: Some("Suppliers and customers are TCA parties; suppliers and customers are modelled as Oracle Trading Community Architecture parties.".into()),
        },
        // ---- Configuration: Publish sandbox (change promotion) -----------
        // The change-promotion action. Publishing a sandbox merges its
        // metadata to the mainline; promotion to PROD is high-stakes and
        // guarded by a re-typed confirmation in the workflow layer.
        ErpOperationMeta {
            function: "fusion.fnd.sandbox.publish".into(),
            description: "Publish a configuration sandbox to the mainline (change promotion; the transport-release analog).".into(),
            function_group: "Configuration".into(),
            package: Some("FND".into()),
            parameters: vec![
                p_imp("SANDBOX_NAME", "VARCHAR2(80)", false, "Sandbox to publish"),
                p_imp_default("PUBLISH_TARGET", "VARCHAR2(30)", "MAINLINE", "Target: MAINLINE (then promoted to a target pod via config-package export/import)"),
                p_exp("PUBLISH_RESULT", "STRUCT(sandbox-publish-response)", false, "Publish outcome"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::manage("FND_MANAGE_SANDBOX_PRIV", "Application Implementation Consultant")],
            erp_note: Some("Publishing is irreversible against the mainline. Cross-pod promotion uses FSM Configuration Packages (export → import). Guard PROD publishes with a re-typed confirmation.".into()),
        },
        // ---- BI Publisher: tabular extract (a BI Publisher extract analog) -------
        ErpOperationMeta {
            function: "fusion.bip.runReport".into(),
            description: "Run a BI Publisher report to extract tabular data (the a BI Publisher extract analog). Prefer OTBI for ad-hoc analytics.".into(),
            function_group: "BI Publisher".into(),
            package: Some("XDO".into()),
            parameters: vec![
                p_imp("REPORT_PATH", "VARCHAR2(2000)", false, "Catalog path, e.g. /Custom/Gaussian Technologies/GL/JournalExtract.xdo"),
                p_imp_default("PARAMETERS", "STRUCT(bip-parameters)", "", "Report parameters"),
                p_imp_default("SIZE_OF_DATA_CHUNK", "NUMBER", "-1", "Chunking for large extracts"),
                p_exp("REPORT_BYTES", "CLOB(base64)", false, "Report output (XML/CSV)"),
            ],
            deprecated: false, read_only: true, commit_required: false,
            authorization: vec![RequiredPrivilege::run("FND_RUN_BIP_REPORT_PRIV", "BI Administrator")],
            erp_note: Some("BI Publisher is the supported path for bulk tabular extracts from Fusion (no direct table SELECT). Bound the result set; use OTBI subject areas for analytics.".into()),
        },
        // ---- REST describe (DDIF_FIELDINFO_GET analog) -------------------
        ErpOperationMeta {
            function: "fusion.rest.describe".into(),
            description: "Describe a Fusion REST resource's attributes/types (the DDIF_FIELDINFO_GET analog).".into(),
            function_group: "REST Framework".into(),
            package: Some("FND".into()),
            parameters: vec![
                p_imp("RESOURCE", "VARCHAR2(240)", false, "Resource name, e.g. itemsV2"),
                p_imp_default("MODE", "VARCHAR2(20)", "all", "describe mode (all / dataOnly)"),
                p_exp("DESCRIBE", "STRUCT(describe-response)", false, "Attribute + link metadata"),
            ],
            deprecated: false, read_only: true, commit_required: false,
            authorization: vec![RequiredPrivilege::view("FND_VIEW_REST_DESCRIBE_PRIV", "Integration Specialist")],
            erp_note: None,
        },
        // ---- EBS transaction control (on-prem two-phase commit) ---------
        // Oracle EBS PL/SQL public APIs follow the FND_API standard: the
        // caller decides persistence via p_commit, then issues COMMIT /
        // ROLLBACK. Fusion REST writes auto-commit per request, so these
        // ops are only used on the on-prem EBS backend (and by the write
        // orchestrator to finalize / cancel a logical unit of work).
        ErpOperationMeta {
            function: "ebs.fnd.transaction.commit".into(),
            description: "Commit the current EBS database transaction (FND_API p_commit / COMMIT).".into(),
            function_group: "EBS Transaction Control".into(),
            package: Some("FND".into()),
            parameters: vec![
                p_imp_default("WAIT", "VARCHAR2(1)", "X", "If X, commit synchronously"),
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::run("FND_COMMIT_TRANSACTION_PRIV", "Application Developer")],
            erp_note: Some("EBS-only. Fusion REST auto-commits per request; this is the on-prem two-phase finalize.".into()),
        },
        ErpOperationMeta {
            function: "ebs.fnd.transaction.rollback".into(),
            description: "Roll back the current EBS database transaction (FND_API ROLLBACK).".into(),
            function_group: "EBS Transaction Control".into(),
            package: Some("FND".into()),
            parameters: vec![
                p_exp("X_RETURN_STATUS", "VARCHAR2(1)", false, "Status S/E/U"),
                p_exp("X_MSG_COUNT", "NUMBER", true, "Message count"),
                p_tab("X_MSG_DATA", "STRUCT(FND_MSG)", false, "FND_MSG_PUB message stack"),
            ],
            deprecated: false, read_only: false, commit_required: false,
            authorization: vec![RequiredPrivilege::run("FND_ROLLBACK_TRANSACTION_PRIV", "Application Developer")],
            erp_note: Some("EBS-only rollback used by the write orchestrator on failure or an unconfirmed outcome.".into()),
        },
    ]
}

fn tf_key(name: &str, data_element: &str, ty: &str, length: u32, desc: &str) -> TableField {
    TableField {
        name: name.into(),
        data_element: data_element.into(),
        type_token: ty.into(),
        length,
        description: Some(desc.into()),
        key: true,
    }
}
fn tf(name: &str, data_element: &str, ty: &str, length: u32, desc: &str) -> TableField {
    TableField {
        name: name.into(),
        data_element: data_element.into(),
        type_token: ty.into(),
        length,
        description: Some(desc.into()),
        key: false,
    }
}

fn seed_tables() -> Vec<MockTable> {
    vec![
        // ---- EGP_SYSTEM_ITEMS_B — Product Hub item master ---------------
        // Item number is VARCHAR2(300) in Fusion.
        MockTable {
            structure: TableStructure {
                table: "EGP_SYSTEM_ITEMS_B".into(),
                description: "Product Hub item master (base)".into(),
                key_fields: vec!["INVENTORY_ITEM_ID".into(), "ORGANIZATION_ID".into()],
                fields: vec![
                    tf_key("INVENTORY_ITEM_ID", "NUMBER", "NUMBER", 18, "Item surrogate key"),
                    tf_key("ORGANIZATION_ID", "NUMBER", "NUMBER", 18, "Inventory organization id"),
                    tf("ITEM_NUMBER", "VARCHAR2", "VARCHAR2", 300, "Item number — VARCHAR2(300) in Fusion"),
                    tf("ITEM_DESCRIPTION", "VARCHAR2", "VARCHAR2", 240, "Item description"),
                    tf("ITEM_CLASS", "VARCHAR2", "VARCHAR2", 80, "Item class (catalog)"),
                    tf("PRIMARY_UOM_CODE", "VARCHAR2", "VARCHAR2", 3, "Primary unit of measure"),
                    tf("ITEM_STATUS_CODE", "VARCHAR2", "VARCHAR2", 10, "Lifecycle status (Active/Inactive)"),
                    tf("CREATION_DATE", "DATE", "DATE", 7, "Created on"),
                    tf("CREATED_BY", "VARCHAR2", "VARCHAR2", 64, "Created by (user)"),
                ],
                authorization_group: "EGP_ITEM_DATA".into(),
                storage_note: None,
            },
            rows: vec![
                row(&[("INVENTORY_ITEM_ID","300100001"),("ORGANIZATION_ID","204"),("ITEM_NUMBER","GT-COMP-GPU-A100"),("ITEM_DESCRIPTION","GPU Compute Module A100"),("ITEM_CLASS","COMPONENT"),("PRIMARY_UOM_CODE","EA"),("ITEM_STATUS_CODE","Active"),("CREATION_DATE","2024-09-01"),("CREATED_BY","GT_DEV")]),
                row(&[("INVENTORY_ITEM_ID","300100002"),("ORGANIZATION_ID","204"),("ITEM_NUMBER","GT-FG-EDGE-NODE"),("ITEM_DESCRIPTION","Gaussian Edge Node (finished good)"),("ITEM_CLASS","FINISHED_GOOD"),("PRIMARY_UOM_CODE","EA"),("ITEM_STATUS_CODE","Active"),("CREATION_DATE","2024-09-15"),("CREATED_BY","GT_DEV")]),
                row(&[("INVENTORY_ITEM_ID","300100003"),("ORGANIZATION_ID","207"),("ITEM_NUMBER","GT-TRADE-SENSOR-KIT"),("ITEM_DESCRIPTION","IoT Sensor Kit (trade)"),("ITEM_CLASS","TRADE_GOOD"),("PRIMARY_UOM_CODE","EA"),("ITEM_STATUS_CODE","Active"),("CREATION_DATE","2025-10-01"),("CREATED_BY","GT_DEV")]),
            ],
        },
        // ---- GL_LEDGERS — ledgers -------------
        MockTable {
            structure: TableStructure {
                table: "GL_LEDGERS".into(),
                description: "General Ledger ledgers".into(),
                key_fields: vec!["LEDGER_ID".into()],
                fields: vec![
                    tf_key("LEDGER_ID", "NUMBER", "NUMBER", 18, "Ledger surrogate key"),
                    tf("NAME", "VARCHAR2", "VARCHAR2", 30, "Ledger name"),
                    tf("CURRENCY_CODE", "VARCHAR2", "VARCHAR2", 15, "Ledger currency (ISO 4217)"),
                    tf("CHART_OF_ACCOUNTS_ID", "NUMBER", "NUMBER", 18, "Chart of accounts (key flexfield structure)"),
                    tf("PERIOD_SET_NAME", "VARCHAR2", "VARCHAR2", 15, "Accounting calendar"),
                    tf("LEDGER_CATEGORY_CODE", "VARCHAR2", "VARCHAR2", 30, "PRIMARY / SECONDARY / ALC"),
                    tf("LEGAL_ENTITY_ID", "NUMBER", "NUMBER", 18, "Default legal entity (XLE_ENTITY_PROFILES)"),
                ],
                authorization_group: "GL_LEDGER_DATA".into(),
                storage_note: Some("Company-code analog. Company code maps onto Ledger + Legal Entity; data access is scoped by Data Access Set, not a tenant client.".into()),
            },
            rows: vec![
                row(&[("LEDGER_ID","300100001"),("NAME","Gaussian Technologies Primary Ledger"),("CURRENCY_CODE","IDR"),("CHART_OF_ACCOUNTS_ID","101"),("PERIOD_SET_NAME","GAUSSIAN_FISCAL"),("LEDGER_CATEGORY_CODE","PRIMARY"),("LEGAL_ENTITY_ID","500001")]),
                row(&[("LEDGER_ID","300100002"),("NAME","Gaussian Technologies USD Reporting"),("CURRENCY_CODE","USD"),("CHART_OF_ACCOUNTS_ID","101"),("PERIOD_SET_NAME","GAUSSIAN_FISCAL"),("LEDGER_CATEGORY_CODE","ALC"),("LEGAL_ENTITY_ID","500001")]),
            ],
        },
        // ---- GL_PERIOD_STATUSES — accounting periods (T001B analog) -----
        MockTable {
            structure: TableStructure {
                table: "GL_PERIOD_STATUSES".into(),
                description: "Accounting period open/close status per ledger + application".into(),
                key_fields: vec!["LEDGER_ID".into(), "APPLICATION_ID".into(), "PERIOD_NAME".into()],
                fields: vec![
                    tf_key("LEDGER_ID", "NUMBER", "NUMBER", 18, "Ledger"),
                    tf_key("APPLICATION_ID", "NUMBER", "NUMBER", 18, "Owning application (101 = GL)"),
                    tf_key("PERIOD_NAME", "VARCHAR2", "VARCHAR2", 15, "Period name, e.g. MAR-26"),
                    tf("CLOSING_STATUS", "VARCHAR2", "VARCHAR2", 1, "O=Open, C=Closed, P=Permanently closed, F=Future, N=Never opened"),
                    tf("PERIOD_YEAR", "NUMBER", "NUMBER", 15, "Fiscal year"),
                    tf("PERIOD_NUM", "NUMBER", "NUMBER", 15, "Period number within the year"),
                    tf("START_DATE", "DATE", "DATE", 7, "Period start"),
                    tf("END_DATE", "DATE", "DATE", 7, "Period end"),
                ],
                authorization_group: "GL_PERIOD_DATA".into(),
                storage_note: Some("Managed via 'Manage Accounting Periods'. CLOSING_STATUS drives whether a journal can post — the posting-period gate.".into()),
            },
            rows: vec![
                row(&[("LEDGER_ID","300100001"),("APPLICATION_ID","101"),("PERIOD_NAME","MAR-26"),("CLOSING_STATUS","C"),("PERIOD_YEAR","2026"),("PERIOD_NUM","3"),("START_DATE","2026-03-01"),("END_DATE","2026-03-31")]),
                row(&[("LEDGER_ID","300100001"),("APPLICATION_ID","101"),("PERIOD_NAME","APR-26"),("CLOSING_STATUS","O"),("PERIOD_YEAR","2026"),("PERIOD_NUM","4"),("START_DATE","2026-04-01"),("END_DATE","2026-04-30")]),
            ],
        },
        // ---- GL_JE_LINES — GL journal lines (BSEG/GL_JE_LINES GL leg) --------
        MockTable {
            structure: TableStructure {
                table: "GL_JE_LINES".into(),
                description: "General Ledger journal entry lines".into(),
                key_fields: vec!["JE_HEADER_ID".into(), "JE_LINE_NUM".into()],
                fields: vec![
                    tf_key("JE_HEADER_ID", "NUMBER", "NUMBER", 18, "Journal header id"),
                    tf_key("JE_LINE_NUM", "NUMBER", "NUMBER", 18, "Journal line number"),
                    tf("LEDGER_ID", "NUMBER", "NUMBER", 18, "Ledger"),
                    tf("CODE_COMBINATION_ID", "NUMBER", "NUMBER", 18, "Account (GL_CODE_COMBINATIONS key flexfield)"),
                    tf("PERIOD_NAME", "VARCHAR2", "VARCHAR2", 15, "Accounting period"),
                    tf("EFFECTIVE_DATE", "DATE", "DATE", 7, "Accounting date"),
                    tf("ENTERED_DR", "NUMBER", "NUMBER", 38, "Entered debit (txn currency)"),
                    tf("ENTERED_CR", "NUMBER", "NUMBER", 38, "Entered credit (txn currency)"),
                    tf("ACCOUNTED_DR", "NUMBER", "NUMBER", 38, "Accounted debit (ledger currency)"),
                    tf("ACCOUNTED_CR", "NUMBER", "NUMBER", 38, "Accounted credit (ledger currency)"),
                    tf("CURRENCY_CODE", "VARCHAR2", "VARCHAR2", 15, "Transaction currency"),
                ],
                authorization_group: "GL_JOURNAL_DATA".into(),
                storage_note: Some("The GL leg of Oracle's accounting backbone (the GL accounting backbone). Oracle has NO single universal journal: GL_JE_LINES holds GL detail while subledger detail lives in XLA_AE_LINES and balances in GL_BALANCES.".into()),
            },
            rows: vec![
                row(&[("JE_HEADER_ID","700100123"),("JE_LINE_NUM","1"),("LEDGER_ID","300100001"),("CODE_COMBINATION_ID","12345"),("PERIOD_NAME","MAR-26"),("EFFECTIVE_DATE","2026-03-15"),("ENTERED_DR","22500000"),("ENTERED_CR","0"),("ACCOUNTED_DR","22500000"),("ACCOUNTED_CR","0"),("CURRENCY_CODE","IDR")]),
                row(&[("JE_HEADER_ID","700100123"),("JE_LINE_NUM","2"),("LEDGER_ID","300100001"),("CODE_COMBINATION_ID","12399"),("PERIOD_NAME","MAR-26"),("EFFECTIVE_DATE","2026-03-15"),("ENTERED_DR","0"),("ENTERED_CR","22500000"),("ACCOUNTED_DR","0"),("ACCOUNTED_CR","22500000"),("CURRENCY_CODE","IDR")]),
            ],
        },
        // ---- XLA_AE_LINES — Subledger Accounting lines ------------------
        MockTable {
            structure: TableStructure {
                table: "XLA_AE_LINES".into(),
                description: "Subledger Accounting (SLA) accounting entry lines".into(),
                key_fields: vec!["AE_HEADER_ID".into(), "AE_LINE_NUM".into()],
                fields: vec![
                    tf_key("AE_HEADER_ID", "NUMBER", "NUMBER", 18, "Subledger accounting header id"),
                    tf_key("AE_LINE_NUM", "NUMBER", "NUMBER", 18, "Accounting line number"),
                    tf("LEDGER_ID", "NUMBER", "NUMBER", 18, "Ledger"),
                    tf("CODE_COMBINATION_ID", "NUMBER", "NUMBER", 18, "Account"),
                    tf("ACCOUNTING_CLASS_CODE", "VARCHAR2", "VARCHAR2", 30, "Accounting class (e.g. ITEM_EXPENSE, LIABILITY)"),
                    tf("ENTERED_DR", "NUMBER", "NUMBER", 38, "Entered debit"),
                    tf("ENTERED_CR", "NUMBER", "NUMBER", 38, "Entered credit"),
                    tf("ACCOUNTED_DR", "NUMBER", "NUMBER", 38, "Accounted debit"),
                    tf("ACCOUNTED_CR", "NUMBER", "NUMBER", 38, "Accounted credit"),
                    tf("CURRENCY_CODE", "VARCHAR2", "VARCHAR2", 15, "Currency"),
                    tf("GL_TRANSFER_STATUS_CODE", "VARCHAR2", "VARCHAR2", 1, "Y/N/NT — transferred to GL?"),
                ],
                authorization_group: "XLA_JOURNAL_DATA".into(),
                storage_note: Some("Subledger detail (AP/AR/Costing/etc.). The 'Create Accounting' and 'Transfer to GL' (Transfer Journal Entries to GL) programs roll XLA_AE_LINES up into GL_JE_LINES — Oracle's two-tier accounting in place of a single universal journal.".into()),
            },
            rows: vec![
                row(&[("AE_HEADER_ID","900100050"),("AE_LINE_NUM","1"),("LEDGER_ID","300100001"),("CODE_COMBINATION_ID","12345"),("ACCOUNTING_CLASS_CODE","ITEM_EXPENSE"),("ENTERED_DR","22500000"),("ENTERED_CR","0"),("ACCOUNTED_DR","22500000"),("ACCOUNTED_CR","0"),("CURRENCY_CODE","IDR"),("GL_TRANSFER_STATUS_CODE","Y")]),
            ],
        },
        // ---- DOO_HEADERS_ALL — sales order header (VBAK analog) ---------
        MockTable {
            structure: TableStructure {
                table: "DOO_HEADERS_ALL".into(),
                description: "Order Management: order header".into(),
                key_fields: vec!["HEADER_ID".into()],
                fields: vec![
                    tf_key("HEADER_ID", "NUMBER", "NUMBER", 18, "Order header id"),
                    tf("ORDER_NUMBER", "VARCHAR2", "VARCHAR2", 50, "Order number"),
                    tf("BUYING_PARTY_ID", "NUMBER", "NUMBER", 18, "Sold-to TCA party"),
                    tf("TRANSACTIONAL_CURR_CODE", "VARCHAR2", "VARCHAR2", 15, "Order currency"),
                    tf("ORDERED_DATE", "DATE", "DATE", 7, "Order date"),
                    tf("STATUS_CODE", "VARCHAR2", "VARCHAR2", 30, "Order status"),
                    tf("BUSINESS_UNIT_ID", "NUMBER", "NUMBER", 18, "Owning business unit"),
                    tf("TOTAL_AMOUNT", "NUMBER", "NUMBER", 38, "Order net amount"),
                ],
                authorization_group: "DOO_ORDER_DATA".into(),
                storage_note: Some("Fusion Order Management. Data access is scoped by Business Unit (BUSINESS_UNIT_ID), the Business-Unit scoping model.".into()),
            },
            rows: vec![
                row(&[("HEADER_ID","400100501"),("ORDER_NUMBER","GT-SO-5001"),("BUYING_PARTY_ID","600100"),("TRANSACTIONAL_CURR_CODE","IDR"),("ORDERED_DATE","2026-01-12"),("STATUS_CODE","OPEN"),("BUSINESS_UNIT_ID","204"),("TOTAL_AMOUNT","187500000")]),
                row(&[("HEADER_ID","400100502"),("ORDER_NUMBER","GT-SO-5002"),("BUYING_PARTY_ID","600100"),("TRANSACTIONAL_CURR_CODE","IDR"),("ORDERED_DATE","2026-01-15"),("STATUS_CODE","CLOSED"),("BUSINESS_UNIT_ID","204"),("TOTAL_AMOUNT","134850000")]),
                row(&[("HEADER_ID","400100503"),("ORDER_NUMBER","GT-SO-5003"),("BUYING_PARTY_ID","600200"),("TRANSACTIONAL_CURR_CODE","USD"),("ORDERED_DATE","2026-01-20"),("STATUS_CODE","OPEN"),("BUSINESS_UNIT_ID","207"),("TOTAL_AMOUNT","45000")]),
            ],
        },
        // ---- FND_SANDBOXES — config sandboxes (E070 transport analog) ---
        MockTable {
            structure: TableStructure {
                table: "FND_SANDBOXES".into(),
                description: "Configuration sandboxes (change-promotion unit; change-promotion unit)".into(),
                key_fields: vec!["SANDBOX_ID".into()],
                fields: vec![
                    tf_key("SANDBOX_ID", "NUMBER", "NUMBER", 18, "Sandbox id"),
                    tf("SANDBOX_NAME", "VARCHAR2", "VARCHAR2", 80, "Sandbox name"),
                    tf("STATUS", "VARCHAR2", "VARCHAR2", 30, "ACTIVE / PUBLISHED / DELETED"),
                    tf("CREATED_BY", "VARCHAR2", "VARCHAR2", 64, "Owner"),
                    tf("CREATION_DATE", "DATE", "DATE", 7, "Created on"),
                    tf("PUBLISH_TARGET", "VARCHAR2", "VARCHAR2", 30, "MAINLINE / target pod for config-package promotion"),
                ],
                authorization_group: "FND_SANDBOX_DATA".into(),
                storage_note: Some("Oracle's change-promotion unit. Publishing merges to MAINLINE; cross-pod promotion uses FSM Configuration Packages. This is the change-promotion unit.".into()),
            },
            rows: vec![
                row(&[("SANDBOX_ID","800100001"),("SANDBOX_NAME","GT_AR_AUTOINVOICE_FIX"),("STATUS","ACTIVE"),("CREATED_BY","GT_DEV"),("CREATION_DATE","2026-03-18"),("PUBLISH_TARGET","MAINLINE")]),
            ],
        },
    ]
}

fn row(pairs: &[(&str, &str)]) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    for (k, v) in pairs {
        m.insert((*k).into(), serde_json::Value::String((*v).into()));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Oracle-correctness invariants.  These tests enforce the rules that
    // hold across the Oracle Fusion Cloud ERP operation/object catalogue,
    // so any drift in our fixtures fails CI loudly (see docs/ORACLE_CORRECTNESS.md).
    // -----------------------------------------------------------------

    /// Every write operation must surface the FND standard return contract
    /// (`X_RETURN_STATUS` + the `X_MSG_DATA` FND_MSG_PUB stack) so agents
    /// can inspect business-side messages — the Oracle equivalent of the legacy
    /// "every write REST operation returns FND return stack" rule.
    #[test]
    fn every_write_op_returns_standard_result() {
        for f in seed_functions() {
            if f.read_only {
                continue;
            }
            let has_status = f
                .parameters
                .iter()
                .any(|p| p.direction == ErpParamDirection::Export && p.name == "X_RETURN_STATUS");
            let has_msg = f
                .parameters
                .iter()
                .any(|p| p.direction == ErpParamDirection::Tables && p.name == "X_MSG_DATA");
            assert!(
                has_status && has_msg,
                "write op {} must declare X_RETURN_STATUS (export) + X_MSG_DATA (tables) \
                 — the FND_MSG_PUB return contract",
                f.function
            );
        }
    }

    /// Bulk/interface writes (FBDI → interface → import) are the only ops
    /// that defer persistence to a follow-up job; synchronous Fusion REST
    /// writes auto-commit. `commit_required` must therefore line up exactly
    /// with the ERP Integration Service family.
    #[test]
    fn every_bulk_write_uses_interface_then_import() {
        for f in seed_functions() {
            if f.read_only {
                continue;
            }
            let is_bulk = f.function_group == "ERP Integration Service";
            assert_eq!(
                f.commit_required, is_bulk,
                "op {}: commit_required ({}) must match the ERP Integration Service \
                 (bulk interface→import) family membership ({})",
                f.function, f.commit_required, is_bulk
            );
            if f.commit_required {
                let note = f.erp_note.as_deref().unwrap_or("").to_lowercase();
                assert!(
                    note.contains("interface") || note.contains("import"),
                    "bulk op {} must document its interface/import two-step in erp_note",
                    f.function
                );
            }
        }
    }

    /// Every operation declares at least one Oracle RBAC privilege (the
    /// S_RFC-authorization analog), and privileges are non-empty.
    #[test]
    fn every_op_declares_required_privilege() {
        for f in seed_functions() {
            assert!(
                !f.authorization.is_empty(),
                "op {} declares no required privilege",
                f.function
            );
            for p in &f.authorization {
                assert!(
                    !p.privilege.is_empty() && !p.duty_role.is_empty(),
                    "op {} has a malformed privilege entry",
                    f.function
                );
            }
        }
    }

    /// Oracle is not client-first (no client column). Instead every
    /// business object is keyed by an Oracle surrogate/scoping id ending in
    /// `_ID`. This replaces the legacy "client as first key" invariant.
    #[test]
    fn every_business_object_declares_scoping_id_key() {
        for t in seed_tables() {
            let s = &t.structure;
            assert!(!s.fields.is_empty(), "object {} has no fields", s.table);
            let first_key = s
                .key_fields
                .first()
                .unwrap_or_else(|| panic!("object {} has no key_fields", s.table));
            assert!(
                first_key.ends_with("_ID"),
                "object {} first key is {first_key}, expected an Oracle surrogate/scoping *_ID key",
                s.table
            );
            // The first key must be a declared field.
            assert!(
                s.fields.iter().any(|f| &f.name == first_key),
                "object {} first key {first_key} is not a declared field",
                s.table
            );
        }
    }

    /// The single most-cited DDIC->Oracle change: item number is
    /// VARCHAR2(300) in Fusion Product Hub, not a fixed short code.
    #[test]
    fn item_number_is_varchar2_300_per_fusion() {
        let items = seed_tables()
            .into_iter()
            .find(|t| t.structure.table == "EGP_SYSTEM_ITEMS_B")
            .unwrap();
        let num = items
            .structure
            .fields
            .iter()
            .find(|f| f.name == "ITEM_NUMBER")
            .unwrap();
        assert_eq!(num.type_token, "VARCHAR2");
        assert_eq!(
            num.length, 300,
            "ITEM_NUMBER length is {}; Fusion Product Hub uses VARCHAR2(300)",
            num.length
        );
    }

    /// GL_JE_LINES is the accounting backbone, and the fixture must record
    /// that Oracle has no single universal journal.
    #[test]
    fn gl_je_lines_is_present_as_accounting_backbone() {
        let t = seed_tables()
            .into_iter()
            .find(|t| t.structure.table == "GL_JE_LINES")
            .expect("GL_JE_LINES missing — it is Oracle's GL accounting backbone");
        let note = t
            .structure
            .storage_note
            .as_deref()
            .unwrap_or("")
            .to_lowercase();
        assert!(
            note.contains("universal journal"),
            "GL_JE_LINES must note that Oracle has no single universal journal; got: {note:?}"
        );
        // And nothing should claim to *be* a universal journal.
        for t in seed_tables() {
            let n = t
                .structure
                .storage_note
                .as_deref()
                .unwrap_or("")
                .to_lowercase();
            assert!(
                !n.contains("is the universal journal"),
                "{} must not claim to be a universal journal — Oracle has none",
                t.structure.table
            );
        }
    }

    /// Subledger objects must document the XLA → GL transfer (Create
    /// Accounting / Transfer to GL), the Oracle equivalent of the legacy
    /// compatibility-view storage note.
    #[test]
    fn subledger_objects_note_xla_to_gl_transfer() {
        let t = seed_tables()
            .into_iter()
            .find(|t| t.structure.table == "XLA_AE_LINES")
            .expect("XLA_AE_LINES fixture missing");
        let note = t
            .structure
            .storage_note
            .as_deref()
            .unwrap_or("")
            .to_lowercase();
        assert!(
            note.contains("transfer") && note.contains("gl"),
            "XLA_AE_LINES must note the transfer to GL; got: {note:?}"
        );
    }

    // ---- functional tests -------------------------------------------

    #[tokio::test]
    async fn system_info_returns_identity() {
        let c = MockErpClient::new(4, serde_json::json!({"user": "DEMO"}));
        let info = c.system_info().await.unwrap();
        assert_eq!(info.sid, "GAUSSIAN-FA-DEV");
        assert_eq!(info.client, "GAUSSIAN_PRIMARY_LEDGER");
    }

    #[tokio::test]
    async fn rfc_search_ranks_by_match() {
        let c = MockErpClient::new(4, serde_json::json!({}));
        let r = c.search_operations("item master", 5).await.unwrap();
        assert!(!r.hits.is_empty());
        assert_eq!(r.hits[0].function, "fusion.scm.itemsV2.get");
    }

    #[tokio::test]
    async fn rfc_metadata_required_param_check() {
        let c = MockErpClient::new(4, serde_json::json!({}));
        let req = ErpCallRequest {
            function: "fusion.scm.itemsV2.get".into(),
            parameters: serde_json::json!({}),
            timeout_ms: 5000,
            require_read_only_safe: true,
        };
        let err = c.call_operation(req, true).await.unwrap_err();
        assert!(
            matches!(err, ErpError::InvalidParameter { ref name, .. } if name == "ITEM_NUMBER")
        );
    }

    #[tokio::test]
    async fn rfc_call_read_only_mode_blocks_writes() {
        let c = MockErpClient::new(4, serde_json::json!({}));
        let req = ErpCallRequest {
            function: "fusion.gl.journalEntries.post".into(),
            parameters: serde_json::json!({ "JOURNAL_ENTRY": {} }),
            timeout_ms: 5000,
            require_read_only_safe: true,
        };
        let err = c.call_operation(req, true).await.unwrap_err();
        assert!(matches!(err, ErpError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn read_table_filters_and_projects() {
        let c = MockErpClient::new(4, serde_json::json!({}));
        let rows = c
            .read_table(ReadTableRequest {
                table: "GL_LEDGERS".into(),
                fields: vec!["NAME".into(), "CURRENCY_CODE".into()],
                where_conditions: vec!["CURRENCY_CODE = 'USD'".into()],
                max_rows: 10,
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].values.get("NAME").unwrap(),
            "Gaussian Technologies USD Reporting"
        );
        assert!(rows[0].values.get("CURRENCY_CODE").is_some());
        assert!(
            rows[0].values.get("LEDGER_ID").is_none(),
            "field not projected"
        );
    }

    #[tokio::test]
    async fn read_table_buffer_overflow() {
        let c = MockErpClient::new(4, serde_json::json!({}));
        let err = c
            .read_table(ReadTableRequest {
                table: "EGP_SYSTEM_ITEMS_B".into(),
                fields: vec![],
                where_conditions: vec![],
                max_rows: 9999,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ErpError::TableBufferOverflow { .. }));
    }

    #[tokio::test]
    async fn bulk_metadata_reports_missing() {
        let c = MockErpClient::new(4, serde_json::json!({}));
        let r = c
            .bulk_operation_metadata(
                &[
                    "fusion.system.serverInformation".into(),
                    "DOES_NOT_EXIST".into(),
                ],
                "EN",
            )
            .await
            .unwrap();
        assert_eq!(r.functions.len(), 1);
        assert_eq!(r.missing, vec!["DOES_NOT_EXIST".to_string()]);
    }
}
