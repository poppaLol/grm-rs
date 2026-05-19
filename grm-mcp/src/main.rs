use grm_mcp::{GrmMcpServer, parse_startup_options, usage};
use rmcp::ServiceExt;
use rmcp::transport::stdio;

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

    let server = match GrmMcpServer::from_startup_options(options).await {
        Ok(server) => server,
        Err(err) => {
            eprintln!("failed to initialize grm-mcp: {err}");
            std::process::exit(1);
        }
    };

    match server.serve(stdio()).await {
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
    }
}
