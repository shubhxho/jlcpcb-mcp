//! `jlcpcb-mcp` binary: serves the MCP tools over stdio.

use jlcpcb_mcp::{Config, JlcpcbServer, Service};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // stdout is the MCP channel: logs must go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("JLCPCB_MCP_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let config = Config::from_env();
    let official = config.credentials.is_some();
    let server = JlcpcbServer::new(Service::new(config));
    tracing::info!(official_api = official, "starting jlcpcb-mcp on stdio");
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

fn print_help() {
    println!(
        "jlcpcb-mcp {}\n\nMCP server (stdio) for JLCPCB / LCSC parts search.\n\
Run it from an MCP client such as Claude Desktop or Cursor; it speaks JSON-RPC on stdin/stdout.\n\n\
Environment:\n  JLCPCB_APP_ID / JLCPCB_API_KEY / JLCPCB_API_SECRET  optional official API credentials\n  \
JLCPCB_MCP_TIMEOUT_SECS  per-request timeout (default 20)\n  JLCPCB_MCP_LOG  log filter for stderr (default warn)",
        env!("CARGO_PKG_VERSION")
    );
}

#[tokio::main]
async fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("-h") | Some("--help") => {
            print_help();
            return;
        }
        Some("-V") | Some("--version") => {
            println!("jlcpcb-mcp {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    if let Err(e) = run().await {
        eprintln!("jlcpcb-mcp: {e}");
        std::process::exit(1);
    }
}
