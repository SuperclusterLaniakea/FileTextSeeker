/// Core search engine - indexing, searching, sorting

pub mod indexer;
pub mod searcher;
pub mod sorter;
pub mod database;

use std::sync::{Arc, RwLock, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender, Receiver};
use crate::file_seeker::types::{FileEntry, SearchOptions, SortSpec, SortField, SortOrder, IndexSource, IndexType};
use crate::file_seeker::config::Config;

/// Message from background indexer to the UI
#[derive(Debug, Clone)]
pub enum IndexProgress {
    Started(String),
    Progress(String, usize),
    FileCount(usize),
    Complete(usize, usize),  // files, folders
    Error(String),
}

/// The main engine that coordinates indexing and searching
pub struct Engine {
    /// All indexed entries
    pub entries: Arc<RwLock<Vec<FileEntry>>>,
    /// Current search results
    pub results: Arc<RwLock<Vec<FileEntry>>>,
    /// Index sources
    pub index_sources: Arc<RwLock<Vec<IndexSource>>>,
    /// Configuration
    pub config: Arc<RwLock<Config>>,
    /// Is currently indexing
    pub indexing: Arc<AtomicBool>,
}

impl Engine {
    /// Create a new engine instance
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            results: Arc::new(RwLock::new(Vec::new())),
            index_sources: Arc::new(RwLock::new(Vec::new())),
            config: Arc::new(RwLock::new(Config::default())),
            indexing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Build the index synchronously (blocks the calling thread)
    pub fn build_index(&self) -> Result<(), String> {
        let sources = self.index_sources.read().map_err(|e| e.to_string())?;
        let mut entries = self.entries.write().map_err(|e| e.to_string())?;
        entries.clear();

        for source in sources.iter() {
            if !source.enabled {
                continue;
            }
            match source.index_type {
                IndexType::Folder => {
                    let folder_entries = indexer::index_folder(&source.path, false)?;
                    entries.extend(folder_entries);
                }
                IndexType::FileList => {
                    if let Some(path_str) = source.path.to_str() {
                        let list_entries = crate::file_seeker::file_list::FileList::load(path_str)
                            .map(|fl| fl.to_file_entries())
                            .unwrap_or_default();
                        entries.extend(list_entries);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Build the index in a background thread. Returns a receiver for progress updates.
    pub fn build_index_async(&self) -> Receiver<IndexProgress> {
        let (tx, rx) = mpsc::channel();
        self.indexing.store(true, Ordering::SeqCst);

        let sources = self.index_sources.read()
            .map(|s| s.clone())
            .unwrap_or_default();
        let entries = self.entries.clone();
        let indexing = self.indexing.clone();

        thread::spawn(move || {
            let mut all_entries = Vec::new();
            let total_sources = sources.len();
            let mut processed = 0usize;

            for source in sources.iter() {
                if !source.enabled {
                    processed += 1;
                    continue;
                }

                let _ = tx.send(IndexProgress::Progress(
                    format!("正在索引: {}", source.path.display()),
                    processed * 100 / total_sources.max(1),
                ));

                match source.index_type {
                    IndexType::Folder => {
                        match indexer::index_folder(&source.path, false) {
                            Ok(mut folder_entries) => {
                                let count = folder_entries.len();
                                let _ = tx.send(IndexProgress::FileCount(count));
                                all_entries.append(&mut folder_entries);
                            }
                            Err(e) => {
                                let _ = tx.send(IndexProgress::Error(
                                    format!("索引错误 {}: {}", source.path.display(), e)
                                ));
                            }
                        }
                    }
                    IndexType::FileList => {
                        if let Some(path_str) = source.path.to_str() {
                            let list_entries = crate::file_seeker::file_list::FileList::load(path_str)
                                .map(|fl| fl.to_file_entries())
                                .unwrap_or_default();
                            let count = list_entries.len();
                            let _ = tx.send(IndexProgress::FileCount(count));
                            all_entries.extend(list_entries);
                        }
                    }
                    _ => {}
                }
                processed += 1;
            }

            // Write all entries at once
            if let Ok(mut e) = entries.write() {
                *e = all_entries;
            }

            let files = entries.read()
                .map(|e| e.iter().filter(|x| !x.is_directory).count())
                .unwrap_or(0);
            let folders = entries.read()
                .map(|e| e.iter().filter(|x| x.is_directory).count())
                .unwrap_or(0);

            let _ = tx.send(IndexProgress::Complete(files, folders));
            indexing.store(false, Ordering::SeqCst);
        });

        rx
    }

    /// Perform a search with the given query and options
    pub fn search(&self, query: &str, options: &SearchOptions) -> Result<Vec<FileEntry>, String> {
        let entries = self.entries.read().map_err(|e| e.to_string())?;
        let results = searcher::SearchEngine::search(&entries, query, options);

        let mut results = results;
        sorter::sort_entries(&mut results, &options.sort);

        if options.offset > 0 && options.offset < results.len() {
            results = results[options.offset..].to_vec();
        }
        if options.max_results > 0 && options.max_results < results.len() {
            results = results[..options.max_results].to_vec();
        }

        Ok(results)
    }

    /// Quick search - only searches first N entries for responsiveness
    pub fn quick_search(&self, query: &str, options: &SearchOptions, max_scan: usize) -> Result<Vec<FileEntry>, String> {
        let entries = self.entries.read().map_err(|e| e.to_string())?;

        // Only scan up to max_scan entries for quick response
        let scan_entries: Vec<FileEntry> = entries.iter()
            .take(max_scan)
            .cloned()
            .collect();

        let results = searcher::SearchEngine::search(&scan_entries, query, options);
        let mut results = results;
        sorter::sort_entries(&mut results, &options.sort);

        if options.max_results > 0 && options.max_results < results.len() {
            results = results[..options.max_results].to_vec();
        }

        Ok(results)
    }

    pub fn total_entries(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    pub fn total_results(&self) -> usize {
        self.results.read().map(|r| r.len()).unwrap_or(0)
    }

    pub fn num_file_results(&self) -> usize {
        self.results.read().map(|r| r.iter().filter(|e| !e.is_directory).count()).unwrap_or(0)
    }

    pub fn num_folder_results(&self) -> usize {
        self.results.read().map(|r| r.iter().filter(|e| e.is_directory).count()).unwrap_or(0)
    }

    pub fn total_file_count(&self) -> usize {
        self.entries.read().map(|e| e.iter().filter(|e| !e.is_directory).count()).unwrap_or(0)
    }

    pub fn total_folder_count(&self) -> usize {
        self.entries.read().map(|e| e.iter().filter(|e| e.is_directory).count()).unwrap_or(0)
    }

    pub fn increment_run_count(&self, path: &str) -> Result<(), String> {
        let mut entries = self.entries.write().map_err(|e| e.to_string())?;
        if let Some(entry) = entries.iter_mut().find(|e| {
            e.full_path.to_string_lossy().as_ref() == path
        }) {
            entry.run_count += 1;
            entry.date_run = Some(chrono::Utc::now());
        }
        Ok(())
    }

    pub fn get_run_count(&self, path: &str) -> Option<u64> {
        self.entries.read().ok().and_then(|entries| {
            entries.iter().find(|e| e.full_path.to_string_lossy().as_ref() == path)
                .map(|e| e.run_count)
        })
    }

    pub fn set_run_count(&self, path: &str, count: u64) -> Result<(), String> {
        let mut entries = self.entries.write().map_err(|e| e.to_string())?;
        if let Some(entry) = entries.iter_mut().find(|e| {
            e.full_path.to_string_lossy().as_ref() == path
        }) {
            entry.run_count = count;
        }
        Ok(())
    }
}

