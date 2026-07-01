use std::sync::Arc;

use axum::Router;
use axum::routing::any_service;
use grm_mcp::{GrmMcpServer, TransportOptions, parse_startup_options, usage};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let options = match parse_startup_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    };
    let transport = options.transport.clone();

    let server = match GrmMcpServer::from_startup_options(options).await {
        Ok(server) => server,
        Err(err) => {
            eprintln!("failed to initialize grm-mcp: {err}");
            std::process::exit(1);
        }
    };

    match transport {
        TransportOptions::Stdio => match server.serve(stdio()).await {
            Ok(service) => {
                if let Err(err) = service.waiting().await {
                    eprintln!("grm-mcp server error: {err}");
                    std::process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("failed to start grm-mcp: {err}");
                std::process::exit(1);
            }
        },
        TransportOptions::Http {
            bind,
            path,
            allowed_hosts,
        } => {
            let listener = match TcpListener::bind(bind).await {
                Ok(listener) => listener,
                Err(err) => {
                    eprintln!("failed to bind grm-mcp HTTP listener on {bind}: {err}");
                    std::process::exit(1);
                }
            };
            let local_addr = listener.local_addr().unwrap_or(bind);
            let http_service = StreamableHttpService::new(
                move || Ok(server.clone()),
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default().with_allowed_hosts(allowed_hosts),
            );
            let app = Router::new().route(&path, any_service(http_service));
            eprintln!("grm-mcp Streamable HTTP listening on http://{local_addr}{path}");
            if let Err(err) = axum::serve(listener, app).await {
                eprintln!("grm-mcp HTTP server error: {err}");
                std::process::exit(1);
            }
        }
    }
}
