use std::{net::SocketAddr, sync::Arc};

use axum::Server;
use rest::RestApi;
use tracing::{info, error};

use session::Session;
use web::Web;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting fathom");
    info!(
        "Current working directory: {}",
        std::env::current_dir().unwrap().canonicalize().unwrap().display()
    );

    let session = Arc::new(Session::new());
    let rest = Arc::new(RestApi::new(session.clone()));
    let web = Arc::new(Web::new_from_env());

    let routes = rest.routes().merge(web.routes());

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let server = Server::bind(&addr).serve(routes.into_make_service());
    let server = server.with_graceful_shutdown(async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler")
    });
    if let Err(e) = server.await {
        error!("server error: {}", e);
    }
}
