use tracing::info;

use session::Session;
use web::Web;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting fathom");
    info!(
        "Current working directory: {}",
        std::env::current_dir().unwrap().display()
    );

    let session = Session::new();
    let web = Web::new(&session);
    web.run().await;
}
