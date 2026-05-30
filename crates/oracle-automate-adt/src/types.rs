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

    /// Legacy SAP-ADT URL fragment used by the not-yet-ported live HTTP
    /// transport (`http.rs`). The offline mock does not use this; it remains
    /// the SAP-ADT path builder until the live client is rewritten against
    /// the Oracle OIC / BI Publisher / Fusion REST endpoints in the
    /// live-transport sub-phase. Variant *names* are Oracle; the URLs are
    /// still SAP and exist only to keep the legacy client + its tests green.
    pub fn adt_path(self, name: &str) -> String {
        let n = name.to_lowercase();
        match self {
            Self::Integration => format!("/sap/bc/adt/programs/programs/{n}/source/main"),
            Self::GroovyScript => format!("/sap/bc/adt/oo/classes/{n}/source/main"),
            Self::Connection => format!("/sap/bc/adt/oo/interfaces/{n}/source/main"),
            Self::Lookup => format!("/sap/bc/adt/programs/includes/{n}/source/main"),
            Self::IntegrationPackage => format!("/sap/bc/adt/functions/groups/{n}/source/main"),
            Self::EssJob => format!("/sap/bc/adt/functions/groups/{{group}}/fmodules/{n}/source/main"),
            Self::BipDataModel => format!("/sap/bc/adt/ddic/tables/{n}/source/main"),
            Self::RestResource => format!("/sap/bc/adt/ddic/structures/{n}/source/main"),
            Self::Attribute => format!("/sap/bc/adt/ddic/dataelements/{n}"),
            Self::ValueSet => format!("/sap/bc/adt/ddic/domains/{n}/source/main"),
            Self::BipReport => format!("/sap/bc/adt/ddic/ddl/sources/{n}/source/main"),
            Self::Project => "/sap/bc/adt/repository/nodestructure".to_string(),
            Self::ScheduledProcess => format!(
                "/sap/bc/adt/repository/informationsystem/objectproperties/values?uri=%2Fsap%2Fbc%2Fadt%2Fvit%2Fwb%2Fobject_type%2Ftrant%2Fobject_name%2F{n}",
            ),
            Self::AppComposerExtension
            | Self::BusinessRule
            | Self::RestService
            | Self::SandboxCustomization => format!("/sap/bc/adt/objects/{n}"),
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
