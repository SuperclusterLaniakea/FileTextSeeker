/// Database management - load/save the Everything database (everything.db)

use std::path::{Path, PathBuf};
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use bzip2::Compression;
use std::io::{Read, Write};
use serde::{Deserialize, Serialize};
use crate::file_seeker::types::FileEntry;

/// Database header structure matching Everything's format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    pub header: DatabaseHeaderInfo,
    pub entries: Vec<FileEntry>,
    pub exclude_list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHeaderInfo {
    pub magic: String,
    pub version: String,
    pub flags: u32,
    pub folder_count: u32,
    pub file_count: u32,
}

impl Database {
    /// Create a new empty database
    pub fn new() -> Self {
        Self {
            header: DatabaseHeaderInfo {
                magic: "EZDB".to_string(),
                version: "1.6.6".to_string(),
                flags: 0,
                folder_count: 0,
                file_count: 0,
            },
            entries: Vec::new(),
            exclude_list: Vec::new(),
        }
    }

    /// Save the database to disk
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string(&self)
            .map_err(|e| format!("Failed to serialize database: {}", e))?;

        // Apply BZIP compression (matching Everything's approach)
        let mut encoder = BzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(json.as_bytes())
            .map_err(|e| format!("Failed to compress database: {}", e))?;
        let compressed = encoder.finish()
            .map_err(|e| format!("Failed to finish compression: {}", e))?;

        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| format!("Failed to create database directory: {}", e))?;
        std::fs::write(path, &compressed)
            .map_err(|e| format!("Failed to write database: {}", e))?;

        Ok(())
    }

    /// Load the database from disk
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Database::new());
        }

        let compressed = std::fs::read(path)
            .map_err(|e| format!("Failed to read database: {}", e))?;

        let mut decoder = BzDecoder::new(&compressed[..]);
        let mut json = String::new();
        decoder.read_to_string(&mut json)
            .map_err(|e| format!("Failed to decompress database: {}", e))?;

        let mut db: Database = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to deserialize database: {}", e))?;

        // Update header counts
        db.header.file_count = db.entries.iter().filter(|e| !e.is_directory).count() as u32;
        db.header.folder_count = db.entries.iter().filter(|e| e.is_directory).count() as u32;

        Ok(db)
    }

    /// Get the default database path
    pub fn default_path() -> PathBuf {
        let mut path = std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        path.push("Everything.db");
        path
    }
}

