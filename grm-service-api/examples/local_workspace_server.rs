use std::net::SocketAddr;
use std::path::PathBuf;

use grm_service_api::{GrpcServerTlsOptions, GrpcWorkspaceService};
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let addr: SocketAddr = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:50051".into())
        .parse()?;
    let workspace_root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("grm-service-workspaces"));
    std::fs::create_dir_all(&workspace_root)?;

    let service = GrpcWorkspaceService::with_local_workspace_root(&workspace_root).into_server();
    let tls = GrpcServerTlsOptions::from_env()?;
    let mut server = Server::builder();
    let transport = if let Some(tls) = tls {
        let requires_client_authentication = tls.requires_client_authentication();
        server = server.tls_config(tls.server_tls_config()?)?;
        if requires_client_authentication {
            "mutual TLS"
        } else {
            "TLS"
        }
    } else {
        "insecure local gRPC"
    };

    println!(
        "serving local GRM workspace gRPC shell on {addr}; workspace root: {}; transport: {transport}",
        workspace_root.display()
    );
    server.add_service(service).serve(addr).await?;

    Ok(())
}
