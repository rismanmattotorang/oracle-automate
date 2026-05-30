//! In-memory graph store seeded with realistic cross-domain Oracle Fusion fixtures.
//!
//! The same fixtures the REST operation + ADT + KB mocks expose, but stitched into
//! one graph so multi-hop traversal demos are meaningful offline.
//!
//! Example dependency chain (encoded below):
//!
//!   `ZIF_FIN_POSTABLE` (interface)
//!       ←implements← `ZCL_FIN_POSTER` (class)
//!           ←calls← `ZFIN_POST_JE` (program)
//!               ↓includes
//!           `ZFIN_TOP`, `ZFIN_F01`
//!       ↓calls
//!     `BAPI_ACC_DOCUMENT_POST` (REST operation)
//!       ↓reads_table
//!     `T001`, `T001B`
//!       ↓describes
//!     `Concept: period_close`
//!       ←contained_in← `BPMN: Order-to-Cash`
//!       ←depends_on← `LeanIX: Oracle Fusion Cloud ERP Finance`

use crate::entity::{Edge, EdgeKind, Entity, EntityKind, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub struct InMemoryGraph {
    nodes: HashMap<NodeId, Entity>,
    /// Adjacency: id → list of (neighbour, edge kind, weight)
    out_edges: HashMap<NodeId, Vec<(NodeId, EdgeKind, f32)>>,
    in_edges: HashMap<NodeId, Vec<(NodeId, EdgeKind, f32)>>,
    /// Raw edge list for community-detection algorithms that prefer it.
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub by_kind: HashMap<String, usize>,
}

impl InMemoryGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed with the cross-domain Oracle fixture set.  Idempotent.
    pub fn with_demo_corpus() -> Self {
        let mut g = Self::new();
        g.seed();
        g
    }

    pub fn add_entity(&mut self, e: Entity) {
        self.nodes.insert(e.id.clone(), e);
    }

    pub fn add_edge(&mut self, e: Edge) {
        self.out_edges
            .entry(e.from.clone())
            .or_default()
            .push((e.to.clone(), e.kind, e.weight));
        self.in_edges
            .entry(e.to.clone())
            .or_default()
            .push((e.from.clone(), e.kind, e.weight));
        self.edges.push(e);
    }

    pub fn node(&self, id: &str) -> Option<&Entity> {
        self.nodes.get(id)
    }
    pub fn nodes(&self) -> impl Iterator<Item = &Entity> {
        self.nodes.values()
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Outgoing neighbours: `id → (to, kind, weight)`.
    pub fn outbound(&self, id: &str) -> &[(NodeId, EdgeKind, f32)] {
        self.out_edges.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Incoming neighbours.
    pub fn inbound(&self, id: &str) -> &[(NodeId, EdgeKind, f32)] {
        self.in_edges.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Undirected adjacency for community detection / PPR.
    pub fn undirected_neighbours(&self, id: &str) -> Vec<(NodeId, f32)> {
        let mut seen: HashMap<NodeId, f32> = HashMap::new();
        for (n, _, w) in self.outbound(id) {
            *seen.entry(n.clone()).or_insert(0.0) += w;
        }
        for (n, _, w) in self.inbound(id) {
            *seen.entry(n.clone()).or_insert(0.0) += w;
        }
        seen.into_iter().collect()
    }

    pub fn stats(&self) -> GraphStats {
        let mut by_kind: HashMap<String, usize> = HashMap::new();
        for e in self.nodes.values() {
            *by_kind.entry(format!("{:?}", e.kind)).or_insert(0) += 1;
        }
        GraphStats {
            node_count: self.nodes.len(),
            edge_count: self.edges.len(),
            by_kind,
        }
    }

    /// Find nodes by free-text match over label + description + tags.
    /// Used by the HippoRAG seeding step.
    pub fn find_seeds(&self, query: &str, max_seeds: usize) -> Vec<NodeId> {
        let q = query.to_lowercase();
        let terms: Vec<&str> = q.split_whitespace().filter(|t| t.len() >= 2).collect();
        if terms.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, &Entity)> = self
            .nodes
            .values()
            .filter_map(|e| {
                let hay = format!(
                    "{} {} {}",
                    e.label.to_lowercase(),
                    e.description.as_deref().unwrap_or("").to_lowercase(),
                    e.tags.join(" ").to_lowercase(),
                );
                let score: usize = terms.iter().map(|t| hay.matches(t).count()).sum();
                if score == 0 {
                    None
                } else {
                    Some((score, e))
                }
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored
            .into_iter()
            .take(max_seeds)
            .map(|(_, e)| e.id.clone())
            .collect()
    }

    fn seed(&mut self) {
        let add = |g: &mut Self,
                   id: &str,
                   kind: EntityKind,
                   label: &str,
                   desc: &str,
                   uri: Option<&str>,
                   tags: &[&str]| {
            g.add_entity(Entity {
                id: id.into(),
                kind,
                label: label.into(),
                description: Some(desc.into()),
                uri: uri.map(String::from),
                tags: tags.iter().map(|s| s.to_string()).collect(),
            });
        };

        // OIC integrations / custom-code artifacts
        add(
            self,
            "integration:KLB_GL_JOURNAL_IMPORT",
            EntityKind::Integration,
            "KLB_GL_JOURNAL_IMPORT",
            "OIC integration that posts GL journals via FBDI (journalEntries / importBulkData).",
            Some("oic-int://KLB/KLB_GL_JOURNAL_IMPORT"),
            &[
                "module:FIN",
                "project:KLB_FINANCE_INTEGRATIONS",
                "kind:integration",
            ],
        );
        add(
            self,
            "integration:KLB_PO_RECEIPT_SYNC",
            EntityKind::Integration,
            "KLB_PO_RECEIPT_SYNC",
            "OIC integration that syncs warehouse receipts to Fusion Receiving.",
            Some("oic-int://KLB/KLB_PO_RECEIPT_SYNC"),
            &[
                "module:SCM",
                "project:KLB_FINANCE_INTEGRATIONS",
                "kind:integration",
            ],
        );
        add(
            self,
            "integration:KLB_INVOICE_HOLD_RULE",
            EntityKind::Integration,
            "KLB_INVOICE_HOLD_RULE",
            "Application Composer Groovy rule that holds high-value AP invoices.",
            Some("oic-int://KLB/KLB_INVOICE_HOLD_RULE"),
            &["module:FIN", "kind:groovy_script"],
        );
        add(
            self,
            "integration:KLB_FUSION_ERP_REST",
            EntityKind::Integration,
            "KLB_FUSION_ERP_REST",
            "OIC connection to Oracle Fusion Cloud ERP REST.",
            Some("oic-int://KLB/KLB_FUSION_ERP_REST"),
            &["kind:connection"],
        );
        add(
            self,
            "integration:KLB_COMPANY_XREF",
            EntityKind::Integration,
            "KLB_COMPANY_XREF",
            "DVM lookup: legacy company code -> Fusion ledger.",
            Some("oic-int://KLB/KLB_COMPANY_XREF"),
            &["kind:lookup"],
        );

        // Oracle Fusion REST operations
        add(
            self,
            "rest:journalEntries.post",
            EntityKind::RestOperation,
            "fusion.gl.journalEntries.post",
            "Create and post a GL journal entry via Fusion REST.",
            Some("oracle-rest://fusion.gl.journalEntries.post"),
            &["module:FIN"],
        );
        add(
            self,
            "rest:receivingReceiptRequests.post",
            EntityKind::RestOperation,
            "fusion.inv.receivingReceiptRequests.post",
            "Create a receiving receipt (goods receipt against a PO).",
            Some("oracle-rest://fusion.inv.receivingReceiptRequests.post"),
            &["module:SCM"],
        );
        add(
            self,
            "rest:itemsV2.get",
            EntityKind::RestOperation,
            "fusion.scm.itemsV2.get",
            "Read Product Hub item master detail.",
            Some("oracle-rest://fusion.scm.itemsV2.get"),
            &["module:SCM"],
        );
        add(
            self,
            "rest:salesOrdersForOrderHub.post",
            EntityKind::RestOperation,
            "fusion.doo.salesOrdersForOrderHub.post",
            "Import a sales order into Order Management.",
            Some("oracle-rest://fusion.doo.salesOrdersForOrderHub.post"),
            &["module:SCM"],
        );

        // Oracle data objects
        add(
            self,
            "obj:GL_LEDGERS",
            EntityKind::DataObject,
            "GL_LEDGERS",
            "General Ledger ledgers.",
            Some("oracle-object://GL_LEDGERS/structure"),
            &["module:FIN"],
        );
        add(
            self,
            "obj:GL_PERIOD_STATUSES",
            EntityKind::DataObject,
            "GL_PERIOD_STATUSES",
            "Accounting period open/close status.",
            Some("oracle-object://GL_PERIOD_STATUSES/structure"),
            &["module:FIN"],
        );
        add(
            self,
            "obj:EGP_SYSTEM_ITEMS_B",
            EntityKind::DataObject,
            "EGP_SYSTEM_ITEMS_B",
            "Product Hub item master.",
            Some("oracle-object://EGP_SYSTEM_ITEMS_B/structure"),
            &["module:SCM"],
        );
        add(
            self,
            "obj:DOO_HEADERS_ALL",
            EntityKind::DataObject,
            "DOO_HEADERS_ALL",
            "Order Management order header.",
            Some("oracle-object://DOO_HEADERS_ALL/structure"),
            &["module:SCM"],
        );
        add(
            self,
            "obj:GL_JE_LINES",
            EntityKind::DataObject,
            "GL_JE_LINES",
            "GL journal entry lines (accounting backbone).",
            Some("oracle-object://GL_JE_LINES/structure"),
            &["module:FIN"],
        );
        add(
            self,
            "obj:XLA_AE_LINES",
            EntityKind::DataObject,
            "XLA_AE_LINES",
            "Subledger Accounting lines.",
            Some("oracle-object://XLA_AE_LINES/structure"),
            &["module:FIN"],
        );

        // Process models
        add(
            self,
            "proc:P2P-001",
            EntityKind::ProcessModel,
            "Procure-to-Pay (P2P)",
            "Requisition through receiving, invoice match, and payment.",
            Some("process://core/P2P-001"),
            &["process:p2p"],
        );
        add(
            self,
            "proc:O2C-002",
            EntityKind::ProcessModel,
            "Order-to-Cash (O2C)",
            "Order capture through fulfillment, billing, and receipt.",
            Some("process://core/O2C-002"),
            &["process:o2c"],
        );

        // Application-catalog entries
        add(
            self,
            "appcat:FS-12001",
            EntityKind::AppCatalogEntry,
            "Oracle Financials Cloud",
            "Financials application: general ledger, payables, receivables.",
            Some("appcat://FS-12001"),
            &["lifecycle:active"],
        );
        add(
            self,
            "appcat:FS-08823",
            EntityKind::AppCatalogEntry,
            "Legacy Billing Engine",
            "Phase-out billing engine (replaced by Oracle Receivables).",
            Some("appcat://FS-08823"),
            &["lifecycle:phase_out"],
        );

        // Oracle Help Center pages
        add(
            self,
            "help:GL/period-close",
            EntityKind::HelpPage,
            "Period-End Close in Oracle General Ledger",
            "Procedure for GL period-end close.",
            Some("oracle-help://GL/period-close"),
            &["module:FIN"],
        );
        add(
            self,
            "help:INV/receiving",
            EntityKind::HelpPage,
            "Receiving Receipts",
            "Procedure for receiving receipts.",
            Some("oracle-help://INV/receiving"),
            &["module:SCM"],
        );

        // Concepts (cross-domain hubs)
        add(
            self,
            "concept:period_close",
            EntityKind::Concept,
            "Period Close",
            "GL period-end close: close subledgers, transfer XLA to GL, revalue, close period.",
            None,
            &["module:FIN"],
        );
        add(
            self,
            "concept:receiving",
            EntityKind::Concept,
            "Receiving",
            "Receiving goods against a purchase order; creates accrual accounting events.",
            None,
            &["module:SCM"],
        );
        add(
            self,
            "concept:journal_entry",
            EntityKind::Concept,
            "Journal Entry",
            "Accounting entry that posts to GL_JE_LINES (and, from a subledger, XLA_AE_LINES).",
            None,
            &["module:FIN"],
        );

        // Edges
        let edges: Vec<(&str, &str, EdgeKind, f32)> = vec![
            // Integrations invoke REST operations + use connection/lookup
            (
                "integration:KLB_GL_JOURNAL_IMPORT",
                "rest:journalEntries.post",
                EdgeKind::Calls,
                2.0,
            ),
            (
                "integration:KLB_GL_JOURNAL_IMPORT",
                "integration:KLB_FUSION_ERP_REST",
                EdgeKind::Calls,
                1.0,
            ),
            (
                "integration:KLB_GL_JOURNAL_IMPORT",
                "integration:KLB_COMPANY_XREF",
                EdgeKind::References,
                1.0,
            ),
            (
                "integration:KLB_PO_RECEIPT_SYNC",
                "rest:receivingReceiptRequests.post",
                EdgeKind::Calls,
                1.0,
            ),
            (
                "integration:KLB_PO_RECEIPT_SYNC",
                "integration:KLB_FUSION_ERP_REST",
                EdgeKind::Calls,
                1.0,
            ),
            // REST operations read / write data objects
            (
                "rest:journalEntries.post",
                "obj:GL_LEDGERS",
                EdgeKind::ReadsTable,
                1.0,
            ),
            (
                "rest:journalEntries.post",
                "obj:GL_PERIOD_STATUSES",
                EdgeKind::ReadsTable,
                1.0,
            ),
            (
                "rest:journalEntries.post",
                "obj:GL_JE_LINES",
                EdgeKind::WritesTable,
                1.0,
            ),
            (
                "rest:journalEntries.post",
                "obj:XLA_AE_LINES",
                EdgeKind::WritesTable,
                1.0,
            ),
            (
                "rest:itemsV2.get",
                "obj:EGP_SYSTEM_ITEMS_B",
                EdgeKind::ReadsTable,
                1.0,
            ),
            (
                "rest:salesOrdersForOrderHub.post",
                "obj:DOO_HEADERS_ALL",
                EdgeKind::WritesTable,
                1.0,
            ),
            (
                "rest:receivingReceiptRequests.post",
                "obj:XLA_AE_LINES",
                EdgeKind::WritesTable,
                1.0,
            ),
            // Process models depend on REST operations
            (
                "proc:P2P-001",
                "rest:receivingReceiptRequests.post",
                EdgeKind::DependsOn,
                1.0,
            ),
            (
                "proc:P2P-001",
                "rest:journalEntries.post",
                EdgeKind::DependsOn,
                1.0,
            ),
            (
                "proc:O2C-002",
                "rest:salesOrdersForOrderHub.post",
                EdgeKind::DependsOn,
                1.0,
            ),
            (
                "proc:O2C-002",
                "rest:journalEntries.post",
                EdgeKind::DependsOn,
                1.0,
            ),
            // App-catalog entries depend on data objects
            (
                "appcat:FS-12001",
                "obj:GL_JE_LINES",
                EdgeKind::DependsOn,
                1.0,
            ),
            (
                "appcat:FS-12001",
                "obj:XLA_AE_LINES",
                EdgeKind::DependsOn,
                1.0,
            ),
            (
                "appcat:FS-12001",
                "obj:GL_LEDGERS",
                EdgeKind::DependsOn,
                1.0,
            ),
            (
                "appcat:FS-08823",
                "obj:DOO_HEADERS_ALL",
                EdgeKind::DependsOn,
                1.0,
            ),
            // Concepts describe entities (cross-domain hubs)
            (
                "concept:period_close",
                "help:GL/period-close",
                EdgeKind::Describes,
                2.0,
            ),
            (
                "concept:period_close",
                "obj:GL_PERIOD_STATUSES",
                EdgeKind::Describes,
                2.0,
            ),
            (
                "concept:period_close",
                "obj:GL_JE_LINES",
                EdgeKind::Describes,
                1.5,
            ),
            (
                "concept:period_close",
                "appcat:FS-12001",
                EdgeKind::Describes,
                1.0,
            ),
            (
                "concept:journal_entry",
                "rest:journalEntries.post",
                EdgeKind::Describes,
                2.0,
            ),
            (
                "concept:journal_entry",
                "obj:GL_JE_LINES",
                EdgeKind::Describes,
                1.5,
            ),
            (
                "concept:journal_entry",
                "integration:KLB_GL_JOURNAL_IMPORT",
                EdgeKind::Describes,
                1.5,
            ),
            (
                "concept:receiving",
                "help:INV/receiving",
                EdgeKind::Describes,
                2.0,
            ),
            (
                "concept:receiving",
                "rest:receivingReceiptRequests.post",
                EdgeKind::Describes,
                2.0,
            ),
            (
                "concept:receiving",
                "integration:KLB_PO_RECEIPT_SYNC",
                EdgeKind::Describes,
                1.0,
            ),
            // Help pages reference data objects / operations
            (
                "help:GL/period-close",
                "obj:GL_PERIOD_STATUSES",
                EdgeKind::References,
                1.0,
            ),
            (
                "help:GL/period-close",
                "obj:GL_JE_LINES",
                EdgeKind::References,
                1.0,
            ),
            (
                "help:INV/receiving",
                "rest:receivingReceiptRequests.post",
                EdgeKind::References,
                1.0,
            ),
        ];
        let mut seen: HashSet<(NodeId, NodeId, EdgeKind)> = HashSet::new();
        for (from, to, kind, weight) in edges {
            let key = (from.to_string(), to.to_string(), kind);
            if seen.insert(key) {
                self.add_edge(Edge {
                    from: from.into(),
                    to: to.into(),
                    kind,
                    weight,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_corpus_has_cross_domain_edges() {
        let g = InMemoryGraph::with_demo_corpus();
        let stats = g.stats();
        assert!(
            stats.node_count >= 20,
            "expected >= 20 nodes, got {}",
            stats.node_count
        );
        assert!(
            stats.edge_count >= 25,
            "expected >= 25 edges, got {}",
            stats.edge_count
        );
        // The period_close concept should reach LeanIX FS-12001 in two hops:
        // concept:period_close → obj:GL_JE_LINES ← appcat:FS-12001
        assert!(g
            .outbound("concept:period_close")
            .iter()
            .any(|(n, _, _)| n == "obj:GL_JE_LINES"));
        assert!(g
            .inbound("obj:GL_JE_LINES")
            .iter()
            .any(|(n, _, _)| n == "appcat:FS-12001"));
    }

    #[test]
    fn find_seeds_locates_relevant_entities() {
        let g = InMemoryGraph::with_demo_corpus();
        let seeds = g.find_seeds("period close GL_JE_LINES", 5);
        assert!(seeds.iter().any(|s| s == "concept:period_close"
            || s == "obj:GL_JE_LINES"
            || s == "help:GL/period-close"));
    }
}
