use std::env;

#[derive(Debug)]
pub struct Config {
    pub template_dir: String,
}

impl Config {
    pub fn env() -> Config {
        let template_dir = env::var("TEMPLATE_DIR").unwrap_or_else(|_| "./templates".to_string());
        Self { template_dir }
    }
}
