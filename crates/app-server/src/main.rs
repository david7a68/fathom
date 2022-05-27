use std::net::SocketAddr;

use lib_rpc::auth::grpc::AuthServer;
use service_auth::AuthService;
use tokio::signal::ctrl_c;
use tonic::transport::Server;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting Fathom app-server");

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let server = Server::builder()
        .add_service(AuthServer::from_provider(AuthService::new()))
        .serve_with_shutdown(addr, async {
            let _ = ctrl_c().await;
            info!("Shutting Down...");
        });

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }
}
