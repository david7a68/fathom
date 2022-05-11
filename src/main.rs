mod config;

use std::{fs::read_to_string, net::SocketAddr};

use axum::{routing, Router, Server, response::Html};
use handlebars::Handlebars;
use once_cell::sync::Lazy;
use tracing::{info, log::warn};

use crate::config::Config;

static CONFIG: Lazy<Config> = Lazy::new(Config::env);

static HANDLEBARS: Lazy<Handlebars<'static>> = Lazy::new(|| {
    use std::fs::read_dir;

    info!("Initializing Handlebars");
    info!("Searching for Handlebars templates in {}", CONFIG.template_dir);

    let mut b = Handlebars::new();
    b.set_strict_mode(true);

    let mut dirs: Vec<_> = read_dir(&CONFIG.template_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    while let Some(entry) = dirs.pop() {
        let meta = entry.metadata().unwrap();

        if meta.is_file() {
            let path = entry.path();

            if "hbs" != path.extension().unwrap() {
                continue;
            }

            let name_str = path.file_stem()
                .unwrap().to_str().expect("all template file names must be valid UTF-8");

            let content =
                read_to_string(entry.path()).expect("all template files must be valid UTF-8");

            b.register_template_string(name_str, &content)
                .expect("template parsing failed");

            info!("Registered template: {} at {}", name_str, entry.path().display());
        } else {
            debug_assert!(meta.is_dir());
            if let Ok(entries) = entry.path().read_dir() {
                dirs.extend(entries.filter_map(|e| e.ok()));
            }
        }
    }

    let num_templates = b.get_templates().len();
    if num_templates == 0 {
        warn!("No handlebars templates found in {}. Is this the correct directory?", CONFIG.template_dir);
    } else {
        info!("Found {} handlebars templates", num_templates);
    }

    info!("Handlebars template registration complete.");

    b
});

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Initialize handlebars, allowing it to make use of synchronous resources.
    // We don't need access to handlebars until the application is running.
    let hb_init = tokio::task::spawn_blocking(|| Lazy::force(&HANDLEBARS));

    let app = Router::new().route("/", routing::get(root));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));

    // Make sure that handlebars is initialized before we start serving
    // requests.
    hb_init.await.unwrap();

    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn root() -> Html<String> {
    Html(HANDLEBARS.render("hello", &()).unwrap())
}
