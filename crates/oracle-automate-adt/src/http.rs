//! Live Oracle artifact client over REST/JSON.
//!
//! `HttpOicClient` implements [`AdtClient`] against the live Oracle surfaces
//! that hold custom code and configuration:
//!
//!   - **Oracle Integration Cloud (OIC)** REST API (`/ic/api/integration/v1/...`)
//!     for integrations, connections, lookups, and projects;
//!   - **BI Publisher** (`/xmlpserver/...`) for report / data-model artifacts;
//!   - **Fusion REST** (`/fscmRestApi/...`) for ESS jobs and custom resources.
//!
//! Oracle is REST/JSON-homogeneous — there is no CSRF dance and no
//! `X-SAP-Client` header — so this client is far leaner than an SAP ADT
//! client. Auth is HTTP Basic or OAuth2/IDCS bearer, selected by
//! [`AdtAuth`]. The artifact URL for each kind comes from
//! [`OracleArtifactKind::oic_path`].

use crate::client::{AdtCallContext, AdtClient};
use crate::destination::{AdtAuth, AdtDestination};
use crate::error::{AdtError, AdtResult};
use crate::types::{
    ActivationOutcome, ActivationRequest, AdtSearchHit, AdtSearchRequest, CdsView,
    OracleArtifactKind, PackageContents, PackageMember, ProgramSource, TableRow, WhereUsedHit,
    WhereUsedRequest, MAX_TABLE_ROWS,
};
use async_trait::async_trait;
use serde_json::Value;

pub struct HttpOicClient {
    destination: AdtDestination,
    http: reqwest::Client,
}

impl HttpOicClient {
    pub fn new(destination: AdtDestination) -> AdtResult<Self> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| AdtError::Internal(format!("failed to build HTTP client: {e}")))?;
        Ok(Self { destination, http })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.destination.base_url.trim_end_matches('/'), path)
    }

    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.destination.auth {
            AdtAuth::Basic { user, password } => rb.basic_auth(user, Some(password)),
            AdtAuth::Bearer { token } => rb.bearer_auth(token),
            // ServiceKey (OAuth2 client-credentials) token resolution and mTLS
            // are configured at deploy time; treat as pre-authorised here.
            _ => rb,
        }
    }

    async fn get_json(&self, path: &str) -> AdtResult<Value> {
        let url = self.url(path);
        let resp = self
            .authed(self.http.get(&url))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| self.transport_err(e))?;
        match resp.status() {
            s if s.is_success() => resp
                .json()
                .await
                .map_err(|e| AdtError::Internal(format!("invalid JSON from {url}: {e}"))),
            reqwest::StatusCode::NOT_FOUND => {
                Err(AdtError::NotFound { kind: "artifact".into(), name: path.into() })
            }
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err(AdtError::AuthFailed(format!("{} on {url}", resp.status())))
            }
            s => Err(AdtError::Internal(format!("unexpected {s} from {url}"))),
        }
    }

    fn transport_err(&self, e: reqwest::Error) -> AdtError {
        AdtError::DestinationDown {
            destination: self.destination.base_url.clone(),
            reason: e.to_string(),
        }
    }

    /// Fetch an artifact and project the JSON into a `ProgramSource`.
    async fn fetch_artifact(&self, kind: OracleArtifactKind, name: &str) -> AdtResult<ProgramSource> {
        let body = self.get_json(&kind.oic_path(name)).await?;
        let source = body
            .get("code")
            .or_else(|| body.get("content"))
            .or_else(|| body.get("source"))
            .map(value_to_text)
            .unwrap_or_else(|| serde_json::to_string_pretty(&body).unwrap_or_default());
        let description = body.get("description").and_then(|v| v.as_str()).map(String::from);
        let package = body
            .get("project")
            .or_else(|| body.get("package"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let active = body
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("ACTIVATED") || s.eq_ignore_ascii_case("ACTIVE"))
            .unwrap_or(true);
        let line_count = source.lines().count();
        Ok(ProgramSource {
            name: name.to_string(),
            kind,
            package,
            description,
            source,
            active,
            line_count,
        })
    }
}

fn value_to_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[async_trait]
impl AdtClient for HttpOicClient {
    fn destination(&self) -> &AdtDestination {
        &self.destination
    }

    async fn get_program(&self, name: &str) -> AdtResult<ProgramSource> {
        self.fetch_artifact(OracleArtifactKind::Integration, name).await
    }
    async fn get_class(&self, name: &str) -> AdtResult<ProgramSource> {
        self.fetch_artifact(OracleArtifactKind::GroovyScript, name).await
    }
    async fn get_interface(&self, name: &str) -> AdtResult<ProgramSource> {
        self.fetch_artifact(OracleArtifactKind::Connection, name).await
    }
    async fn get_include(&self, name: &str) -> AdtResult<ProgramSource> {
        self.fetch_artifact(OracleArtifactKind::Lookup, name).await
    }
    async fn get_function_module(&self, _group: &str, name: &str) -> AdtResult<ProgramSource> {
        self.fetch_artifact(OracleArtifactKind::EssJob, name).await
    }

