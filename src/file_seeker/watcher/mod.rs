/// File system watcher - real-time monitoring of indexed folders
///
/// Uses `notify` crate to watch for file changes and keep the index up-to-date.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use notify::{Config, Event, EventKind, RecursiveMode, Watcher};

/// Events from the file watcher
#[derive(Debug, Clone)]
pub enum WatcherEvent {
    FileAdded(PathBuf),
    FileRemoved(PathBuf),
    FileModified(PathBuf),
    FileRenamed(PathBuf, PathBuf),
    WatcherError(String),
}

/// File watcher manager
pub struct FileWatcher {
    running: Arc<AtomicBool>,
    watcher: Option<Box<dyn Watcher>>,
    event_tx: Sender<WatcherEvent>,
    event_rx: Receiver<WatcherEvent>,
    watched_paths: Vec<PathBuf>,
}

impl FileWatcher {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            running: Arc::new(AtomicBool::new(false)),
            watcher: None,
            event_tx: tx,
            event_rx: rx,
            watched_paths: Vec::new(),
        }
    }

    /// Start watching the specified paths
    pub fn start_watching(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        self.stop_watching();
        self.watched_paths = paths.clone();
        self.running.store(true, Ordering::SeqCst);

        let event_tx = self.event_tx.clone();
        let running = self.running.clone();

        // Build the watcher
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if !running.load(Ordering::SeqCst) {
                return;
            }
            match res {
                Ok(event) => {
                    let tx = event_tx.clone();
                    match event.kind {
                        EventKind::Create(_) => {
                            for path in event.paths {
                                let _ = tx.send(WatcherEvent::FileAdded(path));
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                let _ = tx.send(WatcherEvent::FileRemoved(path));
                            }
                        }
                        EventKind::Modify(_) => {
                            for path in event.paths {
                                let _ = tx.send(WatcherEvent::FileModified(path));
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    let _ = event_tx.send(WatcherEvent::WatcherError(e.to_string()));
                }
            }
        }).map_err(|e| format!("无法创建文件监控器 {}", e))?;

        // Add all paths to watch
        for path in &paths {
            if path.exists() {
                watcher.watch(path, RecursiveMode::Recursive)
                    .map_err(|e| format!("鏃犳硶鐩戞帶 {}: {}", path.display(), e))?;
            }
        }

        self.watcher = Some(Box::new(watcher));
        Ok(())
    }

    /// Stop watching
    pub fn stop_watching(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.watcher = None;
    }

    /// Poll for a watcher event (non-blocking)
    pub fn poll_event(&self) -> Option<WatcherEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Check if watcher is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get currently watched paths
    pub fn watched_paths(&self) -> &[PathBuf] {
        &self.watched_paths
    }
}

