//! Typed entities and edges.
//!
//! The cross-domain Oracle Fusion knowledge graph has six entity families and a
//! small, fixed set of edge kinds.  Both are versioned via `#[non_exhaustive]`
//! so new Oracle domains (HCM, EPM, etc.) can extend without breaking
//! consumers — paper §VII-F notes this stability requirement.

use serde::{Deserialize, Serialize};

pub type NodeId = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// OIC integration / Application Composer Groovy / custom-code artifact.
    Integration,
    /// Oracle data object (table / view / REST resource backing object).
    DataObject,
    /// Object attribute / column.
    Field,
    /// Oracle Fusion REST operation.
    RestOperation,
    /// Process model (Oracle Process Automation / BPMN).
    ProcessModel,
    /// Application-catalog / portfolio fact sheet.
    AppCatalogEntry,
    /// Oracle Help Center page or section.
    HelpPage,
    /// Business concept (e.g. "period close", "goods movement").  These
    /// are the nodes that let GraphRAG community summaries cross domains.
    Concept,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: NodeId,
    pub kind: EntityKind,
    pub label: String,
    /// Short description, used for community summaries.
    #[serde(default)]
    pub description: Option<String>,
    /// Native URI for citation (oracle-help://, oic-int://, oracle-rest://, etc.).
    #[serde(default)]
    pub uri: Option<String>,
    /// Arbitrary string-valued tags ("module:FIN", "project:KLB_FINANCE_INTEGRATIONS", ...).
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// OIC/custom-code object A calls / invokes B.
    Calls,
    /// OIC/custom-code class implements interface.
    Implements,
    /// Program includes another program / include file.
    Includes,
    /// Object reads from table.
    ReadsTable,
    /// Object writes to table.
    WritesTable,
    /// One entity references / mentions another in its documentation.
    References,
    /// Entity is contained in a parent (program in package, field in table).
    ContainedIn,
    /// Entity depends on another (BPMN step depends on REST operation, app depends on table).
    DependsOn,
    /// Concept describes / categorises an entity.
    Describes,
    /// Free-form relationship — last-resort kind that should still be
    /// rare enough that GraphRAG community summaries remain meaningful.
    Related,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    /// Optional weight; defaults to 1.0.  PPR uses it; community detection
    /// treats it as an edge multiplicity.
    #[serde(default = "default_weight")]
    pub weight: f32,
}

fn default_weight() -> f32 { 1.0 }
