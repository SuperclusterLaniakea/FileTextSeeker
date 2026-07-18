/// File List (EFU) handling - load, save, create EFU files
///
/// EFU (Everything File List) is a CSV format:
/// filename,size,date_created,date_modified,date_accessed,attributes

use std::path::Path;
use csv::{ReaderBuilder, WriterBuilder};
use crate::file_seeker::types::{FileEntry, FileAttributes};

pub struct FileList {
    pub entries: Vec<FileListEntry>,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct FileListEntry {
    pub filename: String,
    pub size: u64,
    pub date_created: Option<String>,
    pub date_modified: Option<String>,
    pub attributes: String,
}

impl FileList {
    /// Create a new file list
    pub fn new(filename: &str) -> Self {
        Self {
            entries: Vec::new(),
            filename: filename.to_string(),
        }
    }

    /// Load an EFU file list
    pub fn load(path: &str) -> Result<Self, String> {
        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .from_path(path)
            .map_err(|e| format!("Failed to open EFU file: {}", e))?;

        let mut entries = Vec::new();

        for result in reader.records() {
            let record = result.map_err(|e| format!("Failed to read EFU record: {}", e))?;

            if record.len() < 2 {
                continue;
            }

            let entry = FileListEntry {
                filename: record.get(0).unwrap_or("").to_string(),
                size: record.get(1).unwrap_or("0").parse().unwrap_or(0),
                date_created: record.get(2).map(|s| s.to_string()),
                date_modified: record.get(3).map(|s| s.to_string()),
                attributes: record.get(5).unwrap_or("").to_string(),
            };

            entries.push(entry);
        }

        Ok(Self {
            entries,
            filename: path.to_string(),
        })
    }

    /// Save to EFU file
    pub fn save(&self, path: &str) -> Result<(), String> {
        let mut writer = WriterBuilder::new()
            .from_path(path)
            .map_err(|e| format!("Failed to create EFU file: {}", e))?;

        writer.write_record(&["filename", "size", "date_created", "date_modified", "date_accessed", "attributes"])
            .map_err(|e| format!("Failed to write header: {}", e))?;

        for entry in &self.entries {
            writer.write_record(&[
                &entry.filename,
                &entry.size.to_string(),
                entry.date_created.as_deref().unwrap_or(""),
                entry.date_modified.as_deref().unwrap_or(""),
                "",
                &entry.attributes,
            ]).map_err(|e| format!("Failed to write record: {}", e))?;
        }

        writer.flush().map_err(|e| format!("Failed to flush: {}", e))?;
        Ok(())
    }

    /// Convert FileList entries to FileEntry types for searching
    pub fn to_file_entries(&self) -> Vec<FileEntry> {
        let mut entries = Vec::new();

        for entry in &self.entries {
            let path = std::path::Path::new(&entry.filename);
            let file_entry = FileEntry {
                full_path: path.to_path_buf(),
                file_name: path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                extension: path.extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default(),
                parent_path: path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default(),
                size: entry.size,
                is_directory: entry.attributes.contains('D'),
                file_list_filename: Some(self.filename.clone()),
                ..Default::default()
            };

            entries.push(file_entry);
        }

        entries
    }

    /// Create a file list from scanning a directory path
    pub fn create_from_path(output_path: &str, scan_path: &str) -> Result<Self, String> {
        let mut file_list = FileList::new(output_path);

        for entry in walkdir::WalkDir::new(scan_path) {
            match entry {
                Ok(entry) => {
                    let metadata = match entry.metadata() {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    let list_entry = FileListEntry {
                        filename: entry.path().to_string_lossy().to_string(),
                        size: metadata.len(),
                        date_created: metadata.created().ok()
                            .map(|t| t.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs().to_string())
                                .unwrap_or_default()),
                        date_modified: metadata.modified().ok()
                            .map(|t| t.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs().to_string())
                                .unwrap_or_default()),
                        attributes: String::new(),
                    };

                    file_list.entries.push(list_entry);
                }
                Err(_) => continue,
            }
        }

        file_list.save(output_path)?;
        Ok(file_list)
    }
}

