//! Runnable mock Oracle Fusion Cloud ERP pod.
//!
//! ```text
//! # start the mock pod
//! cargo run -p oracle-automate-fusion-mock -- --bind 127.0.0.1:8088
//!
//! # point the MCP server at it (swap to a real pod by changing the URL)
//! ORACLE_FUSION_BASE_URL=http://127.0.0.1:8088 \
//! ORACLE_FUSION_AUTH=basic ORACLE_FUSION_USER=demo ORACLE_FUSION_PASSWORD=demo \
//!   cargo run -p oracle-automate-server
//! ```

use clap::Parser;
use oracle_automate_fusion_mock::{router, MockConfig};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "oracle-automate-fusion-mock",
    about = "Mock Oracle Fusion Cloud ERP REST API. Point ORACLE_FUSION_BASE_URL at it; swap to a real pod later by changing the URL."
)]
struct Cli {
    /// Listener bind address.
    #[arg(long, default_value = "127.0.0.1:8088")]
    bind: String,

    /// Inject fixed per-request latency (ms) — for timeout / circuit-breaker
    /// tuning (Phase 5).
    #[arg(long, default_value_t = 0)]
    latency_ms: u64,

    /// Accept requests without an Authorization header (a real pod rejects them).
    #[arg(long)]
    no_auth: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg = MockConfig {
        latency_ms: cli.latency_ms,
        require_auth: !cli.no_auth,
    };
    let app = router(cfg);

    let listener = tokio::net::TcpListener::bind(&cli.bind).await?;
    tracing::info!(
        bind = %cli.bind,
        latency_ms = cli.latency_ms,
        require_auth = !cli.no_auth,
        "mock Oracle Fusion pod listening"
    );
    axum::serve(listener, app).await?;
    Ok(())
}
