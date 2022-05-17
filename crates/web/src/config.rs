use std::env;

#[derive(Debug)]
pub struct Config {
    pub template_dir: String,
    pub stylesheet_dir: String,
}

impl Config {
    pub fn env() -> Config {
        Self {
            template_dir: env::var("TEMPLATE_DIR").unwrap_or_else(|_| "".to_string()),
            stylesheet_dir: env::var("CSS_DIR").unwrap_or_else(|_| "".to_string()),
        }
    }
}
