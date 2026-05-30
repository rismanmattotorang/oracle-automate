//! Seed corpus: small, illustrative documents across the four Oracle domains
//! (Integration, BPMN process, application catalog, Oracle Help Center).
//!
//! Runs the documents through the chunker and embedder so the KnowledgeStore
//! exposes the same chunk-level surface as a real ingestion pipeline does.

use oracle_automate_ingest::{chunk_document, ChunkerConfig, EmbeddingClient};
use oracle_automate_kb::{Document, Domain, KnowledgeStore, UpsertBatch};

pub async fn populate_with_embeddings(
    store: &std::sync::Arc<dyn KnowledgeStore>,
    embedder: &dyn EmbeddingClient,
) -> anyhow::Result<()> {
    let docs = seed_documents();
    let chunker = ChunkerConfig::default();

    for doc in docs {
        let mut chunks = chunk_document(&doc, &chunker);
        if chunks.is_empty() {
            continue;
        }
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        let vectors = embedder.embed(&texts).await?;
        for (chunk, vec) in chunks.iter_mut().zip(vectors.into_iter()) {
            chunk.embedding = Some(vec);
        }
        store
            .upsert(UpsertBatch {
                documents: vec![doc],
                chunks,
            })
            .await?;
    }
    Ok(())
}

fn seed_documents() -> Vec<Document> {
    let mut out = Vec::new();

    out.push({
        let mut d = Document::new(
            "integration:KLB/KLB_GL_JOURNAL_IMPORT",
            Domain::Integration,
            "oic-int://KLB/KLB_GL_JOURNAL_IMPORT",
            "KLB_GL_JOURNAL_IMPORT",
            "OIC integration KLB_GL_JOURNAL_IMPORT posts GL journals via FBDI. It builds the \
             JournalImportTemplate, calls erpintegrations.importBulkData to stage rows into \
             GL_INTERFACE, then runs the Journal Import (JournalImportLauncher) ESS job. \
             It enriches the ledger from the KLB_COMPANY_XREF lookup and skips closed periods \
             checked against GL_PERIOD_STATUSES before submission.",
        );
        d.metadata
            .insert("package".into(), "KLB_FINANCE_INTEGRATIONS".into());
        d.metadata.insert("type".into(), "INTEGRATION".into());
        d
    });

    out.push({
        let mut d = Document::new(
            "integration:KLB/KLB_PO_RECEIPT_SYNC",
            Domain::Integration,
            "oic-int://KLB/KLB_PO_RECEIPT_SYNC",
            "KLB_PO_RECEIPT_SYNC",
            "OIC integration KLB_PO_RECEIPT_SYNC syncs warehouse goods receipts to Fusion \
             Receiving. It posts to the receivingReceiptRequests REST resource against a \
             purchase order, triggering receipt accounting events in Subledger Accounting (XLA) \
             that later transfer to GL_JE_LINES via Create Accounting.",
        );
        d.metadata
            .insert("package".into(), "KLB_FINANCE_INTEGRATIONS".into());
        d.metadata.insert("type".into(), "INTEGRATION".into());
        d
    });

    out.push({
        let mut d = Document::new(
            "bpmn:core/P2P-001",
            Domain::Bpmn,
            "process://core/P2P-001",
            "Procure-to-Pay (P2P)",
            "Oracle Procurement P2P process P2P-001: purchase requisition into PO approval into \
             receiving into invoice matching into payment. Process mining shows throughput drops \
             18% at PO approval due to approval-hierarchy coverage gaps in the procurement BU.",
        );
        d.breadcrumbs = vec!["core".into()];
        d
    });

    out.push({
        let mut d = Document::new(
            "bpmn:core/O2C-002", Domain::Bpmn, "process://core/O2C-002",
            "Order-to-Cash (O2C)",
            "Oracle Order Management O2C process O2C-002: order capture into availability check \
             into fulfillment into billing (AR) into receipt application. Mining shows a 12% rework \
             loop between billing and shipping, primarily caused by incomplete ship-to addresses \
             on the TCA party.",
        );
        d.breadcrumbs = vec!["core".into()];
        d
    });

    out.push(Document::new(
        "app_catalog:FS-12001",
        Domain::AppCatalog,
        "appcat://FS-12001",
        "Oracle Financials Cloud",
        "Application fact sheet for Oracle Financials Cloud (FS-12001). Lifecycle: active. \
         Business capabilities: general ledger, payables, receivables, asset accounting, cash \
         management. Integrations: KLB_GL_JOURNAL_IMPORT, Oracle Integration Cloud, ADW reporting. \
         Release: 24D.",
    ));

    out.push(Document::new(
        "app_catalog:FS-08823",
        Domain::AppCatalog,
        "appcat://FS-08823",
        "Legacy Billing Engine",
        "Application fact sheet for Legacy Billing Engine (FS-08823). Lifecycle: phase-out. \
         Integrations: KLB_LEGACY_BILL_FEED, mainframe. EOL: 2026-09. Replacement: Oracle \
         Receivables Cloud (AutoInvoice).",
    ));

    out.push({
        let mut d = Document::new(
            "oracle_help:GL/period-close", Domain::OracleHelp, "oracle-help://GL/period-close",
            "Period-End Close in Oracle General Ledger",
            "Oracle Help Center page on GL period-end close: open and close accounting periods \
             via Manage Accounting Periods (GL_PERIOD_STATUSES), run revaluation for foreign-currency \
             balances, create accounting and transfer subledger entries (XLA_AE_LINES) to GL_JE_LINES, \
             run the GL period close program, and review the close monitor and trial balance.",
        );
        d.breadcrumbs = vec!["Financials".into(), "General Ledger".into()];
        d.metadata.insert("module".into(), "GL".into());
        d
    });

    out.push({
        let mut d = Document::new(
            "oracle_help:INV/receiving", Domain::OracleHelp, "oracle-help://INV/receiving",
            "Receiving Receipts",
            "Oracle Help Center page describing receiving receipts (Receiving work area / \
             receivingReceiptRequests REST). Receipt routing: standard receipt, inspection required, \
             direct delivery. Creates receiving transactions and accrual accounting events that flow \
             through Subledger Accounting to the General Ledger.",
        );
        d.breadcrumbs = vec!["Supply Chain".into(), "Inventory Management".into()];
        d.metadata.insert("module".into(), "INV".into());
        d
    });

    out
}
