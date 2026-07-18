use calamine::Reader;
use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use jieba_rs::Jieba;
use std::collections::HashMap;
use std::io::Read;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use walkdir::WalkDir;

// ==================== 共享数据结构 ====================
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DocMeta {
    pub path: PathBuf,
    pub filename: String,
    pub mtime: u64,
    pub md5: String,
    #[serde(default)]
    pub file_size: u64,
}

#[derive(Clone, Debug)]
pub struct KeywordHit {
    pub location: u32,
    pub snippet: String,
    pub keyword: String,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub file_id: u64,
    pub filename: String,
    pub path: PathBuf,
    pub location: u64,
    pub snippet: String,
    pub score: f32,
    pub mtime: u64,
    pub keyword_count: usize,
    pub hits: Vec<KeywordHit>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortBy {
    Relevance,
    FileNameAsc,
    FileNameDesc,
    DateNewest,
    DateOldest,
    Frequency,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum ResultViewMode {
    #[default]
    Compact,
    Detailed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DatabaseInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub root_dir: PathBuf,
    pub created_at: String,
    pub last_index_time: Option<String>,
    pub index_subdir: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub base_dir: String,
    pub pdf_reader: Option<String>,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub search_history: Vec<String>,
    #[serde(default)]
    pub result_view: ResultViewMode,
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    #[serde(default)]
    pub databases: Vec<DatabaseInfo>,
    #[serde(default)]
    pub active_db_id: Option<String>,
}

fn default_font_scale() -> f32 { 1.0 }

impl Default for Config {
    fn default() -> Self {
        let base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".doc_searcher");
        Self {
            base_dir: base.to_string_lossy().to_string(),
            pdf_reader: None,
            ignore_case: true,
            search_history: Vec::new(),
            result_view: ResultViewMode::Compact,
            font_scale: 1.0,
            databases: Vec::new(),
            active_db_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IndexState {
    Idle,
    Indexing,
    Paused,
}

pub enum IndexMsg {
    Progress {
        current: String,
        total: usize,
        index: usize,
    },
    Done,
}

// ==================== 辅助函数 ====================

pub fn safe_slice(text: &str, start: usize, len: usize) -> &str {
    let mut real_start = start;
    while real_start > 0 && !text.is_char_boundary(real_start) {
        real_start -= 1;
    }
    let end = (real_start + len).min(text.len());
    let mut real_end = end;
    while real_end < text.len() && !text.is_char_boundary(real_end) {
        real_end += 1;
    }
    &text[real_start..real_end]
}

pub fn tokenize_with_case(text: &str, lowercase: bool) -> String {
    let jieba = Jieba::new();
    let words = jieba.cut(text, true).join(" ");
    if lowercase { words.to_lowercase() } else { words }
}

pub fn tokenize(text: &str, ignore_case: bool) -> String {
    tokenize_with_case(text, ignore_case)
}

pub fn tokenize_query(query: &str, ignore_case: bool) -> String {
    let has_boolean = regex::Regex::new(r"(?i)\b(AND|OR|NOT)\b")
        .unwrap()
        .is_match(query);
    if has_boolean {
        if ignore_case { query.to_lowercase() } else { query.to_string() }
    } else {
        tokenize_with_case(query, ignore_case)
    }
}

pub fn compute_md5(path: &Path) -> Result<String> {
    let mut hasher = md5::Context::new();
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0u8; 8192];
    let n = file.read(&mut buffer)?;
    hasher.consume(&buffer[..n]);
    hasher.consume(&format!("{}", file.metadata()?.len()));
    Ok(format!("{:x}", hasher.compute()))
}

pub fn extract_pages_safe(path: &Path, ext: &str) -> Result<Vec<(u32, String)>> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extract_pages(path, ext)));
    match result {
        Ok(res) => res,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            Err(anyhow::anyhow!("extract_pages panicked: {}", msg))
        }
    }
}

