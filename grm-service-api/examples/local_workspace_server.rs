use std::net::SocketAddr;

use grm_service_api::GrpcWorkspaceService;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:50051".into())
        .parse()?;

    println!("serving local GRM workspace gRPC shell on {addr}");
    Server::builder()
        .add_service(GrpcWorkspaceService::new().into_server())
        .serve(addr)
        .await?;

    Ok(())
}
