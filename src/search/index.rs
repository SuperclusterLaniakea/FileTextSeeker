use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::query::{QueryProgram, SortField};
use crate::util;

const SYSTEM_DIRS: [&str; 10] = [
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "$recycle.bin",
    "system volume information",
    "recovery",
    "perflogs",
    "windows.old",
    "config.msi",
];

const INSERT_SQL: &str = "INSERT INTO files(id,path,name,lower_name,ext,size,modified,created,accessed,is_dir,depth,attributes)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
ON CONFLICT(id) DO UPDATE SET path=excluded.path,name=excluded.name,lower_name=excluded.lower_name,
ext=excluded.ext,size=excluded.size,modified=excluded.modified,created=excluded.created,
accessed=excluded.accessed,is_dir=excluded.is_dir,depth=excluded.depth,attributes=excluded.attributes";

#[derive(Clone, Debug)]
pub struct FileRecord {
    pub path: PathBuf,
    pub name: String,
    pub ext: String,
    pub lower_name: String,
    pub lower_path: String,
    pub size: u64,
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
    pub is_dir: bool,
    pub depth: u16,
    pub attributes: u32,
}

impl FileRecord {
    pub fn from_path(path: &Path, depth: u16) -> Option<Self> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        let is_dir = meta.is_dir();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let ext = if is_dir {
            String::new()
        } else {
            path.extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        let lower_name = name.to_lowercase();
        let lower_path = path.to_string_lossy().to_lowercase();
        let attributes = attributes_of(&meta);
        Some(Self {
            path: path.to_path_buf(),
            name,
            ext,
            lower_name,
            lower_path,
            size: if is_dir { 0 } else { meta.len() },
            modified: meta.modified().map(util::system_time_secs).unwrap_or(0),
            created: meta.created().map(util::system_time_secs).unwrap_or(0),
            accessed: meta.accessed().map(util::system_time_secs).unwrap_or(0),
            is_dir,
            depth,
            attributes,
        })
    }
}

#[cfg(windows)]
fn attributes_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes()
}
#[cfg(not(windows))]
fn attributes_of(_meta: &std::fs::Metadata) -> u32 {
    0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexOptions {
    pub excluded_dirs: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub include_hidden: bool,
    pub include_system_dirs: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            excluded_dirs: vec!["$recycle.bin".into(), "system volume information".into()],
            exclude_patterns: Vec::new(),
            include_hidden: false,
            include_system_dirs: false,
        }
    }
}

impl IndexOptions {
    fn excluded_component(&self, name_lower: &str) -> bool {
        self.excluded_dirs
            .iter()
            .any(|x| x.eq_ignore_ascii_case(name_lower))
    }

    fn matches_pattern(&self, name_lower: &str) -> bool {
        for pattern in &self.exclude_patterns {
            let pattern = pattern.trim().to_lowercase();
            if pattern.is_empty() {
                continue;
            }
            if let Ok(glob) = glob::Pattern::new(&pattern) {
                if glob.matches(name_lower) {
                    return true;
                }
            }
        }
        false
    }