pub fn extract_pages(path: &Path, ext: &str) -> Result<Vec<(u32, String)>> {
    match ext {
        "pdf" => {
            let bytes = std::fs::read(path)?;
            let text = pdf_extract::extract_text_from_mem(&bytes)
                .map_err(|e| anyhow::anyhow!("PDF extract error: {}", e))?;
            if text.trim().is_empty() {
                return Ok(vec![]);
            }
            let pages: Vec<(u32, String)> = text
                .split('\x0c')
                .enumerate()
                .filter(|(_, t)| !t.trim().is_empty())
                .map(|(i, t)| (i as u32 + 1, t.trim().to_string()))
                .collect();
            if pages.is_empty() {
                Ok(vec![(1, text)])
            } else {
                Ok(pages)
            }
        }
        "docx" => {
            let data = std::fs::read(path)?;
            let docx = docx_rs::read_docx(&data)
                .map_err(|e| anyhow::anyhow!("DOCX error: {}", e))?;
            let mut paras = Vec::new();
            for child in &docx.document.children {
                if let docx_rs::DocumentChild::Paragraph(p) = child {
                    let mut para_text = String::new();
                    for pchild in &p.children {
                        if let docx_rs::ParagraphChild::Run(run) = pchild {
                            for rchild in &run.children {
                                if let docx_rs::RunChild::Text(t) = rchild {
                                    para_text.push_str(&t.text);
                                }
                            }
                        }
                    }
                    if !para_text.trim().is_empty() { paras.push(para_text); }
                }
            }
            Ok(paras.into_iter().enumerate().map(|(i, t)| (i as u32 + 1, t)).collect())
        }
        "txt" => {
            let text = std::fs::read_to_string(path)?;
            Ok(vec![(1, text)])
        }
        "xlsx" | "xls" => {
            let mut workbook = calamine::open_workbook_auto(path)
                .map_err(|e| anyhow::anyhow!("Excel open error: {}", e))?;
            let mut pages = Vec::new();
            let sheet_names = workbook.sheet_names().to_owned();
            for (idx, sheet_name) in sheet_names.iter().enumerate() {
                if let Ok(range) = workbook.worksheet_range(sheet_name) {
                    let mut text = String::new();
                    for row in range.rows() {
                        for cell in row {
                            match cell {
                                calamine::Data::String(s) => text.push_str(s),
                                calamine::Data::Float(f)  => text.push_str(&f.to_string()),
                                calamine::Data::Int(i)    => text.push_str(&i.to_string()),
                                calamine::Data::Bool(b)   => text.push_str(&b.to_string()),
                                _ => {},
                            }
                            text.push(' ');
                        }
                        text.push('\n');
                    }
                    if !text.trim().is_empty() {
                        pages.push((idx as u32 + 1, text));
                    }
                }
            }
            if pages.is_empty() { pages.push((1, String::new())); }
            Ok(pages)
        }
        "pptx" => {
            let file = std::fs::File::open(path)?;
            let mut archive = zip::ZipArchive::new(file)
                .map_err(|e| anyhow::anyhow!("PPTX ZIP error: {}", e))?;
            let mut pages = Vec::new();
            for i in 1.. {
                let slide_path = format!("ppt/slides/slide{}.xml", i);
                if archive.by_name(&slide_path).is_err() {
                    break;
                }
                let mut xml_str = String::new();
                archive.by_name(&slide_path)
                    .map_err(|e| anyhow::anyhow!("读取幻灯片 {}: {}", i, e))?
                    .read_to_string(&mut xml_str)?;
                let mut reader = quick_xml::Reader::from_str(&xml_str);
                let mut buf = Vec::new();
                let mut text = String::new();
                loop {
                    match reader.read_event_into(&mut buf) {
                        Ok(quick_xml::events::Event::Text(e)) => {
                            text.push_str(&e.unescape().unwrap_or_default());
                        }
                        Ok(quick_xml::events::Event::Eof) => break,
                        Err(e) => {
                            return Err(anyhow::anyhow!("XML 解析错误: {}", e));
                        }
                        _ => {}
                    }
                    buf.clear();
                }
                if !text.trim().is_empty() {
                    pages.push((i as u32, text));
                }
            }
            if pages.is_empty() { pages.push((1, String::new())); }
            Ok(pages)
        }
        _ => Ok(vec![]),
    }
}

