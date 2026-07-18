/// Common types used across the Everything-RS project

use std::path::PathBuf;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a file system entry in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Full path to the file
    pub full_path: PathBuf,
    /// File name (without path)
    pub file_name: String,
    /// File extension
    pub extension: String,
    /// Parent directory path
    pub parent_path: PathBuf,
    /// File size in bytes
    pub size: u64,
    /// Date created
    pub date_created: Option<DateTime<Utc>>,
    /// Date modified
    pub date_modified: Option<DateTime<Utc>>,
    /// Date accessed
    pub date_accessed: Option<DateTime<Utc>>,
    /// Date recently changed
    pub date_recently_changed: Option<DateTime<Utc>>,
    /// File attributes
    pub attributes: FileAttributes,
    /// Is directory
    pub is_directory: bool,
    /// File list filename (if from EFU)
    pub file_list_filename: Option<String>,
    /// Run count
    pub run_count: u64,
    /// Date last run
    pub date_run: Option<DateTime<Utc>>,
}

/// File attributes
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct FileAttributes {
    pub read_only: bool,
    pub hidden: bool,
    pub system: bool,
    pub archive: bool,
    pub device: bool,
    pub normal: bool,
    pub temporary: bool,
    pub sparse: bool,
    pub reparse_point: bool,
    pub compressed: bool,
    pub offline: bool,
    pub not_content_indexed: bool,
    pub encrypted: bool,
    pub directory: bool,
}

/// Sort criteria
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SortField {
    Name,
    Path,
    Size,
    Extension,
    DateCreated,
    DateModified,
    DateAccessed,
    Attributes,
    FileListFileName,
    RunCount,
    DateRecentlyChanged,
    DateRun,
}

/// Sort order
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Sort specification
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SortSpec {
    pub field: SortField,
    pub order: SortOrder,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::Name,
            order: SortOrder::Ascending,
        }
    }
}

/// Search options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Enable regex search
    pub regex: bool,
    /// Match case
    pub match_case: bool,
    /// Match whole word
    pub match_whole_word: bool,
    /// Match full path
    pub match_path: bool,
    /// Match diacritics
    pub match_diacritics: bool,
    /// Maximum results
    pub max_results: usize,
    /// Offset (skip N results)
    pub offset: usize,
    /// Only files (false = both, true = files only)
    pub files_only: bool,
    /// Only folders
    pub folders_only: bool,
    /// Sort specification
    pub sort: SortSpec,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            regex: false,
            match_case: false,
            match_whole_word: false,
            match_path: false,
            match_diacritics: false,
            max_results: 1000,
            offset: 0,
            files_only: false,
            folders_only: false,
            sort: SortSpec::default(),
        }
    }
}

/// Search result item
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entry: FileEntry,
    pub highlighted_name: Option<String>,
    pub highlighted_full_path: Option<String>,
    pub highlighted_path: Option<String>,
}

/// Index type
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum IndexType {
    NTFS,
    Folder,
    FileList,
}

/// Indexed volume or folder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSource {
    pub index_type: IndexType,
    pub path: PathBuf,
    pub enabled: bool,
    pub label: Option<String>,
}

/// Database header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHeader {
    pub magic: String,      // "EZDB" = 0x42445A45
    pub version: String,    // "1.6.6"
    pub flags: u32,
    pub folder_count: u32,
    pub file_count: u32,
    pub folder_decode_size: u32,
    pub file_decode_size: u32,
}

/// Export format
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Csv,
    Efu,
    Txt,
    M3u,
    M3u8,
}

/// Size format for display
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeFormat {
    Auto,
    Bytes,
    KB,
    MB,
}

/// Date format for display
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DateFormat {
    System,
    Iso8601,
    FileTimeDecimal,
    Iso8601Utc,
}

/// Highlight color
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConsoleColor {
    pub value: u8,
}

impl Default for FileEntry {
    fn default() -> Self {
        Self {
            full_path: PathBuf::new(),
            file_name: String::new(),
            extension: String::new(),
            parent_path: PathBuf::new(),
            size: 0,
            date_created: None,
            date_modified: None,
            date_accessed: None,
            date_recently_changed: None,
            attributes: FileAttributes::default(),
            is_directory: false,
            file_list_filename: None,
            run_count: 0,
            date_run: None,
        }
    }
}

