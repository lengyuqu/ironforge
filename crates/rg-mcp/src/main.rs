//! `ironforge-mcp` – MCP server entry point.
//!
//! # Transports
//! - **stdio** (default) – run as subprocess of an AI agent. The token is the
//!   process's own `IRONFORGE_PAT`.
//! - **streamable HTTP** (`--http [ADDR]`, default `127.0.0.1:8765`, or
//!   `IRONFORGE_MCP_LISTEN`) – for agents on another machine. Every connection
//!   brings its own `Authorization: Bearer <token>`; `IRONFORGE_PAT` is not used.
//!   See `rg_mcp::remote` and `.ai/README.md`.
//!
//! The legacy HTTP+SSE transport (`--sse`) is not implemented.
//!
//! # Environment
//! | Variable                       | Default                 | Notes                          |
//! |--------------------------------|-------------------------|--------------------------------|
//! | `IRONFORGE_URL`                | `http://localhost:8080` | IronForge API base             |
//! | `IRONFORGE_PAT`                | _(none)_                | stdio only: Bearer token       |
//! | `IRONFORGE_MCP_LISTEN`         | `127.0.0.1:8765`        | `--http` bind address          |
//! | `IRONFORGE_MCP_ALLOWED_ORIGINS`| _(none)_                | `--http`: browser origins      |

use std::io::{self, BufRead, BufWriter, Write};
use std::io::{stdin, stdout};

// Pull everything from the library crate.
use rg_mcp::protocol::*;
use rg_mcp::server::dispatch;
use rg_mcp::AppState;

// ── stdio main loop ─────────────────────────────────────────────

fn run_stdio(state: &AppState) -> io::Result<()> {
    let stdin = stdin();
    let stdout = stdout();
    let reader = io::BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("read error: {}", e);
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = make_error(
                    serde_json::Value::Null,
                    -32700,
                    &format!("parse error: {}", e),
                );
                write_json(&mut writer, &resp)?;
                continue;
            }
        };

        let resp = dispatch(state, &req);
        write_json(&mut writer, &resp)?;
    }
    Ok(())
}

fn write_json<W: Write>(w: &mut W, resp: &JsonRpcResponse) -> io::Result<()> {
    let s =
        serde_json::to_string(resp).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", s)?;
    w.flush()?;
    Ok(())
}

fn init_logging() {
    // 日志打到 stderr，不污染 stdio JSON-RPC 通道
    let _ = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

/// `--http` takes an optional address as the next argument.
fn http_listen_address(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == "--http")?;
    let next = args.get(at + 1).filter(|a| !a.starts_with("--")).cloned();
    Some(
        next.or_else(|| std::env::var("IRONFORGE_MCP_LISTEN").ok())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "127.0.0.1:8765".to_string()),
    )
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--sse") {
        anyhow::bail!(
            "the legacy HTTP+SSE transport is not implemented; use stdio (no flag) or --http (streamable HTTP)"
        );
    }

    if let Some(address) = http_listen_address(&args) {
        init_logging();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return runtime.block_on(async move {
            let cfg = rg_mcp::remote::RemoteConfig::from_env();
            if std::env::var("IRONFORGE_PAT").is_ok_and(|p| !p.is_empty()) {
                tracing::warn!("IRONFORGE_PAT is ignored in --http mode: each client sends its own token");
            }
            let listener = tokio::net::TcpListener::bind(&address).await?;
            let local = listener.local_addr()?;
            if !local.ip().is_loopback() {
                tracing::warn!(
                    "listening on {local} without TLS: tokens travel in clear text unless a reverse proxy in front terminates TLS"
                );
            }
            tracing::info!("ironforge-mcp streamable HTTP on http://{local}/mcp -> {}", cfg.api_base);
            rg_mcp::remote::serve(listener, cfg).await?;
            Ok(())
        });
    }

    // Tools/resources are dispatched synchronously but perform async reqwest
    // calls via `Handle::current().block_on(...)`. Create and enter a runtime
    // for the whole stdio loop so those handlers never panic due to a missing
    // Tokio context.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let _runtime_guard = runtime.enter();

    init_logging();

    let app_state = AppState::from_env()?;
    run_stdio(&app_state)?;
    Ok(())
}