// 索引辅助函数
pub fn get_file_id(meta_db: &sled::Db, path: &Path) -> Result<Option<u64>> {
    let key = path.to_str().context("invalid path")?.as_bytes();
    if let Ok(Some(val)) = meta_db.get(key) {
        let bytes: [u8; 8] = val
            .as_ref()
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid id length"))?;
        Ok(Some(u64::from_le_bytes(bytes)))
    } else {
        Ok(None)
    }
}

pub fn set_file_id(meta_db: &sled::Db, path: &Path, file_id: u64) -> Result<()> {
    meta_db.insert(
        path.to_str().context("invalid path")?.as_bytes(),
        &file_id.to_le_bytes(),
    )?;
    Ok(())
}

pub fn next_file_id(meta_db: &sled::Db) -> Result<u64> {
    let id_key = b"next_file_id";
    let id = meta_db
        .update_and_fetch(id_key, |old| {
            let old_id = old
                .map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
                .unwrap_or(0);
            Some((old_id + 1).to_le_bytes().to_vec())
        })?
        .map(|b| u64::from_le_bytes(b.as_ref().try_into().unwrap_or([0; 8])))
        .unwrap_or(1);
    Ok(id)
}

pub fn update_single_file(
    writer: &Arc<Mutex<IndexWriter>>,
    meta_db: &sled::Db,
    path: &Path,
    ext: &str,
    _schema: &Schema,
    body_field: Field,
    file_id_field: Field,
    location_field: Field,
    ignore_case: bool,
) -> Result<()> {
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(Some(file_id)) = get_file_id(meta_db, &abs_path) {
        let term = tantivy::Term::from_field_u64(file_id_field, file_id);
        writer.delete_term(term);
        meta_db.remove(&file_id.to_le_bytes())?;
    }
    let pages = extract_pages_safe(&abs_path, ext)?;
    let metadata = std::fs::metadata(&abs_path)?;
    let mtime = metadata.modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs();
    let file_size = metadata.len();
    let md5 = compute_md5(&abs_path)?;
    let filename = abs_path.file_name().unwrap().to_str().unwrap().to_string();
    let file_id = if let Ok(Some(id)) = get_file_id(meta_db, &abs_path) {
        id
    } else {
        next_file_id(meta_db)?
    };
    set_file_id(meta_db, &abs_path, file_id)?;
    let meta = DocMeta {
        path: abs_path,
        filename,
        mtime,
        md5,
        file_size,
    };
    meta_db.insert(&file_id.to_le_bytes(), bincode::serialize(&meta)?)?;
    for (loc, text) in pages {
        if text.trim().is_empty() {
            continue;
        }
        let tokenized_text = tokenize(&text, ignore_case);
        let mut doc = TantivyDocument::default();
        doc.add_u64(file_id_field, file_id);
        doc.add_u64(location_field, loc as u64);
        doc.add_text(body_field, tokenized_text);
        writer.add_document(doc)?;
    }
    writer.commit()?;
    Ok(())
}

