use std::env;

#[derive(Debug)]
pub struct Config {
    pub asset_dir: String,
}

impl Config {
    pub fn env() -> Config {
        Self {
            asset_dir: env::var("ASSET_DIR").unwrap_or_else(|_| "".to_string()),
        }
    }
}
