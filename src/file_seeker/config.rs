/// Configuration management (Everything.ini equivalent)

use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // General Settings
    pub store_settings_in_appdata: bool,
    pub run_as_admin: bool,
    pub run_on_system_startup: bool,
    pub service_enabled: bool,
    pub instance_name: Option<String>,
    pub language_id: u32,

    // Search Settings
    pub match_case: bool,
    pub match_whole_word: bool,
    pub match_path: bool,
    pub match_diacritics: bool,
    pub regex_enabled: bool,
    pub home_search: Option<String>,

    // Index Settings
    pub database_location: Option<String>,
    pub index_date_created: bool,
    pub index_date_modified: bool,
    pub index_date_accessed: bool,
    pub index_attributes: bool,
    pub index_run_count: bool,
    pub exclude_hidden_files: bool,
    pub exclude_system_files: bool,
    pub exclude_list: Vec<String>,

    // NTFS Volumes
    pub ntfs_volumes: Vec<NtfsVolumeConfig>,

    // Folder Indexes
    pub folder_indexes: Vec<String>,

    // File Lists
    pub file_lists: Vec<String>,

    // Window Settings
    pub always_on_top: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub window_title_format: Option<String>,
    pub window_width: u32,
    pub window_height: u32,
    pub window_x: i32,
    pub window_y: i32,

    // Results View
    pub show_thumbnails: bool,
    pub thumbnail_size: u32,
    pub detail_view: bool,

    // Date/Time Format
    pub date_format: Option<String>,
    pub time_format: Option<String>,

    // Fonts & Colors
    pub result_list_font: Option<String>,
    pub result_list_font_size: u32,
    pub search_edit_font: Option<String>,
    pub search_edit_font_size: u32,
    pub status_bar_font: Option<String>,
    pub status_bar_font_size: u32,
    pub header_font: Option<String>,
    pub header_font_size: u32,

    // HTTP Server
    pub http_server_enabled: bool,
    pub http_server_port: u16,
    pub http_server_username: Option<String>,
    pub http_server_password: Option<String>,
    pub http_server_download_enabled: bool,

    // ETP/FTP Server
    pub etp_server_enabled: bool,
    pub etp_server_port: u16,
    pub etp_server_username: Option<String>,
    pub etp_server_password: Option<String>,
    pub etp_welcome_message: Option<String>,

    // Keyboard
    pub keyboard_shortcuts: HashMap<String, String>,

    // History
    pub search_history_enabled: bool,
    pub run_history_enabled: bool,
    pub max_search_history: u32,

    // Context Menu
    pub folder_context_menu_enabled: bool,

    // Advanced
    pub transluscent_selection_alpha: u8,
    pub snap_to_window: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtfsVolumeConfig {
    pub drive_letter: String,
    pub included: bool,
    pub monitor_changes: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            store_settings_in_appdata: false,
            run_as_admin: false,
            run_on_system_startup: false,
            service_enabled: false,
            instance_name: None,
            language_id: 0,
            match_case: false,
            match_whole_word: false,
            match_path: true,
            match_diacritics: false,
            regex_enabled: false,
            home_search: None,
            database_location: None,
            index_date_created: false,
            index_date_modified: true,
            index_date_accessed: false,
            index_attributes: false,
            index_run_count: false,
            exclude_hidden_files: false,
            exclude_system_files: false,
            exclude_list: Vec::new(),
            ntfs_volumes: Vec::new(),
            folder_indexes: Vec::new(),
            file_lists: Vec::new(),
            always_on_top: false,
            fullscreen: false,
            maximized: false,
            minimized: false,
            window_title_format: Some("$s?{$s - }$t$i?{ ($i)}".to_string()),
            window_width: 800,
            window_height: 600,
            window_x: 0,
            window_y: 0,
            show_thumbnails: false,
            thumbnail_size: 256,
            detail_view: true,
            date_format: None,
            time_format: None,
            result_list_font: None,
            result_list_font_size: 14,
            search_edit_font: None,
            search_edit_font_size: 14,
            status_bar_font: None,
            status_bar_font_size: 12,
            header_font: None,
            header_font_size: 12,
            http_server_enabled: false,
            http_server_port: 8080,
            http_server_username: None,
            http_server_password: None,
            http_server_download_enabled: true,
            etp_server_enabled: false,
            etp_server_port: 21,
            etp_server_username: None,
            etp_server_password: None,
            etp_welcome_message: None,
            keyboard_shortcuts: HashMap::new(),
            search_history_enabled: true,
            run_history_enabled: true,
            max_search_history: 100,
            folder_context_menu_enabled: false,
            transluscent_selection_alpha: 128,
            snap_to_window: false,
        }
    }
}

impl Config {
    /// Load configuration from an ini file
    pub fn load(path: &str) -> Result<Self, String> {
        let config_path = std::path::Path::new(path);
        if !config_path.exists() {
            return Ok(Config::default());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))
    }

    /// Save configuration to a file
    pub fn save(&self, path: &str) -> Result<(), String> {
        let content =
            toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(path, content).map_err(|e| format!("Failed to write config: {}", e))
    }

    /// Get the config file path
    pub fn get_config_path() -> PathBuf {
        let mut path = std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        path.push("Everything.ini");
        path
    }

    /// Generate window title based on format
    pub fn generate_window_title(&self, search: Option<&str>, instance: Option<&str>) -> String {
        let format = self
            .window_title_format
            .as_deref()
            .unwrap_or("$s?{$s - }$t$i?{ ($i)}");
        let localized_name = "文件检索助手";
        let mut result = String::new();
        let bytes = format.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'$' && i + 1 < bytes.len() {
                let var = bytes[i + 1];
                match var {
                    b's' => {
                        if let Some(s) = search {
                            if !s.is_empty() {
                                let mut j = i + 2;
                                // Check for conditional format
                                if j < bytes.len() && bytes[j] == b'?' {
                                    // Find the matching }
                                }
                                result.push_str(s);
                            }
                        }
                        i += 2;
                        continue;
                    }
                    b't' => {
                        result.push_str(localized_name);
                        i += 2;
                        continue;
                    }
                    b'i' => {
                        if let Some(inst) = instance {
                            if !inst.is_empty() {
                                result.push_str(inst);
                            }
                        }
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            result.push(bytes[i] as char);
            i += 1;
        }

        result
    }
}

