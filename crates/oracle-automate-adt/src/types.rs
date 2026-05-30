//! Request and response types shared by every Oracle artifact backend.
//!
//! Port note: these model Oracle Fusion / OIC development artifacts (the
//! analog of the SAP ABAP/ADT object surface). The `ErpClient`-style trait
//! method names (`get_program`, `get_class`, …) are renamed alongside the
//! server tool namespace in P5; here the *artifact taxonomy* and the
//! *fixtures* are Oracle.

use serde::{Deserialize, Serialize};

pub const MAX_TABLE_ROWS: usize = 1000;

/// Kinds of Oracle development / configuration artifact this surface serves.
///
/// The Oracle analog of the SAP ABAP object taxonomy: instead of programs,
/// classes and CDS views, Oracle custom logic lives in OIC integrations,
/// Application Composer Groovy, BI Publisher, value sets and sandboxes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OracleArtifactKind {
    /// OIC integration flow.
    Integration,
    /// Application Composer / OIC Groovy script.
    GroovyScript,
    /// OIC connection (adapter instance).
    Connection,
    /// OIC lookup (DVM / cross-reference table).
    Lookup,
    /// OIC integration package / project.
    IntegrationPackage,
    /// Enterprise Scheduler (ESS) scheduled process / job.
    EssJob,
    /// BI Publisher data model.
    BipDataModel,
    /// Custom Fusion REST resource (Application Composer).
    RestResource,
    /// Application Composer object attribute / field.
    Attribute,
    /// Oracle value set (flexfield value set).
    ValueSet,
    /// OIC project / IAR deployment unit.
    Project,
    /// BI Publisher report.
    BipReport,
    /// Business rule (Process Automation / Application Composer).
    BusinessRule,
    /// Custom REST service definition.
    RestService,
    /// Sandbox customization (metadata change in a sandbox).
    SandboxCustomization,
    /// Application Composer extension.
    AppComposerExtension,
    /// Scheduled process definition (ESS job definition).
    ScheduledProcess,
}

impl OracleArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Integration => "Integration",
            Self::GroovyScript => "Groovy Script",
            Self::Connection => "Connection",
            Self::Lookup => "Lookup",
            Self::IntegrationPackage => "Integration Package",
            Self::EssJob => "ESS Job",
            Self::BipDataModel => "BI Publisher Data Model",
            Self::RestResource => "REST Resource",
            Self::Attribute => "Attribute",
            Self::ValueSet => "Value Set",
            Self::Project => "Project",
            Self::BipReport => "BI Publisher Report",
            Self::BusinessRule => "Business Rule",
            Self::RestService => "REST Service",
            Self::SandboxCustomization => "Sandbox Customization",
            Self::AppComposerExtension => "Application Composer Extension",
            Self::ScheduledProcess => "Scheduled Process",
        }
    }

    /// Oracle REST path fragment for the artifact, used by the live
    /// [`HttpOicClient`](crate::http::HttpOicClient): OIC integration API,
    /// BI Publisher, and Fusion REST.
    pub fn oic_path(self, name: &str) -> String {
        let n = name;
        match self {
            Self::Integration => format!("/ic/api/integration/v1/integrations/{n}"),
            Self::GroovyScript => format!("/ic/api/integration/v1/integrations/{n}/groovy"),
            Self::Connection => format!("/ic/api/integration/v1/connections/{n}"),
            Self::Lookup => format!("/ic/api/integration/v1/lookups/{n}"),
            Self::IntegrationPackage | Self::Project => {
                format!("/ic/api/integration/v1/projects/{n}")
            }
            Self::EssJob | Self::ScheduledProcess => {
                format!("/fscmRestApi/resources/11.13.18.05/erpintegrations/{n}")
            }
            Self::BipDataModel => format!("/xmlpserver/services/rest/v1/catalog/dataModels/{n}"),
            Self::BipReport => format!("/xmlpserver/services/rest/v1/reports/{n}"),
            Self::RestResource | Self::RestService => {
                format!("/fscmRestApi/resources/11.13.18.05/{n}")
            }
            Self::ValueSet => format!("/fscmRestApi/resources/11.13.18.05/setupValueSets/{n}"),
            Self::Attribute => format!("/fscmRestApi/resources/11.13.18.05/{n}/describe"),
            Self::SandboxCustomization | Self::AppComposerExtension => {
                format!("/fndSetup/sandboxes/{n}")
            }
            Self::BusinessRule => format!("/bpm/api/4.0/rules/{n}"),
        }
    }
}

/// Source/representation of a single artifact (integration XML/JSON, Groovy,
/// BIP report definition, …). Keeps the field shape stable across backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramSource {
    pub name: String,
    pub kind: OracleArtifactKind,
    /// Owning OIC package / Fusion offering.
    pub package: Option<String>,
    /// Description / short text from the artifact header.
    pub description: Option<String>,
    pub source: String,
    /// Whether the artifact is currently active/activated (vs. configured but
    /// not yet activated/published).
    pub active: bool,
    /// Lines counted from the source.
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdsView {
    pub name: String,
    pub source: String,
    /// Primary data source / view object the report's data model selects from.
    pub root_entity: String,
    /// Structured metadata distilled for quick access (data model params, etc.).
    pub annotations: serde_json::Value,
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMember {
    pub name: String,
    pub kind: OracleArtifactKind,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageContents {
    pub package: String,
    pub description: Option<String>,
    pub members: Vec<PackageMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdtSearchRequest {
    pub query: String,
    #[serde(default)]
    pub kind: Option<OracleArtifactKind>,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    25
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdtSearchHit {
    pub name: String,
    pub kind: OracleArtifactKind,
    pub description: Option<String>,
    pub package: Option<String>,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhereUsedRequest {
    pub name: String,
    pub kind: OracleArtifactKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhereUsedHit {
    pub object: String,
    pub kind: OracleArtifactKind,
    /// Where in the artifact the reference appears (activity, mapping, line).
    pub location: String,
    /// e.g. `read`, `write`, `invoke`, `maps`, `implements`.
    pub usage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    pub values: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationRequest {
    pub name: String,
    pub kind: OracleArtifactKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationOutcome {
    pub name: String,
    pub kind: OracleArtifactKind,
    pub activated: bool,
    pub messages: Vec<String>,
}