    async fn get_package_contents(&self, package: &str) -> AdtResult<PackageContents> {
        let body = self.get_json(&OracleArtifactKind::Project.oic_path(package)).await?;
        let members = body
            .get("integrations")
            .or_else(|| body.get("members"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let name = m.get("code").or_else(|| m.get("name")).and_then(|v| v.as_str())?;
                        Some(PackageMember {
                            name: name.to_string(),
                            kind: OracleArtifactKind::Integration,
                            description: m.get("description").and_then(|v| v.as_str()).map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(PackageContents {
            package: package.to_string(),
            description: body.get("description").and_then(|v| v.as_str()).map(String::from),
            members,
        })
    }

    async fn get_cds_view(&self, name: &str) -> AdtResult<CdsView> {
        let body = self.get_json(&OracleArtifactKind::BipReport.oic_path(name)).await?;
        let source = body.get("dataModel").or_else(|| body.get("sql")).map(value_to_text).unwrap_or_default();
        Ok(CdsView {
            name: name.to_string(),
            source: source.clone(),
            root_entity: body.get("dataSource").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            annotations: body,
            line_count: source.lines().count(),
        })
    }

    async fn search(&self, request: AdtSearchRequest) -> AdtResult<Vec<AdtSearchHit>> {
        // OIC integrations search: GET /ic/api/integration/v1/integrations?q=...
        let body = self
            .get_json(&format!(
                "/ic/api/integration/v1/integrations?q={{name:'{}'}}",
                request.query.replace('\'', "")
            ))
            .await?;
        let kind = request.kind.unwrap_or(OracleArtifactKind::Integration);
        let hits = body
            .get("items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .take(request.max_results.max(1))
                    .filter_map(|it| {
                        let name = it.get("code").or_else(|| it.get("name")).and_then(|v| v.as_str())?;
                        Some(AdtSearchHit {
                            name: name.to_string(),
                            kind,
                            description: it.get("description").and_then(|v| v.as_str()).map(String::from),
                            package: it.get("project").and_then(|v| v.as_str()).map(String::from),
                            score: 1.0,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(hits)
    }

    async fn where_used(&self, request: WhereUsedRequest) -> AdtResult<Vec<WhereUsedHit>> {
        // OIC exposes dependents of a connection/lookup via the usage endpoint.
        let resource = match request.kind {
            OracleArtifactKind::Connection => "connections",
            OracleArtifactKind::Lookup => "lookups",
            _ => "integrations",
        };
        let body = self
            .get_json(&format!("/ic/api/integration/v1/{}/{}/usages", resource, request.name))
            .await
            .unwrap_or(Value::Null);
        let hits = body
            .get("items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| {
                        let obj = it.get("code").or_else(|| it.get("name")).and_then(|v| v.as_str())?;
                        Some(WhereUsedHit {
                            object: obj.to_string(),
                            kind: OracleArtifactKind::Integration,
                            location: it.get("usage").and_then(|v| v.as_str()).unwrap_or("invoke").to_string(),
                            usage: "invoke".into(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(hits)
    }

    async fn get_table_contents(&self, table: &str, max_rows: usize) -> AdtResult<Vec<TableRow>> {
        if max_rows == 0 || max_rows > MAX_TABLE_ROWS {
            return Err(AdtError::InvalidObjectName(format!(
                "max_rows must be in 1..={MAX_TABLE_ROWS}, got {max_rows}"
            )));
        }
        // Oracle exposes no generic table-preview REST endpoint; direct bulk
        // reads go through BI Publisher (oracle.bip.runReport).
        Err(AdtError::DataPreviewBlocked(format!(
            "object {table} is not exposed for direct REST preview; use a BI Publisher extract (oracle.bip.runReport)"
        )))
    }

    async fn activate(&self, request: ActivationRequest, ctx: AdtCallContext) -> AdtResult<ActivationOutcome> {
        if ctx.read_only {
            return Err(AdtError::PermissionDenied(format!(
                "activate({} {}) blocked: read-only mode",
                request.kind.label(),
                request.name
            )));
        }
        // OIC activation: POST .../integrations/{id}?integrationInstruction=activate
        let path = format!(
            "/ic/api/integration/v1/integrations/{}?integrationInstruction=activate",
            request.name
        );
        let resp = self
            .authed(self.http.post(self.url(&path)))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| self.transport_err(e))?;
        let status = resp.status();
        Ok(ActivationOutcome {
            name: request.name.clone(),
            kind: request.kind,
            activated: status.is_success(),
            messages: vec![format!(
                "{} {} activation {} ({status})",
                request.kind.label(),
                request.name,
                if status.is_success() { "succeeded" } else { "failed" },
            )],
        })
    }
}
