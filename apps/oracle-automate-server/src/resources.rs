//! MCP resources (paper §IV-F).
//!
//! Phase 2 publishes four read-only resources:
//!   - `oracle-erp://info`               — live system identity
//!   - `oracle-object://{name}/structure`    — DDIC structure (one resource per
//!     seeded table)
//!   - `oracle-rest://{name}`                — REST operation metadata (one per
//!     seeded function)
//!   - `agents://guardrails`             — the loaded AGENTS.md, if any
//!
//! Each resource is enumerated at startup so MCP clients see them in
//! `resources/list` and can fetch them via `resources/read` without making
//! tool calls.

use crate::context::ServerContext;
use mcp_core::{Error, ReadResourceResult, Resource, ResourceContents};
use mcp_server::{registry::ResourceHandler, ResourceDescriptor};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub fn all(ctx: &Arc<ServerContext>) -> Vec<ResourceDescriptor> {
    let mut out = Vec::new();

    out.push(make_system_info(ctx));

    // One resource per seeded Oracle object.
    for table in &["EGP_SYSTEM_ITEMS_B", "GL_LEDGERS", "DOO_HEADERS_ALL"] {
        out.push(make_table_structure(ctx, table));
    }

    // One resource per seeded Oracle REST operation.
    for rfc in &[
        "fusion.system.serverInformation",
        "fusion.scm.itemsV2.get",
        "fusion.gl.journalEntries.post",
        "fusion.doo.salesOrdersForOrderHub.post",
        "fusion.bip.runReport",
        "fusion.rest.describe",
    ] {
        out.push(make_rfc_meta(ctx, rfc));
    }

    out.push(make_adt_destination(ctx));

    if ctx.metadata_cache.is_some() {
        out.push(make_cache_stats(ctx));
    }

    if ctx.agents_md.is_some() {
        out.push(make_agents_md(ctx));
    }
    out
}

fn make_cache_stats(ctx: &Arc<ServerContext>) -> ResourceDescriptor {
    struct H(Arc<ServerContext>);
    impl ResourceHandler for H {
        fn read(
            &self,
            uri: &str,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<ReadResourceResult>> + Send + '_>>
        {
            let uri = uri.to_string();
            let ctx = Arc::clone(&self.0);
            Box::pin(async move {
                let cache = ctx
                    .metadata_cache
                    .as_ref()
                    .ok_or_else(|| Error::Other("metadata cache disabled".into()))?;
                let s = cache.stats().await;
                let text = serde_json::to_string_pretty(&serde_json::json!({
                    "hits": s.hits,
                    "misses": s.misses,
                    "entries": s.entries,
                    "evictions": s.evictions,
                    "hit_ratio": s.hit_ratio(),
                }))
                .map_err(Error::Json)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents {
                        uri,
                        mime_type: Some("application/json".into()),
                        text: Some(text),
                        blob: None,
                    }],
                })
            })
        }
    }
    ResourceDescriptor {
        resource: Resource {
            uri: "oracle-cache://stats".into(),
            name: "REST operation-metadata cache stats".into(),
            description: Some("Live hit/miss counters for the REST operation-metadata cache (a reference REST-metadata-cache design pattern). JSON.".into()),
            mime_type: Some("application/json".into()),
        },
        handler: Arc::new(H(Arc::clone(ctx))),
    }
}

fn make_adt_destination(ctx: &Arc<ServerContext>) -> ResourceDescriptor {
    struct H(Arc<ServerContext>);
    impl ResourceHandler for H {
        fn read(
            &self,
            uri: &str,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<ReadResourceResult>> + Send + '_>>
        {
            let uri = uri.to_string();
            let dest = self.0.adt_client.destination().redacted();
            Box::pin(async move {
                let text = serde_json::to_string_pretty(&dest).map_err(Error::Json)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents {
                        uri,
                        mime_type: Some("application/json".into()),
                        text: Some(text),
                        blob: None,
                    }],
                })
            })
        }
    }
    ResourceDescriptor {
        resource: Resource {
            uri: "oic-connection://info".into(),
            name: "ADT destination".into(),
            description: Some("Redacted view of the configured Oracle Integration Cloud (OIC) connection (name, base URL, client, language, auth type).".into()),
            mime_type: Some("application/json".into()),
        },
        handler: Arc::new(H(Arc::clone(ctx))),
    }
}

