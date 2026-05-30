//! Runnable mock Oracle Integration Cloud (OIC) pod.
//!
//! ```text
//! # start the mock OIC pod
//! cargo run -p oracle-automate-oic-mock -- --bind 127.0.0.1:8089
//!
//! # point the MCP server at it via a destination TOML, e.g.
//! #   ~/.config/oracle-automate/destinations/mock-oic.toml
//! #     base_url = "http://127.0.0.1:8089"
//! #     client   = "100"
//! #     [auth]    type = "basic"   user = "demo"   password = "demo"
//! ORACLE_AUTOMATE_DESTINATION=mock-oic cargo run -p oracle-automate-server
//! ```

use clap::Parser;
use oracle_automate_oic_mock::{router, MockConfig};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "oracle-automate-oic-mock",
    about = "Mock Oracle Integration Cloud (OIC) / custom-code REST API. Point an OIC destination at it; swap to a real pod later by changing base_url."
)]
struct Cli {
    /// Listener bind address.
    #[arg(long, default_value = "127.0.0.1:8089")]
    bind: String,

    /// Inject fixed per-request latency (ms) — for timeout tuning.
    #[arg(long, default_value_t = 0)]
    latency_ms: u64,

    /// Accept requests without an Authorization header (a real pod rejects them).
    #[arg(long)]
    no_auth: bool,

    /// Self-probe `/healthz` and exit 0/1 — for Docker/k8s health checks on the
    /// distroless image (which has no shell or curl).
    #[arg(long)]
    healthcheck: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.healthcheck {
        std::process::exit(if probe_healthz(&cli.bind).await { 0 } else { 1 });
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
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
        "mock Oracle Integration Cloud pod listening"
    );
    axum::serve(listener, app).await?;
    Ok(())
}

/// Connect to the bound port and `GET /healthz`; true iff a `200` comes back.
/// Pure std/tokio — no HTTP client dependency, so it works in the distroless image.
async fn probe_healthz(bind: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let addr = bind.replace("0.0.0.0", "127.0.0.1");
    let Ok(mut stream) = tokio::net::TcpStream::connect(&addr).await else {
        return false;
    };
    if stream
        .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        stream.read_to_end(&mut buf),
    )
    .await;
    String::from_utf8_lossy(&buf).contains("200")
}
