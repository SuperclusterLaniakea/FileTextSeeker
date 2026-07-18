/// File system indexer - walks directories and builds the file index

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use chrono::{DateTime, Utc};
use walkdir::WalkDir;
use crate::file_seeker::types::{FileEntry, FileAttributes};

/// Index a folder and all its subfolders
pub fn index_folder(path: &Path, include_system: bool) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();
    let root = path.to_path_buf();

    for entry in WalkDir::new(path).into_iter().filter_entry(move |e| {
        // Always allow the root entry itself, so its children can be visited.
        // On Windows, drive roots (C:\, D:\, etc.) have system+hidden attributes,
        // which would cause filter_entry to prune the entire subtree.
        if e.path() == root {
            return true;
        }
        if !include_system {
            // Skip hidden and system files by default
            if let Ok(metadata) = e.metadata() {
                let attrs = get_file_attributes(&metadata);
                if attrs.hidden || attrs.system {
                    return false;
                }
            }
        }
        true
    }) {
        match entry {
            Ok(entry) => {
                let path = entry.path().to_path_buf();
                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let file_entry = FileEntry {
                    full_path: path.clone(),
                    file_name: entry.file_name().to_string_lossy().to_string(),
                    extension: entry.path().extension()
                        .map(|e| e.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    parent_path: entry.path().parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default(),
                    size: metadata.len(),
                    date_created: metadata.created().ok()
                        .and_then(|t| {
                            let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                            Some(DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos()).unwrap())
                        }),
                    date_modified: metadata.modified().ok()
                        .and_then(|t| {
                            let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                            Some(DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos()).unwrap())
                        }),
                    date_accessed: metadata.accessed().ok()
                        .and_then(|t| {
                            let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                            Some(DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos()).unwrap())
                        }),
                    is_directory: metadata.is_dir(),
                    attributes: get_file_attributes(&metadata),
                    ..Default::default()
                };

                entries.push(file_entry);
            }
            Err(_) => continue,
        }
    }

    Ok(entries)
}

/// Index a folder using multi-threaded walker for better performance
pub fn index_folder_fast(path: &Path) -> Result<Vec<FileEntry>, String> {
    let mut entries = Vec::new();

    for entry in jwalk::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let file_entry = FileEntry {
            full_path: path.clone(),
            file_name: entry.file_name().to_string_lossy().to_string(),
            extension: path.extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default(),
            parent_path: path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
            size: metadata.len(),
            date_modified: metadata.modified().ok()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok()
                        .map(|d| DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos()).unwrap())
                }),
            is_directory: metadata.is_dir(),
            attributes: get_file_attributes(&metadata),
            ..Default::default()
        };

        entries.push(file_entry);
    }

    Ok(entries)
}

/// Convert std::fs::Metadata to FileAttributes
pub fn get_file_attributes(metadata: &std::fs::Metadata) -> FileAttributes {
    let mut attrs = FileAttributes::default();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        attrs.read_only = mode & 0o222 == 0;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let file_attrs = metadata.file_attributes();
        attrs.read_only = file_attrs & 0x1 != 0;
        attrs.hidden = file_attrs & 0x2 != 0;
        attrs.system = file_attrs & 0x4 != 0;
        attrs.directory = file_attrs & 0x10 != 0;
        attrs.archive = file_attrs & 0x20 != 0;
        attrs.device = file_attrs & 0x40 != 0;
        attrs.normal = file_attrs & 0x80 != 0;
        attrs.temporary = file_attrs & 0x100 != 0;
        attrs.sparse = file_attrs & 0x200 != 0;
        attrs.reparse_point = file_attrs & 0x400 != 0;
        attrs.compressed = file_attrs & 0x800 != 0;
        attrs.offline = file_attrs & 0x1000 != 0;
        attrs.not_content_indexed = file_attrs & 0x2000 != 0;
        attrs.encrypted = file_attrs & 0x4000 != 0;
    }

    attrs
}

/// Format file size for display
pub fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    if size == 0 {
        return "0 B".to_string();
    }
    let size_f = size as f64;
    let unit_idx = (size_f.log10() / 3.0).floor() as usize;
    if unit_idx >= UNITS.len() {
        return format!("{} {}", size, UNITS[0]);
    }
    let value = size_f / (1024u64.pow(unit_idx as u32) as f64);
    if unit_idx == 0 {
        format!("{} {}", value as u64, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", value, UNITS[unit_idx])
    }
}