    pub fn accepts_path(&self, path: &Path, root: &Path) -> bool {
        let relative = path.strip_prefix(root).unwrap_or(path);
        for component in relative.components() {
            if let Some(name) = component.as_os_str().to_str() {
                let lower = name.to_lowercase();
                if self.excluded_component(&lower) {
                    return false;
                }
            }
        }
        if !self.include_hidden && util::is_hidden_path(path) {
            return false;
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            if self.matches_pattern(&name.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug, Default)]
pub struct IndexSummary {
    pub roots: Vec<PathBuf>,
    pub files: u64,
    pub folders: u64,
    pub errors: u64,
    pub built_at: i64,
    pub options: Option<IndexOptions>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ChangeStats {
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub errors: u64,
    pub files_delta: i64,
    pub folders_delta: i64,
}

#[derive(Clone, Debug)]
pub struct SearchDb {
    path: PathBuf,
}

impl SearchDb {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    pub fn summary(&self) -> Result<IndexSummary, String> {
        if !self.path.is_file() {
            return Err("索引数据库不存在".into());
        }
        let conn = connect(&self.path)?;
        let files = conn
            .query_row("SELECT COUNT(*) FROM files WHERE is_dir=0", [], |row| row.get::<_, i64>(0))
            .map_err(|e| format!("读取索引失败: {e}"))?;
        let folders = conn
            .query_row("SELECT COUNT(*) FROM files WHERE is_dir=1", [], |row| row.get::<_, i64>(0))
            .map_err(|e| format!("读取索引失败: {e}"))?;
        let roots = meta_value(&conn, "roots")
            .and_then(|text| serde_json::from_str::<Vec<PathBuf>>(&text).ok())
            .unwrap_or_default();
        let errors = meta_value(&conn, "errors")
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let built_at = meta_value(&conn, "built_at")
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let options = meta_value(&conn, "options")
            .and_then(|text| serde_json::from_str::<IndexOptions>(&text).ok());
        Ok(IndexSummary {
            roots,
            files: files.max(0) as u64,
            folders: folders.max(0) as u64,
            errors,
            built_at,
            options,
        })
    }

    pub fn query(
        &self,
        program: &QueryProgram,
        sort: SortField,
        ascending: bool,
        limit: usize,
    ) -> Result<Vec<FileRecord>, String> {
        let conn = connect(&self.path)?;
        let sql = format!("SELECT {SELECT_COLUMNS} FROM files ORDER BY {}", order_clause(sort, ascending));
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let record = record_from_row(row)?;
            if program.matches(&record) {
                out.push(record);
                if limit > 0 && out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    pub fn exact(
        &self,
        lower_name: &str,
        sort: SortField,
        ascending: bool,
        limit: usize,
    ) -> Result<Vec<FileRecord>, String> {
        let conn = connect(&self.path)?;
        let sql = format!(
            "SELECT {SELECT_COLUMNS} FROM files WHERE lower_name=?1 ORDER BY {} LIMIT ?2",
            order_clause(sort, ascending)
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params![lower_name, limit.max(1) as i64])
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            out.push(record_from_row(row)?);
        }
        Ok(out)
    }

    pub fn apply_changes(
        &self,
        paths: &[PathBuf],
        roots: &[PathBuf],
        options: &IndexOptions,
    ) -> Result<ChangeStats, String> {
        if paths.is_empty() || !self.path.is_file() {
            return Ok(ChangeStats::default());
        }
        let conn = connect(&self.path)?;
        let mut files = meta_value(&conn, "files").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let mut folders = meta_value(&conn, "folders").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let mut errors = meta_value(&conn, "errors").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let files_before = files;
        let folders_before = folders;
        conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

        let result = (|| -> Result<ChangeStats, String> {
            let mut stats = ChangeStats::default();
            let mut upsert = conn.prepare_cached(INSERT_SQL).map_err(|e| e.to_string())?;
            let mut lookup = conn
                .prepare_cached("SELECT is_dir FROM files WHERE id=?1")
                .map_err(|e| e.to_string())?;
            let mut delete = conn
                .prepare_cached("DELETE FROM files WHERE id=?1")
                .map_err(|e| e.to_string())?;
            let mut delete_prefix = conn
                .prepare_cached("DELETE FROM files WHERE id=?1 OR (id > ?1 AND id < ?2)")
                .map_err(|e| e.to_string())?;
            let mut count_prefix = conn
                .prepare_cached(
                    "SELECT COALESCE(SUM(CASE WHEN is_dir=0 THEN 1 ELSE 0 END),0),
                            COALESCE(SUM(CASE WHEN is_dir=1 THEN 1 ELSE 0 END),0)
                     FROM files WHERE id=?1 OR (id > ?1 AND id < ?2)",
                )
                .map_err(|e| e.to_string())?;

            for path in paths {
                if is_db_artifact(path, &self.path) {
                    continue;
                }
                let id = normalize_id(path);
                match resolve_root(roots, path) {
                    Some(root) if options.accepts_path(path, root) => {}
                    _ => continue,
                }
                let meta = std::fs::symlink_metadata(path).ok();
                let existing = lookup
                    .query_row(params![id], |row| row.get::<_, i64>(0))
                    .optional()
                    .map_err(|e| e.to_string())?;
                match meta {
                    Some(meta) if meta.is_dir() => {
                        if existing == Some(1) {
                            continue;
                        }
                        if existing == Some(0) {
                            files -= 1;
                        }
                        let root = resolve_root(roots, path).expect("checked above");
                        let base_depth = depth_of(root, path);
                        scan_subtree(
                            path,
                            root,
                            base_depth,
                            options,
                            &mut upsert,
                            &mut lookup,
                            &self.path,
                            &mut files,
                            &mut folders,
                            &mut stats,
                        )?;
                    }
                    Some(_) => {
                        let root = resolve_root(roots, path).expect("checked above");
                        let depth = depth_of(root, path);
                        if let Some(record) = FileRecord::from_path(path, depth) {
                            let old_kind = existing;
                            match old_kind {
                                None => {
                                    if record.is_dir {
                                        folders += 1;
                                    } else {
                                        files += 1;
                                    }
                                    stats.added += 1;
                                }
                                Some(old) => {
                                    if old == 0 && record.is_dir {
                                        files -= 1;
                                        folders += 1;
                                    } else if old == 1 && !record.is_dir {
                                        folders -= 1;
                                        files += 1;
                                    }
                                    stats.updated += 1;
                                }
                            }
                            stmt_insert(&mut upsert, &record)?;
                        } else {
                            stats.errors += 1;
                            errors += 1;
                        }
                    }
                    None => {
                        let Some(kind) = existing else { continue };
                        if kind == 1 {
                            let upper = prefix_upper(&id);
                            let (lost_files, lost_folders) = count_prefix
                                .query_row(params![id, upper], |row| {
                                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                                })
                                .map_err(|e| e.to_string())?;
                            let removed = lost_files + lost_folders;
                            if removed == 0 {
                                continue;
                            }
                            delete_prefix
                                .execute(params![id, upper])
                                .map_err(|e| e.to_string())?;
                            files -= lost_files;
                            folders -= lost_folders;
                            stats.removed += removed.max(0) as u64;
                        } else {
                            let removed = delete.execute(params![id]).map_err(|e| e.to_string())?;
                            if removed > 0 {
                                files -= 1;
                                stats.removed += 1;
                            }
                        }
                    }
                }
            }
            stats.files_delta = files - files_before;
            stats.folders_delta = folders - folders_before;
            write_meta(&conn, files.max(0), folders.max(0), errors.max(0))?;
            Ok(stats)
        })();

        match result {
            Ok(stats) => {
                conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
                conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)").ok();
                Ok(stats)
            }
            Err(error) => {
                conn.execute_batch("ROLLBACK").ok();
                Err(error)
            }
        }
    }
}

const SELECT_COLUMNS: &str =
    "path,name,lower_name,ext,size,modified,created,accessed,is_dir,depth,attributes,id";

fn record_from_row(row: &rusqlite::Row<'_>) -> Result<FileRecord, String> {
    let path: String = row.get(0).map_err(|e| e.to_string())?;
    let lower_path: String = row.get(11).map_err(|e| e.to_string())?;
    Ok(FileRecord {
        path: PathBuf::from(path),
        name: row.get(1).map_err(|e| e.to_string())?,
        lower_name: row.get(2).map_err(|e| e.to_string())?,
        ext: row.get(3).map_err(|e| e.to_string())?,
        size: row.get::<_, i64>(4).map_err(|e| e.to_string())?.max(0) as u64,
        modified: row.get(5).map_err(|e| e.to_string())?,
        created: row.get(6).map_err(|e| e.to_string())?,
        accessed: row.get(7).map_err(|e| e.to_string())?,
        is_dir: row.get::<_, i64>(8).map_err(|e| e.to_string())? != 0,
        depth: row.get::<_, i64>(9).map_err(|e| e.to_string())?.clamp(0, u16::MAX as i64) as u16,
        attributes: row.get::<_, i64>(10).map_err(|e| e.to_string())?.max(0) as u32,
        lower_path,
    })
}

fn connect(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("打开索引数据库失败: {e}"))?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-32768;
         PRAGMA temp_store=MEMORY;
         PRAGMA foreign_keys=OFF;",
    )
    .map_err(|e| format!("初始化索引数据库失败: {e}"))?;
    ensure_schema(&conn)?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files(
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            lower_name TEXT NOT NULL,
            ext TEXT NOT NULL,
            size INTEGER NOT NULL,
            modified INTEGER NOT NULL,
            created INTEGER NOT NULL,
            accessed INTEGER NOT NULL,
            is_dir INTEGER NOT NULL,
            depth INTEGER NOT NULL,
            attributes INTEGER NOT NULL
        ) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS idx_files_name ON files(lower_name, id);
        CREATE INDEX IF NOT EXISTS idx_files_size ON files(size, id);
        CREATE INDEX IF NOT EXISTS idx_files_modified ON files(modified, id);
        CREATE INDEX IF NOT EXISTS idx_files_ext ON files(ext, lower_name, id);
        CREATE INDEX IF NOT EXISTS idx_files_type ON files(is_dir, ext, id);
        CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|e| format!("创建索引数据库结构失败: {e}"))
}