pub fn full_scan_and_index(
    writer: &Arc<Mutex<IndexWriter>>,
    meta_db: &sled::Db,
    root: &Path,
    schema: &Schema,
    body_field: Field,
    file_id_field: Field,
    location_field: Field,
    progress_tx: Option<Sender<IndexMsg>>,
    pause_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    ignore_case: bool,
) -> Result<()> {
    let mut total_files = 0;
    for entry in WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let ext = entry.path().extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            if matches!(ext.as_str(), "pdf" | "docx" | "txt" | "xlsx" | "xls" | "pptx") {
                total_files += 1;
            }
        }
    }

    let mut file_index = 0;
    for entry in WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        if stop_flag.load(Ordering::SeqCst) {
            break;
        }
        while pause_flag.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if stop_flag.load(Ordering::SeqCst) {
                break;
            }
        }

        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry.path().extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if !matches!(ext.as_str(), "pdf" | "docx" | "txt" | "xlsx" | "xls" | "pptx") {
            continue;
        }

        let abs_path = entry.path().to_path_buf();
        file_index += 1;
        if let Some(ref tx) = progress_tx {
            let _ = tx.send(IndexMsg::Progress {
                current: abs_path.display().to_string(),
                total: total_files,
                index: file_index,
            });
        }

        let current_mtime = std::fs::metadata(&abs_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let current_md5 = compute_md5(&abs_path).unwrap_or_default();

        if let Ok(Some(file_id)) = get_file_id(meta_db, &abs_path) {
            if let Ok(Some(meta_bytes)) = meta_db.get(&file_id.to_le_bytes()) {
                if let Ok(old_meta) = bincode::deserialize::<DocMeta>(&meta_bytes) {
                    if old_meta.mtime == current_mtime && old_meta.md5 == current_md5 {
                        continue;
                    }
                }
            }
        }

        if let Err(e) = update_single_file(
            writer, meta_db, &abs_path, &ext, schema, body_field, file_id_field, location_field, ignore_case,
        ) {
            let log_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".doc_searcher");
            let _ = std::fs::create_dir_all(&log_dir);
            let error_log = log_dir.join("error.log");
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&error_log) {
                let _ = writeln!(f, "Index error {}: {}", abs_path.display(), e);
            }
        }
    }

    if !stop_flag.load(Ordering::SeqCst) {
        let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
        let mut to_remove = Vec::new();
        for item in meta_db.iter() {
            if let Ok((key, value)) = item {
                if key.len() == 8 {
                    let bytes: [u8; 8] = match key.as_ref().try_into() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let file_id = u64::from_le_bytes(bytes);
                    if let Ok(meta) = bincode::deserialize::<DocMeta>(&value) {
                        if !meta.path.exists() {
                            to_remove.push((file_id, meta.path.clone()));
                        }
                    }
                }
            }
        }
        for (file_id, path) in to_remove {
            let term = tantivy::Term::from_field_u64(file_id_field, file_id);
            writer.delete_term(term);
            meta_db.remove(&file_id.to_le_bytes())?;
            if let Some(p) = path.to_str() {
                meta_db.remove(p.as_bytes())?;
            }
        }
        writer.commit()?;
    }

    if let Some(tx) = progress_tx {
        let _ = tx.send(IndexMsg::Done);
    }
    Ok(())
}

pub fn compare_index(meta_db: &sled::Db, root: &Path) -> (usize, usize, usize) {
    let mut new_count = 0;
    let mut mod_count = 0;
    let mut del_count = 0;

    let mut disk_files: HashMap<PathBuf, (u64, String)> = HashMap::new();
    for entry in WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        if !matches!(ext.as_str(), "pdf" | "docx" | "txt" | "xlsx" | "xls" | "pptx") {
            continue;
        }
        let mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let md5 = compute_md5(&path).unwrap_or_default();
        disk_files.insert(path, (mtime, md5));
    }

    let mut db_files: HashMap<PathBuf, (u64, String)> = HashMap::new();
    for item in meta_db.iter() {
        if let Ok((key, value)) = item {
            if key.len() == 8 {
                if let Ok(meta) = bincode::deserialize::<DocMeta>(&value) {
                    db_files.insert(meta.path.clone(), (meta.mtime, meta.md5.clone()));
                }
            }
        }
    }

    for (path, (mtime, md5)) in &disk_files {
        if let Some((db_mtime, db_md5)) = db_files.get(path) {
            if db_mtime != mtime || db_md5 != md5 {
                mod_count += 1;
            }
        } else {
            new_count += 1;
        }
    }
    for path in db_files.keys() {
        if !disk_files.contains_key(path) {
            del_count += 1;
        }
    }
    (new_count, mod_count, del_count)
}