fn make_system_info(ctx: &Arc<ServerContext>) -> ResourceDescriptor {
    struct H(Arc<ServerContext>);
    impl ResourceHandler for H {
        fn read(
            &self,
            uri: &str,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<ReadResourceResult>> + Send + '_>>
        {
            let uri = uri.to_string();
            let ctx = Arc::clone(&self.0);
            Box::pin(async move {
                let info = ctx
                    .erp_client
                    .system_info()
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                let text = serde_json::to_string_pretty(&info).map_err(Error::Json)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents {
                        uri,
                        mime_type: Some("application/json".into()),
                        text: Some(text),
                        blob: None,
                    }],
                })
            })
        }
    }
    ResourceDescriptor {
        resource: Resource {
            uri: "oracle-erp://info".into(),
            name: "Oracle environment identity".into(),
            description: Some("Live SID, client, release, host, and identity. JSON.".into()),
            mime_type: Some("application/json".into()),
        },
        handler: Arc::new(H(Arc::clone(ctx))),
    }
}

fn make_table_structure(ctx: &Arc<ServerContext>, table: &str) -> ResourceDescriptor {
    struct H {
        ctx: Arc<ServerContext>,
        table: String,
    }
    impl ResourceHandler for H {
        fn read(
            &self,
            uri: &str,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<ReadResourceResult>> + Send + '_>>
        {
            let uri = uri.to_string();
            let ctx = Arc::clone(&self.ctx);
            let table = self.table.clone();
            Box::pin(async move {
                let s = ctx
                    .erp_client
                    .table_structure(&table)
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                let text = serde_json::to_string_pretty(&s).map_err(Error::Json)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents {
                        uri,
                        mime_type: Some("application/json".into()),
                        text: Some(text),
                        blob: None,
                    }],
                })
            })
        }
    }
    ResourceDescriptor {
        resource: Resource {
            uri: format!("oracle-object://{table}/structure"),
            name: format!("DDIC structure of {table}"),
            description: Some(format!("Field metadata for SAP table {table}.")),
            mime_type: Some("application/json".into()),
        },
        handler: Arc::new(H {
            ctx: Arc::clone(ctx),
            table: table.into(),
        }),
    }
}

fn make_rfc_meta(ctx: &Arc<ServerContext>, function: &str) -> ResourceDescriptor {
    struct H {
        ctx: Arc<ServerContext>,
        function: String,
    }
    impl ResourceHandler for H {
        fn read(
            &self,
            uri: &str,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<ReadResourceResult>> + Send + '_>>
        {
            let uri = uri.to_string();
            let ctx = Arc::clone(&self.ctx);
            let function = self.function.clone();
            Box::pin(async move {
                let meta = ctx
                    .erp_client
                    .operation_metadata(&function, "EN")
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                let text = serde_json::to_string_pretty(&meta).map_err(Error::Json)?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents {
                        uri,
                        mime_type: Some("application/json".into()),
                        text: Some(text),
                        blob: None,
                    }],
                })
            })
        }
    }
    ResourceDescriptor {
        resource: Resource {
            uri: format!("oracle-rest://{function}"),
            name: format!("REST operation metadata: {function}"),
            description: Some(format!(
                "Parameter signature and read-only flag for {function}."
            )),
            mime_type: Some("application/json".into()),
        },
        handler: Arc::new(H {
            ctx: Arc::clone(ctx),
            function: function.into(),
        }),
    }
}

fn make_agents_md(ctx: &Arc<ServerContext>) -> ResourceDescriptor {
    struct H(Arc<ServerContext>);
    impl ResourceHandler for H {
        fn read(
            &self,
            uri: &str,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<ReadResourceResult>> + Send + '_>>
        {
            let uri = uri.to_string();
            let text = self.0.agents_md.clone().unwrap_or_default();
            Box::pin(async move {
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents {
                        uri,
                        mime_type: Some("text/markdown".into()),
                        text: Some(text),
                        blob: None,
                    }],
                })
            })
        }
    }
    ResourceDescriptor {
        resource: Resource {
            uri: "agents://guardrails".into(),
            name: "Agent guardrails".into(),
            description: Some(
                "Project-local AGENTS.md, surfaced from disk on server start.".into(),
            ),
            mime_type: Some("text/markdown".into()),
        },
        handler: Arc::new(H(Arc::clone(ctx))),
    }
}