fn meta_value(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key=?1", params![key], |row| row.get(0))
        .optional()
        .ok()
        .flatten()
}

fn write_meta(conn: &Connection, files: i64, folders: i64, errors: i64) -> Result<(), String> {
    let pairs = [
        ("files", files.to_string()),
        ("folders", folders.to_string()),
        ("errors", errors.to_string()),
    ];
    let mut stmt = conn
        .prepare("INSERT OR REPLACE INTO meta(key,value) VALUES(?1,?2)")
        .map_err(|e| e.to_string())?;
    for (key, value) in pairs {
        stmt.execute(params![key, value]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn order_clause(sort: SortField, ascending: bool) -> String {
    let dir = if ascending { "ASC" } else { "DESC" };
    match sort {
        SortField::Name => format!("lower_name {dir}, id {dir}"),
        SortField::Path => format!("id {dir}"),
        SortField::Size => format!("size {dir}, id {dir}"),
        SortField::Type => format!("is_dir {dir}, ext {dir}, id {dir}"),
        SortField::Modified => format!("modified {dir}, id {dir}"),
        SortField::Extension => format!("ext {dir}, lower_name {dir}, id {dir}"),
    }
}

fn stmt_insert(stmt: &mut rusqlite::CachedStatement<'_>, record: &FileRecord) -> Result<(), String> {
    stmt.execute(params![
        record.lower_path,
        record.path.to_string_lossy().to_string(),
        record.name,
        record.lower_name,
        record.ext,
        record.size as i64,
        record.modified,
        record.created,
        record.accessed,
        i64::from(record.is_dir),
        record.depth as i64,
        record.attributes as i64,
    ])
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn normalize_id(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// The index database and its sidecar files must never be indexed, even when
/// the configured database path sits inside a watched root.
fn is_db_artifact(path: &Path, db: &Path) -> bool {
    let (Some(parent), Some(db_parent)) = (path.parent(), db.parent()) else {
        return false;
    };
    if !parent.to_string_lossy().eq_ignore_ascii_case(&db_parent.to_string_lossy()) {
        return false;
    }
    let (Some(name), Some(db_name)) = (path.file_name(), db.file_name()) else {
        return false;
    };
    let name = name.to_string_lossy().to_ascii_lowercase();
    let db_name = db_name.to_string_lossy().to_ascii_lowercase();
    name == db_name
        || name == format!("{db_name}.tmp")
        || name == format!("{db_name}.bak")
        || name == format!("{db_name}-wal")
        || name == format!("{db_name}-shm")
}

fn prefix_upper(prefix: &str) -> String {
    let mut upper = prefix.to_string();
    upper.push('\u{10ffff}');
    upper
}

fn resolve_root<'a>(roots: &'a [PathBuf], path: &Path) -> Option<&'a PathBuf> {
    roots.iter().find(|root| path.starts_with(root))
}

fn depth_of(root: &Path, path: &Path) -> u16 {
    path.strip_prefix(root)
        .map(|rel| rel.components().count() as u16)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn scan_subtree(
    start: &Path,
    root: &Path,
    base_depth: u16,
    options: &IndexOptions,
    upsert: &mut rusqlite::CachedStatement<'_>,
    lookup: &mut rusqlite::CachedStatement<'_>,
    db: &Path,
    files: &mut i64,
    folders: &mut i64,
    stats: &mut ChangeStats,
) -> Result<(), String> {
    let walker = WalkDir::new(start)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || (!is_db_artifact(entry.path(), db) && entry_allowed(entry, root, options))
        });
    for entry in walker {
        let Ok(entry) = entry else {
            stats.errors += 1;
            continue;
        };
        let depth = base_depth.saturating_add(entry.depth() as u16);
        if depth == 0 || entry.file_type().is_symlink() {
            continue;
        }
        let path = entry.path();
        let Some(record) = FileRecord::from_path(path, depth) else {
            stats.errors += 1;
            continue;
        };
        let id = normalize_id(path);
        let existing = lookup
            .query_row(params![id], |row| row.get::<_, i64>(0))
            .optional()
            .map_err(|e| e.to_string())?;
        match existing {
            None => {
                if record.is_dir {
                    *folders += 1;
                } else {
                    *files += 1;
                }
                stats.added += 1;
            }
            Some(old) => {
                if old == 0 && record.is_dir {
                    *files -= 1;
                    *folders += 1;
                } else if old == 1 && !record.is_dir {
                    *folders -= 1;
                    *files += 1;
                }
                stats.updated += 1;
            }
        }
        stmt_insert(upsert, &record)?;
    }
    Ok(())
}

fn entry_allowed(entry: &walkdir::DirEntry, root: &Path, options: &IndexOptions) -> bool {
    let name = entry.file_name().to_string_lossy().to_lowercase();
    if options.excluded_component(&name) {
        return false;
    }
    if entry.file_type().is_dir() {
        if !options.include_system_dirs
            && entry.depth() == 1
            && root.parent().is_none()
            && SYSTEM_DIRS.contains(&name.as_str())
        {
            return false;
        }
        if !options.include_hidden && util::is_hidden_path(entry.path()) {
            return false;
        }
        if options.matches_pattern(&name) {
            return false;
        }
    } else if options.matches_pattern(&name) {
        return false;
    }
    true
}

pub fn build_database(
    path: &Path,
    roots: &[PathBuf],
    options: &IndexOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, &Path),
) -> Result<IndexSummary, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建索引目录失败: {e}"))?;
    }
    let tmp = path.with_extension("db.tmp");
    let _ = std::fs::remove_file(&tmp);
    let conn = Connection::open(&tmp).map_err(|e| format!("创建索引数据库失败: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode=OFF;
         PRAGMA synchronous=OFF;
         PRAGMA cache_size=-32768;
         PRAGMA temp_store=MEMORY;",
    )
    .map_err(|e| format!("初始化索引数据库失败: {e}"))?;
    ensure_schema(&conn)?;
    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

    let mut files: u64 = 0;
    let mut folders: u64 = 0;
    let mut errors: u64 = 0;
    let mut records: usize = 0;
    let mut cancelled = false;
    {
        let mut insert = conn.prepare_cached(INSERT_SQL).map_err(|e| e.to_string())?;
        'outer: for root in roots {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            if !root.exists() {
                errors += 1;
                continue;
            }
            let walker = WalkDir::new(root)
                .follow_links(false)
                .sort_by_file_name()
                .into_iter()
                .filter_entry(|entry| {
                    entry.depth() == 0
                        || (!is_db_artifact(entry.path(), path)
                            && entry_allowed(entry, root, options))
                });
            for entry in walker {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break 'outer;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => {
                        errors += 1;
                        continue;
                    }
                };
                if entry.depth() == 0 {
                    continue;
                }
                if !options.include_hidden && util::is_hidden_path(entry.path()) {
                    continue;
                }
                if entry.file_type().is_symlink() {
                    continue;
                }
                let Some(record) = FileRecord::from_path(entry.path(), entry.depth() as u16) else {
                    errors += 1;
                    continue;
                };
                stmt_insert(&mut insert, &record)?;
                if record.is_dir {
                    folders += 1;
                } else {
                    files += 1;
                }
                records += 1;
                if records % 512 == 0 {
                    progress(records, entry.path());
                }
                if records % 4096 == 0 {
                    conn.execute_batch("COMMIT; BEGIN").map_err(|e| e.to_string())?;
                }
            }
        }
    }
    if cancelled {
        conn.execute_batch("COMMIT").ok();
        drop(conn);
        let _ = std::fs::remove_file(&tmp);
        return Err("已取消索引".into());
    }
    let roots_json = serde_json::to_string(roots).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES(?1,?2)",
        params!["roots", roots_json],
    )
    .map_err(|e| e.to_string())?;
    let options_json = serde_json::to_string(options).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES(?1,?2)",
        params!["options", options_json],
    )
    .map_err(|e| e.to_string())?;
    let built_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    for (key, value) in [
        ("files", files.to_string()),
        ("folders", folders.to_string()),
        ("errors", errors.to_string()),
        ("built_at", built_at.to_string()),
    ] {
        conn.execute(
            "INSERT OR REPLACE INTO meta(key,value) VALUES(?1,?2)",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    drop(conn);
    replace_database(&tmp, path)?;
    progress(records, Path::new(""));
    Ok(IndexSummary {
        roots: roots.to_vec(),
        files,
        folders,
        errors,
        built_at,
        options: Some(options.clone()),
    })
}

fn replace_database(tmp: &Path, dest: &Path) -> Result<(), String> {
    let backup = dest.with_extension("db.bak");
    let _ = std::fs::remove_file(&backup);
    let wal = PathBuf::from(format!("{}-wal", dest.display()));
    let shm = PathBuf::from(format!("{}-shm", dest.display()));
    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(&shm);
    if dest.exists() {
        std::fs::rename(dest, &backup).map_err(|e| format!("替换索引数据库失败: {e}"))?;
    }
    if let Err(error) = std::fs::rename(tmp, dest) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, dest);
        }
        return Err(format!("替换索引数据库失败: {error}"));
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::query::QueryOptions;
    use std::fs;

    #[test]
    fn builds_queries_and_updates_incrementally() {
        let root = std::env::temp_dir().join(format!("fts_db_test_{}", std::process::id()));
        let db_path = root.join("search.db");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("alpha.txt"), "a").unwrap();

        let options = IndexOptions {
            excluded_dirs: vec!["node_modules".into()],
            exclude_patterns: vec!["*.tmp".into()],
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let summary = build_database(&db_path, &[root.clone()], &options, &cancel, |_, _| {}).unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.folders, 1);

        let db = SearchDb::new(&db_path);
        let query = QueryProgram::parse("alpha", &QueryOptions::default()).unwrap();
        let hits = db.query(&query, SortField::Name, true, 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "alpha.txt");

        let nested = root.join("newdir");
        fs::create_dir_all(nested.join("deep")).unwrap();
        fs::write(nested.join("deep/note.md"), "x").unwrap();
        let stats = db.apply_changes(&[nested.clone()], &[root.clone()], &options).unwrap();
        assert_eq!(stats.added, 3);
        let hits = db.query(&query, SortField::Name, true, 50).unwrap();
        assert_eq!(hits.len(), 1);
        let note = QueryProgram::parse("note", &QueryOptions::default()).unwrap();
        assert_eq!(db.query(&note, SortField::Name, true, 50).unwrap().len(), 1);

        fs::remove_dir_all(&nested).unwrap();
        let stats = db.apply_changes(&[nested], &[root.clone()], &options).unwrap();
        assert_eq!(stats.removed, 3);
        assert!(db.query(&note, SortField::Name, true, 50).unwrap().is_empty());
        let summary = db.summary().unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.folders, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sorting_and_exact_match_follow_requested_order() {
        let root = std::env::temp_dir().join(format!("fts_db_order_{}", std::process::id()));
        let db_path = root.join("search.db");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("b.txt"), "22").unwrap();
        fs::write(root.join("a.txt"), "1").unwrap();
        fs::write(root.join("c.txt"), "333").unwrap();
        let options = IndexOptions::default();
        let cancel = AtomicBool::new(false);
        build_database(&db_path, &[root.clone()], &options, &cancel, |_, _| {}).unwrap();
        let db = SearchDb::new(db_path);
        let query = QueryProgram::parse("", &QueryOptions::default()).unwrap();
        let hits = db.query(&query, SortField::Size, true, 2).unwrap();
        assert_eq!(hits.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["a.txt", "b.txt"]);
        let hits = db.query(&query, SortField::Name, false, 3).unwrap();
        assert_eq!(hits.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["c.txt", "b.txt", "a.txt"]);
        let exact = db.exact("a.txt", SortField::Name, true, 10).unwrap();
        assert_eq!(exact.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
