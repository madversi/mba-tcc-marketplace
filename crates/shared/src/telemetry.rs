use tracing_subscriber::EnvFilter;

use crate::config::{AppConfig, LogConfig, LogFormat};

pub fn init_tracing(config: &LogConfig) {
    let filter = EnvFilter::try_new(&config.level).unwrap_or_else(|_| EnvFilter::new("info"));

    match config.format {
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init(),
        LogFormat::Pretty => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
}

pub fn init_from_app_config(config: &AppConfig) {
    init_tracing(&config.log)
}
