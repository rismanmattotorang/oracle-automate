//! Offline mock Oracle artifact client.
//!
//! Seeded with realistic Gaussian Technologies Fusion / OIC fixtures:
//!   - Integrations: GT_GL_JOURNAL_IMPORT, GT_PO_RECEIPT_SYNC
//!   - Groovy scripts: GT_INVOICE_HOLD_RULE, GT_ITEM_DEFAULTING
//!   - Connections: GT_FUSION_ERP_REST
//!   - Lookups: GT_COMPANY_XREF
//!   - ESS jobs: JournalImportLauncher in package GL
//!   - BI Publisher reports: GT_GL_JOURNAL_EXTRACT
//!   - Projects/packages: GT_FINANCE_INTEGRATIONS
//!   - Where-used links wired between the above so impact analysis is
//!     meaningful in demos.

use crate::client::{OicCallContext, OicClient};
use crate::destination::OicDestination;
use crate::error::{OicError, OicResult};
use crate::types::{
    ActivationOutcome, ActivationRequest, CdsView, OicSearchHit, OicSearchRequest,
    OracleArtifactKind, PackageContents, PackageMember, ProgramSource, TableRow, WhereUsedHit,
    WhereUsedRequest, MAX_TABLE_ROWS,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub struct MockOicClient {
    destination: OicDestination,
    programs: HashMap<String, ProgramSource>,
    classes: HashMap<String, ProgramSource>,
    interfaces: HashMap<String, ProgramSource>,
    includes: HashMap<String, ProgramSource>,
    function_modules: HashMap<(String, String), ProgramSource>,
    cds_views: HashMap<String, CdsView>,
    packages: HashMap<String, PackageContents>,
    where_used: HashMap<(String, OracleArtifactKind), Vec<WhereUsedHit>>,
    tables: HashMap<String, Vec<TableRow>>,
}

impl MockOicClient {
    pub fn new(destination: OicDestination) -> Arc<Self> {
        let mut s = Self {
            destination,
            programs: HashMap::new(),
            classes: HashMap::new(),
            interfaces: HashMap::new(),
            includes: HashMap::new(),
            function_modules: HashMap::new(),
            cds_views: HashMap::new(),
            packages: HashMap::new(),
            where_used: HashMap::new(),
            tables: HashMap::new(),
        };
        s.seed();
        Arc::new(s)
    }

    fn seed(&mut self) {
        // Integrations (OIC integration flows)
        self.programs.insert("GT_GL_JOURNAL_IMPORT".into(), prog(
            "GT_GL_JOURNAL_IMPORT", OracleArtifactKind::Integration, "GT_FINANCE_INTEGRATIONS",
            "Stage and import GL journals via FBDI",
            "<integration name=\"GT_GL_JOURNAL_IMPORT\" version=\"01.00.0000\">\n  <trigger adapter=\"rest\"/>\n  <invoke connection=\"GT_FUSION_ERP_REST\" operation=\"importBulkData\"/>\n  <!-- builds JournalImportTemplate FBDI zip, calls erpintegrations.importBulkData,\n       then polls the Journal Import ESS request to completion -->\n</integration>\n",
        ));
        self.programs.insert("GT_PO_RECEIPT_SYNC".into(), prog(
            "GT_PO_RECEIPT_SYNC", OracleArtifactKind::Integration, "GT_FINANCE_INTEGRATIONS",
            "Sync warehouse receipts to Fusion Receiving",
            "<integration name=\"GT_PO_RECEIPT_SYNC\" version=\"01.00.0000\">\n  <trigger adapter=\"rest\"/>\n  <invoke connection=\"GT_FUSION_ERP_REST\" resource=\"receivingReceiptRequests\" method=\"POST\"/>\n</integration>\n",
        ));

        // Groovy scripts (Application Composer)
        self.classes.insert("GT_INVOICE_HOLD_RULE".into(), prog(
            "GT_INVOICE_HOLD_RULE", OracleArtifactKind::GroovyScript, "GT_FINANCE_INTEGRATIONS",
            "AP invoice hold trigger (Application Composer)",
            "// Application Composer object trigger (Groovy)\nif (InvoiceAmount > 100000000 && ApprovalStatus == 'PENDING') {\n  adf.util.applyHold('AMOUNT_THRESHOLD', 'Exceeds IDR 100,000,000 — needs controller approval')\n}\n",
        ));
        self.classes.insert("GT_ITEM_DEFAULTING".into(), prog(
            "GT_ITEM_DEFAULTING", OracleArtifactKind::GroovyScript, "GT_SCM_EXTENSIONS",
            "Default item attributes on creation",
            "// Groovy: default primary UOM for hardware components\nif (ItemClass == 'COMPONENT' && PrimaryUOMValue == null) {\n  setAttribute('PrimaryUOMValue', 'EA')\n}\n",
        ));

        // Connections (OIC adapter instances)
        self.interfaces.insert("GT_FUSION_ERP_REST".into(), prog(
            "GT_FUSION_ERP_REST", OracleArtifactKind::Connection, "GT_FINANCE_INTEGRATIONS",
            "Connection to Fusion Cloud ERP REST",
            "{\n  \"name\": \"GT_FUSION_ERP_REST\",\n  \"adapter\": \"oracle-erp-cloud\",\n  \"baseUri\": \"https://gaussian.fa.ocs.oraclecloud.com\",\n  \"securityPolicy\": \"OAuth Client Credentials\"\n}\n",
        ));

        // Lookups (DVM / cross-reference)
        self.includes.insert(
            "GT_COMPANY_XREF".into(),
            prog(
                "GT_COMPANY_XREF",
                OracleArtifactKind::Lookup,
                "GT_FINANCE_INTEGRATIONS",
                "Legacy company code -> Fusion ledger cross-reference",
                "LEGACY_CODE,FUSION_LEDGER\nKF01,Gaussian Technologies Primary Ledger\nKF02,Gaussian Technologies USD Reporting\n",
            ),
        );

        // ESS jobs (scheduled processes)
        self.function_modules.insert(("GL".into(), "JournalImportLauncher".into()), prog(
            "JournalImportLauncher", OracleArtifactKind::EssJob, "GL",
            "GL Journal Import ESS job",
            "Job: /oracle/apps/ess/financials/generalLedger/programs/common/JournalImportLauncher\nParameters: InterfaceRunId, LedgerId, Source=GAUSSIAN_OIC, GroupId\n",
        ));

        // BI Publisher report (data extract)
        self.cds_views.insert("GT_GL_JOURNAL_EXTRACT".into(), CdsView {
            name: "GT_GL_JOURNAL_EXTRACT".into(),
            root_entity: "GL_JE_LINES".into(),
            annotations: serde_json::json!({
                "catalogPath": "/Custom/Gaussian Technologies/Finance/GT_GL_JOURNAL_EXTRACT.xdo",
                "dataModel": "GT_GL_JOURNAL_DM",
                "outputFormat": "csv",
                "label": "GL journal line extract"
            }),
            source: "SELECT jl.je_header_id, jl.je_line_num, l.name ledger, jl.code_combination_id,\n       jl.period_name, jl.entered_dr, jl.entered_cr, jl.currency_code\n  FROM gl_je_lines jl\n  JOIN gl_ledgers l ON l.ledger_id = jl.ledger_id\n WHERE jl.period_name = :p_period\n   AND l.ledger_id = :p_ledger_id\n".into(),
            line_count: 8,
        });

        // Projects / packages
        self.packages.insert(
            "GT_FINANCE_INTEGRATIONS".into(),
            PackageContents {
                package: "GT_FINANCE_INTEGRATIONS".into(),
                description: Some(
                    "Gaussian Technologies Finance OIC integrations + extensions".into(),
                ),
                members: vec![
                    PackageMember {
                        name: "GT_GL_JOURNAL_IMPORT".into(),
                        kind: OracleArtifactKind::Integration,
                        description: Some("GL journal FBDI import".into()),
                    },
                    PackageMember {
                        name: "GT_PO_RECEIPT_SYNC".into(),
                        kind: OracleArtifactKind::Integration,
                        description: Some("Receiving sync".into()),
                    },
                    PackageMember {
                        name: "GT_FUSION_ERP_REST".into(),
                        kind: OracleArtifactKind::Connection,
                        description: Some("Fusion ERP REST connection".into()),
                    },
                    PackageMember {
                        name: "GT_COMPANY_XREF".into(),
                        kind: OracleArtifactKind::Lookup,
                        description: Some("Company cross-reference".into()),
                    },
                    PackageMember {
                        name: "GT_INVOICE_HOLD_RULE".into(),
                        kind: OracleArtifactKind::GroovyScript,
                        description: Some("AP invoice hold".into()),
                    },
                    PackageMember {
                        name: "GT_GL_JOURNAL_EXTRACT".into(),
                        kind: OracleArtifactKind::BipReport,
                        description: Some("GL journal extract".into()),
                    },
                ],
            },
        );
        self.packages.insert(
            "GT_SCM_EXTENSIONS".into(),
            PackageContents {
                package: "GT_SCM_EXTENSIONS".into(),
                description: Some(
                    "Gaussian Technologies SCM Application Composer extensions".into(),
                ),
                members: vec![PackageMember {
                    name: "GT_ITEM_DEFAULTING".into(),
                    kind: OracleArtifactKind::GroovyScript,
                    description: Some("Item attribute defaulting".into()),
                }],
            },
        );

        // Where-used links — the value of impact analysis at demo time.
        self.where_used.insert(
            ("GT_FUSION_ERP_REST".into(), OracleArtifactKind::Connection),
            vec![
                WhereUsedHit {
                    object: "GT_GL_JOURNAL_IMPORT".into(),
                    kind: OracleArtifactKind::Integration,
                    location: "invoke activity 'importJournals'".into(),
                    usage: "invoke".into(),
                },
                WhereUsedHit {
                    object: "GT_PO_RECEIPT_SYNC".into(),
                    kind: OracleArtifactKind::Integration,
                    location: "invoke activity 'postReceipt'".into(),
                    usage: "invoke".into(),
                },
            ],
        );
        self.where_used.insert(
            ("GT_COMPANY_XREF".into(), OracleArtifactKind::Lookup),
            vec![WhereUsedHit {
                object: "GT_GL_JOURNAL_IMPORT".into(),
                kind: OracleArtifactKind::Integration,
                location: "map 'enrichLedger'".into(),
                usage: "read".into(),
            }],
        );

        // Tables for the data-preview surface (Oracle objects)
        self.tables.insert(
            "GL_LEDGERS".into(),
            vec![
                row(&[
                    ("LEDGER_ID", "300100001"),
                    ("NAME", "Gaussian Technologies Primary Ledger"),
                    ("CURRENCY_CODE", "IDR"),
                ]),
                row(&[
                    ("LEDGER_ID", "300100002"),
                    ("NAME", "Gaussian Technologies USD Reporting"),
                    ("CURRENCY_CODE", "USD"),
                ]),
            ],
        );
    }
}

fn prog(
    name: &str,
    kind: OracleArtifactKind,
    package: &str,
    description: &str,
    source: &str,
) -> ProgramSource {
    let line_count = source.lines().count();
    ProgramSource {
        name: name.into(),
        kind,
        package: Some(package.into()),
        description: Some(description.into()),
        source: source.into(),
        active: true,
        line_count,
    }
}

fn row(pairs: &[(&str, &str)]) -> TableRow {
    let mut m = serde_json::Map::new();
    for (k, v) in pairs {
        m.insert((*k).into(), serde_json::Value::String((*v).into()));
    }
    TableRow { values: m }
}

#[async_trait]
impl OicClient for MockOicClient {
    fn destination(&self) -> &OicDestination {
        &self.destination
    }

    async fn get_integration(&self, name: &str) -> OicResult<ProgramSource> {
        get_object(&self.programs, name, OracleArtifactKind::Integration)
    }
    async fn get_groovy_script(&self, name: &str) -> OicResult<ProgramSource> {
        get_object(&self.classes, name, OracleArtifactKind::GroovyScript)
    }
    async fn get_connection(&self, name: &str) -> OicResult<ProgramSource> {
        get_object(&self.interfaces, name, OracleArtifactKind::Connection)
    }
    async fn get_lookup(&self, name: &str) -> OicResult<ProgramSource> {
        get_object(&self.includes, name, OracleArtifactKind::Lookup)
    }
    async fn get_ess_job(&self, group: &str, name: &str) -> OicResult<ProgramSource> {
        self.function_modules
            .get(&(group.to_uppercase(), name.to_string()))
            .cloned()
            .ok_or_else(|| OicError::NotFound {
                kind: "EssJob".into(),
                name: format!("{group}/{name}"),
            })
    }
    async fn get_project_contents(&self, package: &str) -> OicResult<PackageContents> {
        self.packages
            .get(&package.to_uppercase())
            .cloned()
            .ok_or_else(|| OicError::NotFound {
                kind: "Project".into(),
                name: package.into(),
            })
    }
    async fn get_bip_report(&self, name: &str) -> OicResult<CdsView> {
        self.cds_views
            .get(&name.to_uppercase())
            .cloned()
            .ok_or_else(|| OicError::NotFound {
                kind: "BipReport".into(),
                name: name.into(),
            })
    }

    async fn search(&self, request: OicSearchRequest) -> OicResult<Vec<OicSearchHit>> {
        let q = request.query.to_lowercase();
        let terms: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<OicSearchHit> = Vec::new();

        let kind_match =
            |k: OracleArtifactKind| request.kind.map(|wanted| wanted == k).unwrap_or(true);
        let mut push = |name: &str,
                        kind: OracleArtifactKind,
                        desc: Option<&str>,
                        pkg: Option<&str>,
                        score: usize| {
            if kind_match(kind) && score > 0 {
                hits.push(OicSearchHit {
                    name: name.into(),
                    kind,
                    description: desc.map(String::from),
                    package: pkg.map(String::from),
                    score: score as f32,
                });
            }
        };
        let score_of = |hay: &str| -> usize {
            let hay_lc = hay.to_lowercase();
            terms.iter().map(|t| hay_lc.matches(t).count()).sum()
        };

        for (n, p) in &self.programs {
            push(
                n,
                p.kind,
                p.description.as_deref(),
                p.package.as_deref(),
                score_of(&format!(
                    "{n} {} {}",
                    p.description.as_deref().unwrap_or(""),
                    p.package.as_deref().unwrap_or("")
                )),
            );
        }
        for (n, p) in &self.classes {
            push(
                n,
                p.kind,
                p.description.as_deref(),
                p.package.as_deref(),
                score_of(&format!(
                    "{n} {} {}",
                    p.description.as_deref().unwrap_or(""),
                    p.package.as_deref().unwrap_or("")
                )),
            );
        }
        for (n, p) in &self.interfaces {
            push(
                n,
                p.kind,
                p.description.as_deref(),
                p.package.as_deref(),
                score_of(&format!("{n} {}", p.description.as_deref().unwrap_or(""))),
            );
        }
        for ((_g, n), p) in &self.function_modules {
            push(
                n,
                p.kind,
                p.description.as_deref(),
                p.package.as_deref(),
                score_of(&format!("{n} {}", p.description.as_deref().unwrap_or(""))),
            );
        }
        for (n, v) in &self.cds_views {
            push(
                n,
                OracleArtifactKind::BipReport,
                None,
                None,
                score_of(&format!("{n} {}", v.root_entity)),
            );
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(request.max_results.max(1));
        Ok(hits)
    }

    async fn where_used(&self, request: WhereUsedRequest) -> OicResult<Vec<WhereUsedHit>> {
        Ok(self
            .where_used
            .get(&(request.name.to_uppercase(), request.kind))
            .cloned()
            .unwrap_or_default())
    }

    async fn preview_data(&self, table: &str, max_rows: usize) -> OicResult<Vec<TableRow>> {
        if max_rows == 0 || max_rows > MAX_TABLE_ROWS {
            return Err(OicError::InvalidObjectName(format!(
                "max_rows must be in 1..={MAX_TABLE_ROWS}, got {max_rows}"
            )));
        }
        // Some Fusion objects can't be read through the REST/describe surface
        // (subledger detail, large fact tables). Surface the block so the
        // agent falls back to a BI Publisher extract.
        if table.eq_ignore_ascii_case("XLA_AE_LINES") {
            return Err(OicError::DataPreviewBlocked(format!(
                "object {table} is not exposed for direct preview; fall back to a BI Publisher extract (oracle.bip.runReport)",
            )));
        }
        let rows = self
            .tables
            .get(&table.to_uppercase())
            .cloned()
            .ok_or_else(|| OicError::NotFound {
                kind: "BipDataModel".into(),
                name: table.into(),
            })?;
        let mut out = rows;
        out.truncate(max_rows);
        Ok(out)
    }

    async fn activate(
        &self,
        request: ActivationRequest,
        ctx: OicCallContext,
    ) -> OicResult<ActivationOutcome> {
        if ctx.read_only {
            return Err(OicError::PermissionDenied(format!(
                "activate({} {}) blocked: read-only mode",
                request.kind.label(),
                request.name,
            )));
        }
        // Acknowledge activation/publish; in OIC this activates the
        // integration (or publishes the sandbox) and may produce warnings.
        Ok(ActivationOutcome {
            name: request.name.clone(),
            kind: request.kind,
            activated: true,
            messages: vec![format!(
                "{} {} activated (mock)",
                request.kind.label(),
                request.name
            )],
        })
    }
}

fn get_object(
    map: &HashMap<String, ProgramSource>,
    name: &str,
    kind: OracleArtifactKind,
) -> OicResult<ProgramSource> {
    map.get(&name.to_uppercase())
        .cloned()
        .ok_or_else(|| OicError::NotFound {
            kind: kind.label().into(),
            name: name.into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Arc<MockOicClient> {
        MockOicClient::new(OicDestination::mock("dev"))
    }

    #[tokio::test]
    async fn get_program_returns_source() {
        let c = client();
        let p = c.get_integration("gt_gl_journal_import").await.unwrap();
        assert_eq!(p.name, "GT_GL_JOURNAL_IMPORT");
        assert!(p.source.contains("importBulkData"));
        assert!(p.line_count > 0);
    }

    #[tokio::test]
    async fn search_filters_by_kind() {
        let c = client();
        let hits = c
            .search(OicSearchRequest {
                query: "invoice hold".into(),
                kind: Some(OracleArtifactKind::GroovyScript),
                max_results: 20,
            })
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits
            .iter()
            .all(|h| h.kind == OracleArtifactKind::GroovyScript));
        assert!(hits.iter().any(|h| h.name == "GT_INVOICE_HOLD_RULE"));
    }

    #[tokio::test]
    async fn where_used_traces_dependency_chain() {
        let c = client();
        // The connection should report the integrations that invoke it.
        let hits = c
            .where_used(WhereUsedRequest {
                name: "GT_FUSION_ERP_REST".into(),
                kind: OracleArtifactKind::Connection,
            })
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|h| h.object == "GT_GL_JOURNAL_IMPORT"));
        assert!(hits.iter().all(|h| h.usage == "invoke"));
    }

    #[tokio::test]
    async fn data_preview_block_is_surfaced() {
        let c = client();
        let err = c.preview_data("XLA_AE_LINES", 10).await.unwrap_err();
        assert!(matches!(err, OicError::DataPreviewBlocked(_)));
    }

    #[tokio::test]
    async fn activate_blocked_in_read_only() {
        let c = client();
        let err = c
            .activate(
                ActivationRequest {
                    name: "GT_GL_JOURNAL_IMPORT".into(),
                    kind: OracleArtifactKind::Integration,
                },
                OicCallContext { read_only: true },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, OicError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn activate_allowed_when_writes_enabled() {
        let c = client();
        let outcome = c
            .activate(
                ActivationRequest {
                    name: "GT_GL_JOURNAL_IMPORT".into(),
                    kind: OracleArtifactKind::Integration,
                },
                OicCallContext { read_only: false },
            )
            .await
            .unwrap();
        assert!(outcome.activated);
    }

    #[tokio::test]
    async fn package_contents_includes_seeded_objects() {
        let c = client();
        let pkg = c
            .get_project_contents("GT_FINANCE_INTEGRATIONS")
            .await
            .unwrap();
        assert!(pkg.members.iter().any(|m| m.name == "GT_FUSION_ERP_REST"));
        assert!(pkg
            .members
            .iter()
            .any(|m| m.name == "GT_GL_JOURNAL_EXTRACT"));
    }

    #[tokio::test]
    async fn function_module_lookup_uses_group_namespace() {
        let c = client();
        let fm = c.get_ess_job("GL", "JournalImportLauncher").await.unwrap();
        assert_eq!(fm.name, "JournalImportLauncher");
        assert!(fm.source.contains("JournalImportLauncher"));
    }
}
