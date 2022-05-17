//! Web server

mod config;

use std::{
    collections::HashMap,
    fs::read_to_string,
    net::SocketAddr,
    path::{Path as StdPath, PathBuf},
    sync::Arc,
};

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing, Extension, Router, Server,
};
use handlebars::Handlebars;
use once_cell::sync::Lazy;
use serde::Serialize;
use tracing::{error, info, warn};

use comm::session::Api as SessionApi;

use config::Config;

static CONFIG: Lazy<Config> = Lazy::new(Config::env);

#[derive(Serialize)]
struct ErrorReport {
    description: String,
}

pub struct Web {
    templates: Arc<Handlebars<'static>>,
    stylesheets: HashMap<String, String>,
    addr: SocketAddr,
    session_api: Box<dyn SessionApi>,
}

impl Web {
    pub fn new(session_api: Box<dyn SessionApi>) -> Self {
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
            templates: Arc::new(hb),
            stylesheets: load_stylesheet_dir(&CONFIG.stylesheet_dir),
            addr,
            session_api,
        }
    }

    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/", routing::get(url_root))
            .route("/static/css/*path", routing::get(url_static_css))
            .layer(Extension(self))
    }
}

fn load_template_dir(dir: &str) -> Vec<(PathBuf, String)> {
    use std::fs::read_dir;

    info!(
        "Looking for .hbs templates in {}",
        StdPath::new(dir).canonicalize().unwrap().display()
    );

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

fn load_stylesheet_dir(dir: &str) -> HashMap<String, String> {
    use std::fs::read_dir;

    info!(
        "Looking for .css stylesheets in {}",
        StdPath::new(dir).canonicalize().unwrap().display()
    );

    let mut stylesheets = HashMap::new();

    if let Ok(dirs) = read_dir(dir) {
        let mut dirs: Vec<_> = dirs.filter_map(|e| e.ok()).collect();

        while let Some(entry) = dirs.pop() {
            let meta = entry.metadata().unwrap();

            if meta.is_file() {
                let path = entry.path();

                if "css" != path.extension().unwrap_or_default() {
                    continue;
                }

                let content = read_to_string(&path).expect("all CSS files must be valid UTF-8");
                stylesheets.insert(
                    path.file_stem().unwrap().to_str().unwrap().to_string(),
                    content,
                );
            } else {
                debug_assert!(meta.is_dir());
                if let Ok(entries) = entry.path().read_dir() {
                    dirs.extend(entries.filter_map(|e| e.ok()));
                }
            }
        }
    } else {
        warn!("Failed to read CSS directory {}", CONFIG.template_dir);
    }

    stylesheets
}

async fn url_root(Extension(web): Extension<Arc<Web>>) -> Html<String> {
    match web.templates.render("index", &()) {
        Ok(html) => Html(html),
        Err(e) => {
            error!("template rendering error: {}", e);
            Html(
                web.templates
                    .render(
                        "error",
                        &ErrorReport {
                            description: format!("{}", e),
                        },
                    )
                    .unwrap(),
            )
        }
    }
}

async fn url_static_css(
    Extension(web): Extension<Arc<Web>>,
    Path(path): Path<PathBuf>,
) -> Result<impl IntoResponse, StatusCode> {
    let filename = path.file_stem().ok_or(StatusCode::BAD_REQUEST)?;
    let filename = filename.to_str().ok_or(StatusCode::BAD_REQUEST)?;
    println!("client requested {}", filename);
    println!("{:?}", web.stylesheets);
    let content = web.stylesheets.get(filename).ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        content.to_string(),
    ))
}
