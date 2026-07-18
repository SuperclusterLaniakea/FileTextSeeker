/// Run history and search history tracking

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Track file run history (how many times files have been opened/run)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunHistory {
    entries: Vec<RunHistoryEntry>,
    max_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunHistoryEntry {
    pub path: String,
    pub count: u64,
    pub last_run: Option<String>,
}

impl RunHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    /// Increment run count for a file
    pub fn increment(&mut self, path: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.count += 1;
            entry.last_run = Some(chrono::Utc::now().to_rfc3339());
        } else {
            self.entries.push(RunHistoryEntry {
                path: path.to_string(),
                count: 1,
                last_run: Some(chrono::Utc::now().to_rfc3339()),
            });

            // Trim if over max
            if self.entries.len() > self.max_entries {
                self.entries.remove(0);
            }
        }
    }

    /// Get run count for a file
    pub fn get_count(&self, path: &str) -> u64 {
        self.entries.iter()
            .find(|e| e.path == path)
            .map(|e| e.count)
            .unwrap_or(0)
    }

    /// Set run count for a file
    pub fn set_count(&mut self, path: &str, count: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.count = count;
        } else {
            self.entries.push(RunHistoryEntry {
                path: path.to_string(),
                count,
                last_run: None,
            });
        }
    }

    /// Get entries sorted by run count (most run first)
    pub fn most_run(&self, limit: usize) -> Vec<&RunHistoryEntry> {
        let mut sorted: Vec<&RunHistoryEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.count.cmp(&a.count));
        sorted.into_iter().take(limit).collect()
    }

    /// Get entries sorted by last run time (most recent first)
    pub fn last_run(&self, limit: usize) -> Vec<&RunHistoryEntry> {
        let mut sorted: Vec<&RunHistoryEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.last_run.cmp(&a.last_run));
        sorted.into_iter().take(limit).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Load from file
    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read run history: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse run history: {}", e))
    }

    /// Save to file
    pub fn save(&self, path: &str) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize run history: {}", e))?;
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write run history: {}", e))
    }
}

/// Track search history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHistory {
    entries: VecDeque<String>,
    max_entries: usize,
}

impl SearchHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
        }
    }

    /// Add a search to history
    pub fn add(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }
        // Remove duplicate if exists
        self.entries.retain(|e| e != query);
        // Add to front
        self.entries.push_front(query.to_string());
        // Trim
        while self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }
    }

    /// Get all history entries
    pub fn get_all(&self) -> Vec<&str> {
        self.entries.iter().map(|s| s.as_str()).collect()
    }

    /// Get recent entries
    pub fn get_recent(&self, count: usize) -> Vec<&str> {
        self.entries.iter().take(count).map(|s| s.as_str()).collect()
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Load from file
    pub fn load(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read search history: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse search history: {}", e))
    }

    /// Save to file
    pub fn save(&self, path: &str) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize search history: {}", e))?;
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write search history: {}", e))
    }
}

