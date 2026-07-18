use notify::Watcher;
use tantivy::schema::Value;
use calamine::Reader;
use chrono::TimeZone;
use eframe::egui::{self, Align, Color32, Layout, RichText, ScrollArea, Sense, Vec2};
use rfd::FileDialog;
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};
use crate::doc_searcher::indexer::*;

// ==================== 数据结构 ====================
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
struct DatabaseInfo {
    id: String,
    name: String,
    description: String,
    root_dir: PathBuf,
    created_at: String,
    last_index_time: Option<String>,
    index_subdir: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Config {
    base_dir: String,
    pdf_reader: Option<String>,
    #[serde(default)]
    ignore_case: bool,
    #[serde(default)]
    search_history: Vec<String>,
    #[serde(default)]
    result_view: ResultViewMode,
    #[serde(default = "default_font_scale")]
    font_scale: f32,
    #[serde(default)]
    databases: Vec<DatabaseInfo>,
    #[serde(default)]
    active_db_id: Option<String>,
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



// ==================== 主应用结构体 ====================
pub struct DocSearcherApp {
    pub index: Option<tantivy::Index>,
    pub reader: Option<tantivy::IndexReader>,
    pub writer: Option<Arc<Mutex<tantivy::IndexWriter>>>,
    pub schema: tantivy::schema::Schema,
    pub body_field: tantivy::schema::Field,
    pub file_id_field: tantivy::schema::Field,
    pub location_field: tantivy::schema::Field,
    pub meta_db: Option<sled::Db>,

    pub config: Config,
    config_path: PathBuf,
    base_dir: PathBuf,

    pub root_dir: Option<PathBuf>,
    pub index_state: IndexState,
    pause_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    pub index_status: String,

    log_messages: VecDeque<String>,
    total_indexed: usize,
    current_processing: String,
    progress_total: usize,
    progress_current: usize,

    compare_result: Option<(usize, usize, usize)>,

    search_query: String,
    results: Vec<SearchResult>,
    sort_by: SortBy,
    selected_result: Option<usize>,
    selected_hit: Option<usize>,

    show_settings: bool,
    temp_index_dir: String,
    temp_pdf_reader: String,
    temp_ignore_case: bool,
    show_help: bool,
    show_query_tips: bool,
    font_scale_tmp: f32,

    show_db_manager: bool,
    new_db_name: String,
    new_db_desc: String,
    new_db_root: Option<PathBuf>,

    _watcher: Option<notify::RecommendedWatcher>,
    _watcher_handle: Option<std::thread::JoinHandle<()>>,
    pub progress_rx: Option<Receiver<IndexMsg>>,
}

// ==================== 应用方法实现 ====================
impl DocSearcherApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".doc_searcher");
        std::fs::create_dir_all(&config_dir).ok();
        let config_path = config_dir.join("config.json");
        let config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Config>(&s).ok())
                .unwrap_or_default()
        } else {
            Config::default()
        };

        let base_dir = PathBuf::from(&config.base_dir);
        std::fs::create_dir_all(&base_dir).ok();

        let mut schema_builder = tantivy::schema::Schema::builder();
        let file_id_field = schema_builder.add_u64_field("file_id", tantivy::schema::STORED);
        let location_field = schema_builder.add_u64_field("location", tantivy::schema::STORED);
        let text_options = tantivy::schema::TextOptions::default()
            .set_indexing_options(
                tantivy::schema::TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let body_field = schema_builder.add_text_field("body", text_options);
        let schema = schema_builder.build();

        let mut app = Self {
            index: None,
            reader: None,
            writer: None,
            schema,
            body_field,
            file_id_field,
            location_field,
            meta_db: None,
            config: config.clone(),
            config_path,
            base_dir,
            root_dir: None,
            index_state: IndexState::Idle,
            pause_flag: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            index_status: "未加载数据库".to_string(),
            log_messages: VecDeque::with_capacity(100),
            total_indexed: 0,
            current_processing: String::new(),
            progress_total: 0,
            progress_current: 0,
            compare_result: None,
            search_query: String::new(),
            results: vec![],
            sort_by: SortBy::Relevance,
            selected_result: None,
            selected_hit: None,
            show_settings: false,
            temp_index_dir: String::new(),
            temp_pdf_reader: String::new(),
            temp_ignore_case: true,
            show_help: false,
            show_query_tips: false,
            font_scale_tmp: config.font_scale,
            show_db_manager: false,
            new_db_name: String::new(),
            new_db_desc: String::new(),
            new_db_root: None,
            _watcher: None,
            _watcher_handle: None,
            progress_rx: None,
        };

        if let Some(ref active_id) = app.config.active_db_id.clone() {
            if let Err(e) = app.load_database(active_id) {
                app.log(format!("加载数据库失败: {}", e));
                app.config.active_db_id = None;
                app.save_config();
            }
        }
        app
    }

    // ---------- 数据库管理 ----------
    fn db_dir(&self, db_id: &str) -> PathBuf {
        self.base_dir.join("db").join(db_id)
    }

    fn load_database(&mut self, db_id: &str) -> anyhow::Result<()> {
        self.close_current_db();
        let db_info = self.config.databases.iter()
            .find(|d| d.id == db_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("数据库不存在"))?;

        let db_path = self.db_dir(db_id);
        let index_path = db_path.join(&db_info.index_subdir).join("index");
        let meta_path = db_path.join(&db_info.index_subdir).join("meta");
        std::fs::create_dir_all(&index_path)?;
        std::fs::create_dir_all(&meta_path)?;

        let index = tantivy::Index::open_in_dir(&index_path).unwrap_or_else(|_| {
            tantivy::Index::create_in_dir(&index_path, self.schema.clone())
                .expect("无法创建索引目录")
        });
        let reader = index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let writer = Arc::new(Mutex::new(index.writer(50_000_000)?));
        let meta_db = sled::open(&meta_path)?;

        self.index = Some(index);
        self.reader = Some(reader);
        self.writer = Some(writer);
        self.meta_db = Some(meta_db);
        self.root_dir = Some(db_info.root_dir.clone());
        self.total_indexed = Self::count_indexed_files(self.meta_db.as_ref().unwrap());
        self.index_status = format!("数据库: {}", db_info.name);
        self.config.active_db_id = Some(db_id.to_string());
        self.save_config();
        self.log(format!("已切换到数据库: {}", db_info.name));
        Ok(())
    }

    fn close_current_db(&mut self) {
        self.stop_indexing();
        self._watcher = None;
        self._watcher_handle = None;
        self.progress_rx = None;
        self.index = None;
        self.reader = None;
        self.writer = None;
        self.meta_db = None;
        self.root_dir = None;
        self.total_indexed = 0;
        self.results.clear();
        self.selected_result = None;
        self.selected_hit = None;
    }

    fn create_database(&mut self, name: String, desc: String, root: PathBuf) -> anyhow::Result<()> {
        let id = format!("db_{}", chrono::Utc::now().timestamp_micros());
        let db_info = DatabaseInfo {
            id: id.clone(),
            name,
            description: desc,
            root_dir: root.canonicalize().unwrap_or(root),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            last_index_time: None,
            index_subdir: "data".to_string(),
        };
        let db_path = self.db_dir(&id);
        std::fs::create_dir_all(db_path.join(&db_info.index_subdir))?;

        self.config.databases.push(db_info);
        self.config.active_db_id = Some(id.clone());
        self.save_config();
        self.load_database(&id)?;
        self.log("新数据库已创建并激活".to_string());
        Ok(())
    }

    fn delete_database(&mut self, db_id: &str) {
        if self.config.active_db_id.as_deref() == Some(db_id) {
            self.close_current_db();
            self.config.active_db_id = None;
        }
        self.config.databases.retain(|d| d.id != db_id);
        let db_path = self.db_dir(db_id);
        let _ = std::fs::remove_dir_all(db_path);
        self.save_config();
        self.log(format!("数据库已删除: {}", db_id));
    }

    fn count_indexed_files(meta_db: &sled::Db) -> usize {
        meta_db.iter()
            .filter(|item| {
                if let Ok((key, _)) = item {
                    key.len() == 8
                } else {
                    false
                }
            })
            .count()
    }

    fn save_config(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.config) {
            let _ = std::fs::write(&self.config_path, json);
        }
    }

    // ---------- 索引控制 ----------
    fn start_watcher(&mut self) {
        let root = match &self.root_dir {
            Some(d) => d.clone(),
            None => return,
        };
        let writer = self.writer.clone().expect("无激活数据库");
        let meta_db = self.meta_db.clone().expect("无激活数据库");
        let schema = self.schema.clone();
        let body_field = self.body_field;
        let file_id_field = self.file_id_field;
        let location_field = self.location_field;
        let ignore_case = self.config.ignore_case;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                if let notify::EventKind::Modify(_) | notify::EventKind::Create(_) = event.kind {
                    for p in event.paths {
                        let _ = tx.send(p);
                    }
                }
            }
        }).expect("Failed to create file watcher");
        watcher.watch(&root, notify::RecursiveMode::Recursive).expect("Failed to watch directory");
        self._watcher = Some(watcher);

        let handle = std::thread::spawn(move || {
            for path in rx {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if meta.is_file() {
                        let ext = path.extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if matches!(ext.as_str(), "pdf" | "docx" | "txt" | "xlsx" | "xls" | "pptx") {
                            if let Ok(abs_path) = path.canonicalize() {
                                let _ = update_single_file(
                                    &writer, &meta_db, &abs_path, &ext, &schema,
                                    body_field, file_id_field, location_field, ignore_case,
                                );
                            }
                        }
                    }
                }
            }
        });
        self._watcher_handle = Some(handle);
    }

    fn start_indexing(&mut self) {
        if self.index_state == IndexState::Indexing || self.writer.is_none() {
            if self.writer.is_none() {
                self.log("请先选择或激活数据库".to_string());
            }
            return;
        }
        self.index_state = IndexState::Indexing;
        self.stop_flag.store(false, Ordering::SeqCst);
        self.pause_flag.store(false, Ordering::SeqCst);
        let root = self.root_dir.as_ref().unwrap().clone();
        let writer = self.writer.as_ref().unwrap().clone();
        let meta_db = self.meta_db.as_ref().unwrap().clone();
        let schema = self.schema.clone();
        let body_field = self.body_field;
        let file_id_field = self.file_id_field;
        let location_field = self.location_field;
        let pause_flag = self.pause_flag.clone();
        let stop_flag = self.stop_flag.clone();
        let ignore_case = self.config.ignore_case;

        let (tx, rx) = channel::<IndexMsg>();
        self.progress_rx = Some(rx);
        self.progress_total = 0;
        self.progress_current = 0;

        if self._watcher.is_none() {
            self.start_watcher();
        }

        std::thread::spawn(move || {
            let _ = full_scan_and_index(
                &writer, &meta_db, &root, &schema, body_field, file_id_field, location_field,
                Some(tx), pause_flag, stop_flag, ignore_case,
            );
        });
        self.log("开始建立索引...".to_string());
    }

    fn pause_indexing(&mut self) {
        self.pause_flag.store(true, Ordering::SeqCst);
        self.index_state = IndexState::Paused;
        self.log("索引已暂停".to_string());
    }

    fn resume_indexing(&mut self) {
        self.pause_flag.store(false, Ordering::SeqCst);
        self.index_state = IndexState::Indexing;
        self.log("索引已恢复".to_string());
    }

    fn stop_indexing(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        self.pause_flag.store(false, Ordering::SeqCst);
        self.index_state = IndexState::Idle;
        if let Some(ref meta) = self.meta_db {
            self.total_indexed = Self::count_indexed_files(meta);
        }
        self.log("索引已停止，临时资源已释放".to_string());
        self.index_status = "索引已停止".to_string();
    }

    fn trigger_compare(&mut self) {
        let root = match &self.root_dir {
            Some(d) => d.clone(),
            None => return,
        };
        if let Some(ref meta) = self.meta_db {
            let (new_count, mod_count, del_count) = compare_index(meta, &root);
            self.compare_result = Some((new_count, mod_count, del_count));
            self.log(format!(
                "对比结果：新增 {}，修改 {}，删除 {}",
                new_count, mod_count, del_count
            ));
        }
    }

    fn rebuild_index(&mut self) {
        if self.writer.is_none() {
            self.log("请先激活数据库".to_string());
            return;
        }
        if let Ok(mut writer) = self.writer.as_ref().unwrap().lock() {
            let _ = writer.delete_all_documents();
            let _ = writer.commit();
        }
        if let Some(ref meta) = self.meta_db {
            let _ = meta.clear();
        }
        self.total_indexed = 0;
        self.start_indexing();
    }

    // ---------- 搜索 ----------
    fn search(&mut self) {
        let reader = match &self.reader {
            Some(r) => r,
            None => {
                self.log("无激活数据库".to_string());
                return;
            }
        };
        let meta_db = match &self.meta_db {
            Some(m) => m,
            None => return,
        };
        self.results.clear();
        self.selected_result = None;
        self.selected_hit = None;
        let query_str = self.search_query.trim();
        if query_str.is_empty() {
            return;
        }

        let q = query_str.to_string();
        if !self.config.search_history.contains(&q) {
            self.config.search_history.insert(0, q.clone());
            self.config.search_history.truncate(20);
            self.save_config();
        }

        let ignore_case = self.config.ignore_case;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let tokenized = tokenize_query(query_str, ignore_case);
            let query_parser = tantivy::query::QueryParser::for_index(
                self.index.as_ref().unwrap(),
                vec![self.body_field],
            );
            let query = query_parser
                .parse_query(&tokenized)
                .map_err(|e| anyhow::anyhow!("查询解析错误: {}", e))?;

            let searcher = reader.searcher();
            let top_docs = searcher
                .search(&query, &tantivy::collector::TopDocs::with_limit(500))
                .map_err(|e| anyhow::anyhow!("搜索错误: {}", e))?;

            let query_words: Vec<String> = tokenized
                .split_whitespace()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if query_words.is_empty() {
                return Ok(());
            }

            for (score, doc_address) in top_docs {
                if let Ok(doc) = searcher.doc::<tantivy::TantivyDocument>(doc_address) {
                    let file_id = doc
                        .get_first(self.file_id_field)
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let location = doc
                        .get_first(self.location_field)
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    if let Ok(Some(meta_bytes)) = meta_db.get(&(file_id as u64).to_le_bytes()) {
                        if let Ok(meta) = bincode::deserialize::<DocMeta>(&meta_bytes) {
                            let body_text = doc
                                .get_first(self.body_field)
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let keyword_count = query_words
                                .iter()
                                .map(|w| body_text.split(' ').filter(|&t| t == w).count())
                                .sum();
                            let mut hits = Vec::new();
                            for kw in &query_words {
                                let mut start = 0;
                                while start < body_text.len() {
                                    if let Some(pos) = body_text[start..].find(kw) {
                                        let abs_pos = start + pos;
                                        let begin = abs_pos.saturating_sub(250);
                                        let snippet = safe_slice(body_text, begin, kw.len() + 500)
                                            .to_string();
                                        hits.push(KeywordHit {
                                            location: location as u32,
                                            snippet,
                                            keyword: kw.clone(),
                                        });
                                        start = abs_pos + kw.len();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            let main_snippet = {
                                let snippet = safe_slice(body_text, 0, 500).to_string();
                                let mut s = if body_text.len() > 500 {
                                    format!("{}...", snippet)
                                } else {
                                    snippet
                                };
                                for kw in &query_words {
                                    s = s.replace(kw, &format!("【{}】", kw));
                                }
                                s
                            };
                            self.results.push(SearchResult {
                                file_id,
                                filename: meta.filename.clone(),
                                path: meta.path.clone(),
                                location,
                                snippet: main_snippet,
                                score,
                                mtime: meta.mtime,
                                keyword_count,
                                hits,
                            });
                        }
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        }));

        match result {
            Ok(Ok(())) => {
                Self::sort_results(&mut self.results, &self.sort_by);
                self.index_status = format!("找到 {} 个结果", self.results.len());
            }
            Ok(Err(e)) => {
                self.index_status = format!("搜索失败: {}", e);
                self.log(format!("搜索错误: {}", e));
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                self.index_status = format!("搜索崩溃: {}", msg);
                self.log(format!("搜索崩溃: {}", msg));
            }
        }
    }

    fn sort_results(results: &mut Vec<SearchResult>, sort_by: &SortBy) {
        match sort_by {
            SortBy::Relevance => {
                results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
            }
            SortBy::FileNameAsc => {
                results.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));
            }
            SortBy::FileNameDesc => {
                results.sort_by(|a, b| {
                    b.filename
                        .to_lowercase()
                        .cmp(&a.filename.to_lowercase())
                        .reverse()
                });
            }
            SortBy::DateNewest => results.sort_by(|a, b| b.mtime.cmp(&a.mtime)),
            SortBy::DateOldest => results.sort_by(|a, b| a.mtime.cmp(&b.mtime)),
            SortBy::Frequency => results.sort_by(|a, b| b.keyword_count.cmp(&a.keyword_count)),
        }
    }

    fn resort_current_results(&mut self) {
        let sort_by = self.sort_by.clone();
        Self::sort_results(&mut self.results, &sort_by);
    }

    // ---------- 文件打开 ----------
    fn open_file_with_location(&mut self, res: &SearchResult) {
        let path = res.path.clone();
        let location = res.location;
        let query = self.search_query.clone();
        self.open_with_command(&path, location, &query);
    }

    fn open_hit_with_location(&mut self, hit: &KeywordHit, file_path: &Path) {
        self.open_with_command(file_path, hit.location as u64, &hit.keyword);
    }

    fn open_with_command(&mut self, file_path: &Path, page: u64, keyword: &str) {
        self.log(format!(
            "[open] 参数: file_path='{}', page={}, keyword='{}'",
            file_path.display(),
            page,
            keyword
        ));

        let abs_path = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else if let Some(root) = self.root_dir.clone() {
            let joined = root.join(file_path);
            match joined.canonicalize() {
                Ok(canon) => canon,
                Err(_) => joined,
            }
        } else {
            file_path.to_path_buf()
        };

        let mut opened = false;
        if abs_path.extension().map_or(false, |e| e == "pdf") {
            if let Some(ref cmd_template) = self.config.pdf_reader {
                let cmd = cmd_template
                    .replace("{file}", &abs_path.display().to_string())
                    .replace("{page}", &page.to_string())
                    .replace("{keyword}", keyword);
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if !parts.is_empty() {
                    match std::process::Command::new(parts[0])
                        .args(&parts[1..])
                        .spawn()
                    {
                        Ok(_) => opened = true,
                        Err(e) => self.log(format!("启动 PDF 阅读器失败: {}", e)),
                    }
                }
            }
        }

        if !opened {
            if let Err(e) = open::that(&abs_path) {
                self.log(format!("打开文件失败: {}", e));
            }
        }
    }

    // ---------- 工具方法 ----------
    fn log(&mut self, msg: String) {
        self.log_messages.push_back(msg);
        if self.log_messages.len() > 100 {
            self.log_messages.pop_front();
        }
    }

    fn copy_path_to_clipboard(&mut self, path: &Path, ctx: &egui::Context) {
        ctx.output_mut(|o| o.copied_text = path.to_string_lossy().to_string());
        self.log(format!("已复制路径: {}", path.display()));
    }

    fn export_results_to_csv(&self, path: &Path) -> anyhow::Result<()> {
        let meta_db = match &self.meta_db {
            Some(m) => m,
            None => return Err(anyhow::anyhow!("无数据库")),
        };
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record(&[
            "文件名", "路径", "位置", "分数", "命中次数", "修改时间", "文件大小", "摘要",
        ])?;
        for res in &self.results {
            let file_size_str = if let Ok(Some(meta_bytes)) = meta_db.get(&res.file_id.to_le_bytes()) {
                if let Ok(meta) = bincode::deserialize::<DocMeta>(&meta_bytes) {
                    format_file_size(meta.file_size)
                } else {
                    "?".to_string()
                }
            } else {
                "?".to_string()
            };
            wtr.write_record(&[
                &res.filename,
                &res.path.to_string_lossy().into_owned(),
                &res.location.to_string(),
                &format!("{:.2}", res.score),
                &res.keyword_count.to_string(),
                &format_timestamp(res.mtime),
                &file_size_str,
                &res.snippet,
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }

    fn clear_search(&mut self) {
        self.search_query.clear();
        self.results.clear();
        self.selected_result = None;
        self.selected_hit = None;
    }
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // 处理索引进度消息
        let mut progress_msgs = Vec::new();
        if let Some(ref rx) = self.progress_rx {
            while let Ok(msg) = rx.try_recv() {
                progress_msgs.push(msg);
            }
        }
        for msg in progress_msgs {
            match msg {
                IndexMsg::Progress { current, total, index } => {
                    self.current_processing = format!("第 {}/{} 个文件: {}", index, total, current);
                    self.total_indexed = total;
                    self.progress_total = total;
                    self.progress_current = index;
                    self.log(format!("正在索引: {}", self.current_processing));
                }
                IndexMsg::Done => {
                    if let Some(ref meta) = self.meta_db {
                        self.total_indexed = Self::count_indexed_files(meta);
                    }
                    self.log("索引完成".to_string());
                    self.progress_rx = None;
                    self.index_state = IndexState::Idle;
                    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    if let Some(ref active_id) = self.config.active_db_id.clone() {
                        if let Some(db) = self.config.databases.iter_mut().find(|d| d.id == *active_id) {
                            db.last_index_time = Some(now);
                            self.save_config();
                        }
                    }
                }
            }
        }

        // 快捷键
        let input = ctx.input(|i| i.clone());
        if input.modifiers.ctrl && input.keys_down.contains(&egui::Key::F) {
            ctx.memory_mut(|mem| mem.request_focus(egui::Id::new("doc_search_text")));
        }
        if input.keys_down.contains(&egui::Key::Escape) {
            self.clear_search();
            ctx.memory_mut(|mem| mem.request_focus(egui::Id::new("doc_search_text")));
        }

        // 设置字体缩放
        let _ = ctx.pixels_per_point(); // 保留原逻辑，实际不使用全局缩放

        ui.vertical(|ui| {
            // ---- 顶部工具栏 ----
            ui.horizontal(|ui| {
                if let Some(ref active_id) = self.config.active_db_id.clone() {
                    if let Some(db) = self.config.databases.iter().find(|d| d.id == *active_id) {
                        ui.label(RichText::new(format!("📚 {}", db.name)).strong());
                        ui.label(format!("({})", db.root_dir.display()));
                    }
                } else {
                    ui.label("未激活数据库");
                }
                ui.separator();
                if ui.button("📂 管理数据库").clicked() {
                    self.show_db_manager = true;
                }
                ui.separator();
                ui.add_enabled_ui(self.writer.is_some(), |ui| {
                    match self.index_state {
                        IndexState::Idle => {
                            if ui.button("▶ 开始索引").clicked() {
                                self.start_indexing();
                            }
                        }
                        IndexState::Indexing => {
                            if ui.button("⏸ 暂停").clicked() {
                                self.pause_indexing();
                            }
                            if ui.button("⏹ 停止").clicked() {
                                self.stop_indexing();
                            }
                        }
                        IndexState::Paused => {
                            if ui.button("▶ 恢复").clicked() {
                                self.resume_indexing();
                            }
                            if ui.button("⏹ 停止").clicked() {
                                self.stop_indexing();
                            }
                        }
                    }
                    if ui.button("📊 对比").clicked() {
                        self.trigger_compare();
                    }
                });
                ui.separator();
                ui.label("排序:");
                let prev = self.sort_by.clone();
                egui::ComboBox::from_id_source("sort_combo")
                    .selected_text(match self.sort_by {
                        SortBy::Relevance => "🔥相关度",
                        SortBy::FileNameAsc => "📄文件名↑",
                        SortBy::FileNameDesc => "📄文件名↓",
                        SortBy::DateNewest => "🕒最新",
                        SortBy::DateOldest => "🕒最早",
                        SortBy::Frequency => "🔢词频",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.sort_by, SortBy::Relevance, "🔥相关度");
                        ui.selectable_value(&mut self.sort_by, SortBy::FileNameAsc, "📄文件名↑");
                        ui.selectable_value(&mut self.sort_by, SortBy::FileNameDesc, "📄文件名↓");
                        ui.selectable_value(&mut self.sort_by, SortBy::DateNewest, "🕒最新");
                        ui.selectable_value(&mut self.sort_by, SortBy::DateOldest, "🕒最早");
                        ui.selectable_value(&mut self.sort_by, SortBy::Frequency, "🔢词频");
                    });
                if self.sort_by != prev {
                    self.resort_current_results();
                }
                ui.separator();
                egui::ComboBox::from_id_source("view_combo")
                    .selected_text(
                        if self.config.result_view == ResultViewMode::Compact {
                            "📋紧凑"
                        } else {
                            "📊详细"
                        },
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.config.result_view,
                            ResultViewMode::Compact,
                            "📋紧凑",
                        );
                        ui.selectable_value(
                            &mut self.config.result_view,
                            ResultViewMode::Detailed,
                            "📊详细",
                        );
                    });
                ui.separator();
                if ui.button("⚙️设置").clicked() {
                    self.show_settings = true;
                }
                if ui.button("❓帮助").clicked() {
                    self.show_help = true;
                }
            });

            // ---- 进度条 ----
            if self.index_state != IndexState::Idle && self.progress_total > 0 {
                let p = self.progress_current as f32 / self.progress_total.max(1) as f32;
                ui.add(
                    egui::ProgressBar::new(p)
                        .text(format!("索引: {}/{}", self.progress_current, self.progress_total)),
                );
            }

            // ---- 搜索栏与主内容区 ----
            if self.reader.is_some() {
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("输入关键词搜索...")
                            .desired_width(250.0)
                            .id(egui::Id::new("doc_search_text")),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.search();
                    }
                    if ui.button("🔍搜索").clicked() {
                        self.search();
                    }
                    if ui.button("🗑清除").clicked() {
                        self.clear_search();
                    }
                    let mut ignore = self.config.ignore_case;
                    if ui.checkbox(&mut ignore, "忽略大小写").clicked() {
                        self.config.ignore_case = ignore;
                        self.save_config();
                    }
                    if !self.config.search_history.is_empty() {
                        egui::ComboBox::from_id_source("search_hist")
                            .selected_text("历史")
                            .show_ui(ui, |ui| {
                                for h in self.config.search_history.clone() {
                                    if ui.selectable_label(false, &h).clicked() {
                                        self.search_query = h;
                                    }
                                }
                            });
                    }
                    ui.toggle_value(&mut self.show_query_tips, "💡");
                    if self.show_query_tips {
                        ui.label("支持 AND/OR/NOT、通配符 *, ?、短语 \"...\" 等");
                    }
                });

                ui.label(format!(
                    "已索引: {} | 上次索引: {} | {}",
                    self.total_indexed,
                    self.config
                        .databases
                        .iter()
                        .find(|d| Some(d.id.as_str()) == self.config.active_db_id.as_deref())
                        .and_then(|d| d.last_index_time.as_ref())
                        .unwrap_or(&"无".to_string()),
                    self.index_status
                ));
                if let Some((n, m, d)) = self.compare_result {
                    ui.label(format!("对比: 新增{} 修改{} 删除{}", n, m, d));
                }
                if !self.results.is_empty() {
                    if ui.button("📤导出CSV").clicked() {
                        if let Some(path) = FileDialog::new()
                            .set_file_name("search_results.csv")
                            .save_file()
                        {
                            if let Err(e) = self.export_results_to_csv(&path) {
                                self.log(format!("导出失败: {}", e));
                            } else {
                                self.log(format!("已导出到 {}", path.display()));
                            }
                        }
                    }
                }
            } else {
                ui.label("请先创建或激活一个数据库以开始使用。");
            }

            // ---- 结果区域 ----
            if self.reader.is_some() && !self.results.is_empty() {
                ui.columns(2, |cols| {
                    let left = &mut cols[0];
                    left.heading("搜索结果");
                    let mut open_later: Option<SearchResult> = None;
                    let mut select_idx: Option<usize> = None;
                    let mut copy_later: Option<PathBuf> = None;

                    ScrollArea::vertical().show(left, |ui| {
                        let h = match self.config.result_view {
                            ResultViewMode::Compact => 60.0,
                            ResultViewMode::Detailed => 90.0,
                        };
                        for (i, res) in self.results.iter().enumerate() {
                            let selected = Some(i) == self.selected_result;
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), h),
                                Sense::click(),
                            );
                            let mut child = ui.child_ui(rect, *ui.layout());
                            let frame = egui::Frame::group(child.style())
                                .fill(if selected {
                                    Color32::from_rgb(230, 240, 255)
                                } else {
                                    child.visuals().extreme_bg_color
                                })
                                .stroke(egui::Stroke::new(1.0, Color32::GRAY));
                            frame.show(&mut child, |ui| {
                                ui.set_min_height(h);
                                ui.vertical(|ui| {
                                    match self.config.result_view {
                                        ResultViewMode::Compact => {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(format!("{}. {}", i + 1, res.filename))
                                                        .strong()
                                                        .color(Color32::from_rgb(0, 100, 200)),
                                                );
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        ui.label(format!("🔢{}次", res.keyword_count));
                                                        if ui.button("📍").clicked() {
                                                            open_later = Some(res.clone());
                                                        }
                                                        if ui.button("📋").clicked() {
                                                            copy_later = Some(res.path.clone());
                                                        }
                                                    },
                                                );
                                            });
                                            ui.label(format!("{} | 分数:{:.2}", res.snippet, res.score));
                                        }
                                        ResultViewMode::Detailed => {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(format!("{}. {}", i + 1, res.filename))
                                                        .strong()
                                                        .color(Color32::from_rgb(0, 100, 200)),
                                                );
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if ui.button("📍").clicked() {
                                                            open_later = Some(res.clone());
                                                        }
                                                        if ui.button("📋").clicked() {
                                                            copy_later = Some(res.path.clone());
                                                        }
                                                    },
                                                );
                                            });
                                            ui.label(format!("路径: {}", res.path.display()));
                                            ui.label(format!(
                                                "修改: {} | 大小: {}",
                                                format_timestamp(res.mtime),
                                                self.meta_db
                                                    .as_ref()
                                                    .and_then(|m| m
                                                        .get(&res.file_id.to_le_bytes())
                                                        .ok()
                                                        .flatten())
                                                    .and_then(|b| {
                                                        bincode::deserialize::<DocMeta>(&b).ok()
                                                    })
                                                    .map(|m| format_file_size(m.file_size))
                                                    .unwrap_or_else(|| "?".to_string())
                                            ));
                                            ui.label(format!("片段: {}", res.snippet));
                                        }
                                    }
                                });
                            });
                            if resp.double_clicked() {
                                open_later = Some(res.clone());
                            } else if resp.clicked() {
                                select_idx = Some(i);
                            }
                        }
                    });
                    if let Some(res) = open_later {
                        self.open_file_with_location(&res);
                    }
                    if let Some(idx) = select_idx {
                        self.selected_result = Some(idx);
                        self.selected_hit = None;
                    }
                    if let Some(p) = copy_later {
                        self.copy_path_to_clipboard(&p, &ctx);
                    }

                    let right = &mut cols[1];
                    if let Some(idx) = self.selected_result {
                        if let Some(res) = self.results.get(idx) {
                            right.heading("命中详情");
                            let fp = res.path.clone();
                            let mut open_hit = None;
                            let mut select_hit = None;
                            ScrollArea::vertical()
                                .id_source("right_scroll")
                                .show(right, |ui| {
                                    for (hi, hit) in res.hits.iter().enumerate() {
                                        let selected = Some(hi) == self.selected_hit;
                                        let frame = egui::Frame::group(ui.style())
                                            .fill(if selected {
                                                Color32::from_rgb(240, 240, 220)
                                            } else {
                                                ui.visuals().extreme_bg_color
                                            })
                                            .stroke(egui::Stroke::new(1.0, Color32::GRAY));
                                        let inner = frame.show(ui, |ui| {
                                            ui.set_min_width(ui.available_width() - 10.0);
                                            ui.horizontal(|ui| {
                                                ui.label(format!("{} [{}]", hit.keyword, hit.location));
                                                if ui.button("📍").clicked() {
                                                    open_hit = Some(hit.clone());
                                                }
                                                if ui.button("🔍").clicked() {
                                                    select_hit = Some(hi);
                                                }
                                            });
                                            ui.label(&hit.snippet);
                                        });
                                        if inner.response.clicked() {
                                            select_hit = Some(hi);
                                        }
                                    }
                                    if let Some(hi) = select_hit.or(self.selected_hit) {
                                        if let Some(hit) = res.hits.get(hi) {
                                            ui.separator();
                                            ui.label("高亮预览:");
                                            ui.label(
                                                hit.snippet.replace(
                                                    &hit.keyword,
                                                    &format!("【{}】", hit.keyword),
                                                ),
                                            );
                                        }
                                    }
                                });
                            if let Some(hit) = open_hit {
                                self.open_hit_with_location(&hit, &fp);
                            }
                            if let Some(hi) = select_hit {
                                self.selected_hit = Some(hi);
                            }
                        }
                    } else {
                        right.label("选择结果查看详情");
                    }
                });
            }
        });

        // 日志区域（底部）
        ui.separator();
        ui.label("日志:");
        ScrollArea::vertical()
            .id_source("doc_log_scroll")
            .max_height(100.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for msg in self.log_messages.iter().rev() {
                    ui.label(msg);
                }
            });

        // ---- 弹出窗口 ----
        // 设置窗口
        if self.show_settings {
            egui::Window::new("设置").collapsible(false).show(&ctx, |ui| {
                ui.label("索引存储基础目录:");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.config.base_dir);
                    if ui.button("浏览").clicked() {
                        if let Some(dir) = FileDialog::new().pick_folder() {
                            self.config.base_dir = dir.to_string_lossy().to_string();
                            self.base_dir = dir;
                        }
                    }
                    if ui.button("打开文件夹").clicked() {
                        let _ = open::that(&self.base_dir);
                    }
                });
                ui.label("PDF阅读器命令:");
                ui.text_edit_singleline(&mut self.temp_pdf_reader);
                ui.label("占位符: {file} {page} {keyword}");
                ui.checkbox(&mut self.temp_ignore_case, "忽略大小写(需重建索引)");
                if ui.button("重建当前索引").clicked() {
                    self.show_settings = false;
                    self.rebuild_index();
                }
                ui.horizontal(|ui| {
                    ui.label("界面缩放:");
                    ui.add(egui::Slider::new(&mut self.font_scale_tmp, 0.5..=2.0).step_by(0.1));
                });
                if ui.button("保存").clicked() {
                    self.config.pdf_reader = if self.temp_pdf_reader.is_empty() {
                        None
                    } else {
                        Some(self.temp_pdf_reader.clone())
                    };
                    self.config.ignore_case = self.temp_ignore_case;
                    self.config.font_scale = self.font_scale_tmp;
                    self.save_config();
                    self.show_settings = false;
                }
                if ui.button("取消").clicked() {
                    self.show_settings = false;
                }
            });
        }

        // 帮助窗口
        if self.show_help {
            egui::Window::new("帮助").collapsible(false).show(&ctx, |ui| {
                ui.heading("本地文档检索系统 v2.1");
                ui.separator();
                ui.label("快捷键: Ctrl+F 搜索, Esc 清除");
                ui.label("支持索引的文件格式: PDF, DOCX, TXT, XLSX, XLS, PPTX");
                ui.separator();
                ui.label("高级查询语法：");
                ui.label("• 使用 AND / OR / NOT 组合关键词，例如: 中国 AND 美国");
                ui.label("• 通配符 * (任意多个字符) 和 ? (单个字符)，例如: 程序*");
                ui.label("• 短语匹配请用英文双引号，例如: \"机器 学习\" (注意中文词间空格)");
                ui.label("• 使用 + 表示必须出现，- 表示不能出现，例如: +北京 -上海");
                ui.separator();
                ui.label("页面跳转：");
                ui.label("• 对于 PDF/PPTX，搜索结果中的位置编号即为页码或幻灯片编号。");
                ui.label("• 在设置中配置 PDF 阅读器命令，例如 SumatraPDF:");
                ui.label(
                    "    \"C:\\Program Files\\SumatraPDF\\SumatraPDF.exe\" -page {page} \"{file}\"",
                );
                ui.label("  或 Foxit Reader: \"FoxitReader.exe\" /A page={page} \"{file}\"");
                ui.separator();
                if ui.button("关闭").clicked() {
                    self.show_help = false;
                }
            });
        }

        // 数据库管理窗口
        if self.show_db_manager {
            egui::Window::new("数据库管理")
                .collapsible(false)
                .show(&ctx, |ui| {
                    ui.heading("已有数据库");
                    let mut to_activate: Option<String> = None;
                    let mut to_delete: Option<String> = None;
                    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        for db in &self.config.databases {
                            let selected = self.config.active_db_id.as_deref() == Some(&db.id);
                            let frame = egui::Frame::group(ui.style()).fill(if selected {
                                Color32::from_rgb(200, 255, 200)
                            } else {
                                ui.visuals().extreme_bg_color
                            });
                            frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!("📚 {} ({})", db.name, db.description));
                                    if ui.button("激活").clicked() {
                                        to_activate = Some(db.id.clone());
                                    }
                                    if ui.button("删除").clicked() {
                                        to_delete = Some(db.id.clone());
                                    }
                                });
                                ui.label(format!(
                                    "路径: {} | 创建: {}",
                                    db.root_dir.display(),
                                    db.created_at
                                ));
                            });
                        }
                    });
                    if let Some(id) = to_activate {
                        if let Err(e) = self.load_database(&id) {
                            self.log(format!("激活失败: {}", e));
                        }
                        self.show_db_manager = false;
                    }
                    if let Some(id) = to_delete {
                        self.delete_database(&id);
                    }

                    ui.separator();
                    ui.heading("新建数据库");
                    ui.horizontal(|ui| {
                        ui.label("名称:");
                        ui.text_edit_singleline(&mut self.new_db_name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("简介:");
                        ui.text_edit_singleline(&mut self.new_db_desc);
                    });
                    if ui.button("选择目标文件夹").clicked() {
                        if let Some(dir) = FileDialog::new().pick_folder() {
                            self.new_db_root = Some(dir);
                        }
                    }
                    if let Some(ref path) = self.new_db_root {
                        ui.label(format!("已选: {}", path.display()));
                    }
                    if ui.button("创建并激活").clicked() {
                        if self.new_db_name.trim().is_empty() {
                            self.log("名称不能为空".to_string());
                        } else if self.new_db_root.is_none() {
                            self.log("请选择文件夹".to_string());
                        } else {
                            let root = self.new_db_root.take().unwrap();
                            if let Err(e) = self.create_database(
                                self.new_db_name.clone(),
                                self.new_db_desc.clone(),
                                root,
                            ) {
                                self.log(format!("创建失败: {}", e));
                            } else {
                                self.new_db_name.clear();
                                self.new_db_desc.clear();
                                self.show_db_manager = false;
                            }
                        }
                    }
                    if ui.button("关闭").clicked() {
                        self.show_db_manager = false;
                    }
                });
        }
    }    
}
// ==================== 重构后的渲染入口 ====================



// 原 main.rs 中的独立函数，转换为模块级函数
fn format_timestamp(secs: u64) -> String {
    if let Some(dt) = chrono::Utc.timestamp_opt(secs as i64, 0).single() {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        "时间错误".to_string()
    }
}

fn format_file_size(size: u64) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}

