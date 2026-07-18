/// SDK module - Everything SDK implementation

use crate::file_seeker::engine::Engine;
use crate::file_seeker::types::{FileEntry, SearchOptions, FileAttributes};
use std::sync::Arc;

/// Everything SDK main interface
pub struct EverythingSdk {
    engine: Arc<Engine>,
}

impl EverythingSdk {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }

    /// Search query
    pub fn query(&self, search_text: &str) -> Result<Vec<FileEntry>, String> {
        let options = SearchOptions::default();
        self.engine.search(search_text, &options)
    }

    /// Get filename of a result
    pub fn get_result_filename<'a>(&self, entry: &'a FileEntry) -> &'a str {
        &entry.file_name
    }

    /// Get path of a result
    pub fn get_result_path<'a>(&self, entry: &'a FileEntry) -> &'a std::path::Path {
        &entry.parent_path
    }

    /// Get full path of a result
    pub fn get_result_full_path_name<'a>(&self, entry: &'a FileEntry) -> &'a std::path::Path {
        &entry.full_path
    }

    /// Get extension of a result
    pub fn get_result_extension<'a>(&self, entry: &'a FileEntry) -> &'a str {
        &entry.extension
    }

    /// Get size of a result
    pub fn get_result_size(&self, entry: &FileEntry) -> u64 {
        entry.size
    }

    /// Get date created
    pub fn get_result_date_created(&self, entry: &FileEntry) -> Option<chrono::DateTime<chrono::Utc>> {
        entry.date_created
    }

    /// Get date modified
    pub fn get_result_date_modified(&self, entry: &FileEntry) -> Option<chrono::DateTime<chrono::Utc>> {
        entry.date_modified
    }

    /// Get date accessed
    pub fn get_result_date_accessed(&self, entry: &FileEntry) -> Option<chrono::DateTime<chrono::Utc>> {
        entry.date_accessed
    }

    /// Get file attributes
    pub fn get_result_attributes<'a>(&self, entry: &'a FileEntry) -> &'a FileAttributes {
        &entry.attributes
    }

    /// Check if result is a file
    pub fn is_file_result(&self, entry: &FileEntry) -> bool {
        !entry.is_directory
    }

    /// Check if result is a folder
    pub fn is_folder_result(&self, entry: &FileEntry) -> bool {
        entry.is_directory
    }

    /// Total number of results
    pub fn get_num_results(&self) -> usize {
        self.engine.total_results()
    }

    /// Number of file results
    pub fn get_num_file_results(&self) -> usize {
        self.engine.num_file_results()
    }

    /// Number of folder results
    pub fn get_num_folder_results(&self) -> usize {
        self.engine.num_folder_results()
    }

    /// Total results in index (no limit)
    pub fn get_tot_results(&self) -> usize {
        self.engine.total_entries()
    }

    /// Total files in index
    pub fn get_tot_file_results(&self) -> usize {
        self.engine.total_file_count()
    }

    /// Total folders in index
    pub fn get_tot_folder_results(&self) -> usize {
        self.engine.total_folder_count()
    }

    /// Get current search text (returns the current_search parameter)
    pub fn get_search<'a>(&self, current_search: &'a str) -> &'a str {
        current_search
    }

    /// Check if database is loaded
    pub fn is_db_loaded(&self) -> bool {
        self.engine.total_entries() > 0
    }

    /// Get target machine name
    pub fn get_target_machine() -> String {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "localhost".to_string())
    }

    /// Get revision
    pub fn get_revision() -> u32 {
        1362
    }

    /// Get major version
    pub fn get_major_version() -> u32 {
        1
    }

    /// Get minor version
    pub fn get_minor_version() -> u32 {
        6
    }

    /// Get build number
    pub fn get_build_number() -> u32 {
        6
    }
}

