use std::net::SocketAddr;
use std::path::PathBuf;

use grm_service_api::GrpcWorkspaceService;
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

    println!(
        "serving local GRM workspace gRPC shell on {addr}; workspace root: {}",
        workspace_root.display()
    );
    Server::builder()
        .add_service(GrpcWorkspaceService::with_local_workspace_root(workspace_root).into_server())
        .serve(addr)
        .await?;

    Ok(())
}
