mod config;

use std::{fs::read_to_string, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{response::Html, routing, Extension, Router, Server};
use handlebars::Handlebars;
use once_cell::sync::Lazy;
use serde::Serialize;
use tracing::{error, warn};

use config::Config;

static CONFIG: Lazy<Config> = Lazy::new(Config::env);

#[derive(Serialize)]
struct  ErrorReport {
    description: String,
}

async fn root(Extension(handlebars): Extension<Arc<Handlebars<'static>>>) -> Html<String> {
    match handlebars.render("hello", &()) {
        Ok(html) => Html(html),
        Err(e) => {
            error!("template rendering error: {}", e);
            Html(handlebars.render("error", &ErrorReport { description: format!("{}", e)}).unwrap())
        }
    }
}

pub struct Web {
    hb: Arc<Handlebars<'static>>,
    addr: SocketAddr,
}

impl Web {
    pub async fn new() -> Self {
        let hb = {
            let mut hb = Handlebars::new();
            let templates = load_template_dir(&CONFIG.template_dir);

            for (path, content) in templates {
                if let Err(e) = hb.register_template_string(
                    path.file_stem()
                        // Explicitly permit a single unnamed template.
                        .unwrap_or_default()
                        .to_str()
                        .expect("all template file names must be valid UTF-8"),
                    &content,
                ) {
                    error!("error when registering template {}: {}", path.display(), e);
                }
            }

            hb
        };

        let addr = SocketAddr::from(([0, 0, 0, 0], 8080));

        Self {
            hb: Arc::new(hb),
            addr,
        }
    }

    pub async fn run(self) {
        let router = Router::new()
            .route("/", routing::get(root))
            .layer(Extension(self.hb));

        Server::bind(&self.addr)
            .serve(router.into_make_service())
            .await
            .unwrap();
    }
}

fn load_template_dir(dir: &str) -> Vec<(PathBuf, String)> {
    use std::fs::read_dir;
    let mut templates = vec![];

    if let Ok(dirs) = read_dir(dir) {
        let mut dirs: Vec<_> = dirs.filter_map(|e| e.ok()).collect();

        while let Some(entry) = dirs.pop() {
            let meta = entry.metadata().unwrap();

            if meta.is_file() {
                let path = entry.path();

                if "hbs" != path.extension().unwrap_or_default() {
                    continue;
                }

                let content =
                    read_to_string(&path).expect("all template files must be valid UTF-8");
                templates.push((path, content));
            } else {
                debug_assert!(meta.is_dir());
                if let Ok(entries) = entry.path().read_dir() {
                    dirs.extend(entries.filter_map(|e| e.ok()));
                }
            }
        }
    } else {
        warn!("Failed to read template directory {}", CONFIG.template_dir);
    }

    templates
}
