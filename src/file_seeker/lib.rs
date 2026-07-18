/// 文件检索助手- A filename search engine for Rust
///
/// This is a reimplementation of the Everything search engine
/// with all major features 1:1 replicated.

pub mod engine;
pub mod types;
pub mod config;
pub mod cli;
pub mod file_list;
pub mod history;
pub mod rename;
pub mod http_server;
pub mod etp;
pub mod sdk;
pub mod tray;
pub mod autostart;
pub mod watcher;
pub mod ftp;

#[cfg(feature = "gui")]
pub mod gui;

use std::sync::Arc;
use engine::Engine;

/// Initialize the 文件检索助手engine
pub fn initialize() -> Engine {
    let engine = Engine::new();

    // Load configuration
    let config_path = config::Config::get_config_path();
    if let Ok(cfg) = config::Config::load(&config_path.to_string_lossy()) {
        *engine.config.write().unwrap() = cfg;
    }

    engine
}

/// Get version string
pub fn version() -> &'static str {
    "1.0.0"
}

/// Get build info
pub fn build_info() -> String {
    format!(
        "文件检索助手v{} ({} bits) on {}",
        version(),
        std::mem::size_of::<usize>() * 8,
        std::env::consts::OS
    )
}

