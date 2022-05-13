use tracing::info;
use web::Web;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting fathom");
    info!("Current working directory: {}", std::env::current_dir().unwrap().display());

    let web = Web::new().await;
    web.run().await;
}
