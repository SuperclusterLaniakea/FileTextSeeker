use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(700);
const POLL: Duration = Duration::from_millis(200);
const MAX_BATCH: usize = 4000;

/// Watches the folders behind the indexes and reports debounced path batches.
pub struct MonitorHub {
    ctx: eframe::egui::Context,
    updates: Option<Receiver<Vec<PathBuf>>>,
    control: Option<Sender<()>>,
    key: String,
}

impl MonitorHub {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        Self { ctx, updates: None, control: None, key: String::new() }
    }

    pub fn sync(&mut self, paths: &[PathBuf], enabled: bool) {
        let mut watched: Vec<PathBuf> = paths.iter().filter(|path| path.is_dir()).cloned().collect();
        watched.sort_by_key(|path| path.to_string_lossy().to_lowercase());
        watched.dedup_by(|a, b| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()));
        let key = if enabled {
            watched
                .iter()
                .map(|path| path.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join("|")
        } else {
            String::new()
        };
        if key == self.key {
            return;
        }
        self.stop_worker();
        self.key = key;
        if !enabled || watched.is_empty() {
            return;
        }
        let (update_tx, update_rx) = mpsc::channel();
        let (control_tx, control_rx) = mpsc::channel();
        self.updates = Some(update_rx);
        self.control = Some(control_tx);
        let ctx = self.ctx.clone();
        let _ = std::thread::Builder::new()
            .name("fts-monitor".into())
            .spawn(move || worker(watched, update_tx, ctx, control_rx));
    }

    pub fn drain(&mut self) -> Vec<PathBuf> {
        let Some(receiver) = &self.updates else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut paths = Vec::new();
        while let Ok(batch) = receiver.try_recv() {
            for path in batch {
                let key = path.to_string_lossy().to_lowercase();
                if seen.insert(key) {
                    paths.push(path);
                }
                if paths.len() >= MAX_BATCH {
                    return paths;
                }
            }
        }
        paths
    }

    fn stop_worker(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.send(());
        }
        self.updates = None;
    }
}

impl Drop for MonitorHub {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

fn worker(
    roots: Vec<PathBuf>,
    updates: Sender<Vec<PathBuf>>,
    ctx: eframe::egui::Context,
    control: Receiver<()>,
) {
    let (raw_tx, raw_rx) = mpsc::channel::<Vec<PathBuf>>();
    let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else {
            return;
        };
        if matches!(event.kind, EventKind::Access(_)) || event.paths.is_empty() {
            return;
        }
        let _ = raw_tx.send(event.paths);
    });
    let Ok(mut watcher) = watcher else {
        return;
    };
    let mut watching = 0usize;
    for root in &roots {
        if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
            watching += 1;
        }
    }
    if watching == 0 {
        return;
    }

    let mut pending: Vec<PathBuf> = Vec::new();
    let mut pending_keys: HashSet<String> = HashSet::new();
    let mut last_event = Instant::now();
    loop {
        if control.try_recv().is_ok() {
            break;
        }
        match raw_rx.recv_timeout(POLL) {
            Ok(paths) => {
                for path in paths {
                    let key = path.to_string_lossy().to_lowercase();
                    if pending_keys.insert(key) {
                        pending.push(path);
                    }
                }
                last_event = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let due = !pending.is_empty()
            && (last_event.elapsed() >= DEBOUNCE || pending.len() >= 20_000);
        if due {
            let batch = std::mem::take(&mut pending);
            pending_keys.clear();
            if updates.send(batch).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    }
    drop(watcher);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reports_created_file_after_debounce() {
        let root = std::env::temp_dir().join(format!("fts_monitor_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let ctx = eframe::egui::Context::default();
        let mut hub = MonitorHub::new(ctx);
        hub.sync(&[root.clone()], true);
        std::thread::sleep(Duration::from_millis(600));
        let file = root.join("watched.txt");
        fs::write(&file, "x").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while Instant::now() < deadline {
            if hub.drain().iter().any(|path| path == &file) {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        hub.sync(&[], false);
        let _ = fs::remove_dir_all(&root);
        assert!(found, "monitor did not report the created file");
    }
}
