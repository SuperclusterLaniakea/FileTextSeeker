pub mod extract;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use tantivy::collector::TopDocs;
use tantivy::query::{AllQuery, BooleanQuery, BoostQuery, Occur, PhraseQuery, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TantivyDocument, TextFieldIndexing, TextOptions, Value,
    STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, TextAnalyzer, Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, Term};
use walkdir::WalkDir;

use crate::util;

pub use extract::{extract_file, supported_extension};

const INDEX_DIR: &str = "index";
const META_DB: &str = "meta.sqlite3";
const TOKENIZER: &str = "jieba";

const SKIP_DIRS: [&str; 14] = [
    "$recycle.bin",
    "system volume information",
    "node_modules",
    ".git",
    ".svn",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".idea",
    ".vscode",
    "appdata",
    "windows",
    "program files",
];

#[derive(Clone, Debug, Default)]
pub struct FileMeta {
    pub size: u64,
    pub mtime: i64,
    pub units: u32,
}

#[derive(Clone, Debug, Default)]
pub struct BuildStats {
    pub scanned: u64,
    pub indexed_files: u64,
    pub skipped: u64,
    pub removed: u64,
    pub units: u64,
    pub errors: u64,
    pub cancelled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildMode {
    Rebuild,
    Update,
}

#[derive(Clone, Debug)]
pub struct DocHit {
    pub path: String,
    pub name: String,
    pub part: String,
    pub score: f32,
    pub keyword_count: usize,
    pub snippet: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DocSort {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    Newest,
    Oldest,
    Frequency,
}

impl DocSort {
    pub fn from_id(id: &str) -> Self {
        match id {
            "name_asc" => Self::NameAsc,
            "name_desc" => Self::NameDesc,
            "newest" => Self::Newest,
            "oldest" => Self::Oldest,
            "frequency" => Self::Frequency,
            _ => Self::Relevance,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::NameAsc => "name_asc",
            Self::NameDesc => "name_desc",
            Self::Newest => "newest",
            Self::Oldest => "oldest",
            Self::Frequency => "frequency",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Relevance => "相关度",
            Self::NameAsc => "文件名 ↑",
            Self::NameDesc => "文件名 ↓",
            Self::Newest => "最新在前",
            Self::Oldest => "最早在前",
            Self::Frequency => "词频优先",
        }
    }

    pub fn apply(self, hits: &mut [DocHit]) {
        match self {
            Self::Relevance => hits.sort_by(|a, b| b.score.total_cmp(&a.score)),
            Self::Frequency => hits.sort_by(|a, b| {
                b.keyword_count
                    .cmp(&a.keyword_count)
                    .then_with(|| b.score.total_cmp(&a.score))
            }),
            Self::NameAsc => hits.sort_by_cached_key(|hit| hit.name.to_lowercase()),
            Self::NameDesc => {
                hits.sort_by_cached_key(|hit| hit.name.to_lowercase());
                hits.reverse();
            }
            Self::Newest => hits.sort_by(|a, b| b.modified.cmp(&a.modified)),
            Self::Oldest => hits.sort_by(|a, b| a.modified.cmp(&b.modified)),
        }
    }
}

pub struct DocStore {
    index: Index,
    reader: IndexReader,
    schema: Schema,
    path_f: Field,
    name_f: Field,
    part_f: Field,
    content_f: Field,
    size_f: Field,
    modified_f: Field,
}

#[derive(Clone, Debug)]
pub struct DocQueryOptions {
    pub all_terms: bool,
    pub limit: usize,
}

impl Default for DocQueryOptions {
    fn default() -> Self {
        Self { all_terms: true, limit: 200 }
    }
}

fn schema() -> (Schema, SchemaFields) {
    let mut b = Schema::builder();
    let path = b.add_text_field("path", STRING | STORED);
    let text_index = TextFieldIndexing::default()
        .set_tokenizer(TOKENIZER)
        .set_index_option(IndexRecordOption::WithFreqsAndPositions);
    let text_opts = TextOptions::default().set_indexing_options(text_index.clone()).set_stored();
    let name = b.add_text_field("name", text_opts.clone());
    let content = b.add_text_field("content", text_opts);
    let part = b.add_text_field("part", STORED);
    let size = b.add_u64_field("size", STORED);
    let modified = b.add_u64_field("modified", STORED);
    (
        b.build(),
        SchemaFields { path, name, part, content, size, modified },
    )
}

struct SchemaFields {
    path: Field,
    name: Field,
    part: Field,
    content: Field,
    size: Field,
    modified: Field,
}

impl DocStore {
    pub fn open(base_dir: &Path) -> Result<Self, String> {
        let dir = base_dir.join(INDEX_DIR);
        let index = Index::open_in_dir(&dir).map_err(|e| format!("打开文档索引失败: {e}"))?;
        let schema = index.schema();
        let fields = schema_fields(&schema)?;
        let reader = index
            .reader()
            .map_err(|e| format!("读取文档索引失败: {e}"))?;
        Ok(Self {
            index,
            reader,
            schema,
            path_f: fields.path,
            name_f: fields.name,
            part_f: fields.part,
            content_f: fields.content,
            size_f: fields.size,
            modified_f: fields.modified,
        })
    }

    pub fn doc_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    pub fn search(&self, raw: &str, options: &DocQueryOptions) -> Result<Vec<DocHit>, String> {
        let query = build_query(raw, options.all_terms, self.content_f, self.name_f);
        let searcher = self.reader.searcher();
        let limit = options.limit.max(1);
        let hits = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| format!("检索失败: {e}"))?;
        let terms = query_tokens(raw);
        let frequency_terms = frequency_terms(raw);
        let mut out = Vec::with_capacity(hits.len());
        for (score, addr) in hits {
            let doc: TantivyDocument = match searcher.doc(addr) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let path = doc
                .get_first(self.path_f)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = doc
                .get_first(self.name_f)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let part = doc
                .get_first(self.part_f)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let content = doc
                .get_first(self.content_f)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let size = doc.get_first(self.size_f).and_then(|v| v.as_u64()).unwrap_or(0);
            let modified = doc
                .get_first(self.modified_f)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let lower = content.to_lowercase();
            let keyword_count = frequency_terms
                .iter()
                .map(|term| count_term(&lower, term))
                .sum();
            out.push(DocHit {
                path,
                name,
                part,
                score,
                keyword_count,
                snippet: make_snippet_with_lower(content, &lower, &terms, 220),
                size,
                modified,
            });
        }
        let _ = &self.schema;
        Ok(out)
    }
}

fn schema_fields(schema: &Schema) -> Result<SchemaFields, String> {
    Ok(SchemaFields {
        path: schema.get_field("path").map_err(|_| "索引缺少 path 字段")?,
        name: schema.get_field("name").map_err(|_| "索引缺少 name 字段")?,
        part: schema.get_field("part").map_err(|_| "索引缺少 part 字段")?,
        content: schema.get_field("content").map_err(|_| "索引缺少 content 字段")?,
        size: schema.get_field("size").map_err(|_| "索引缺少 size 字段")?,
        modified: schema.get_field("modified").map_err(|_| "索引缺少 modified 字段")?,
    })
}

pub fn build_index(
    base_dir: &Path,
    root: &Path,
    mode: BuildMode,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64, &Path),
    log: &mut dyn FnMut(String),
) -> Result<BuildStats, String> {
    std::fs::create_dir_all(base_dir).map_err(|e| e.to_string())?;
    if !root.is_dir() {
        return Err(format!("索引目录不存在: {}", root.display()));
    }
    let index_dir = base_dir.join(INDEX_DIR);
    let rebuild = mode == BuildMode::Rebuild || !index_dir.join("meta.json").exists();
    let (index, schema, f) = if rebuild {
        if index_dir.exists() {
            let _ = std::fs::remove_dir_all(&index_dir);
        }
        std::fs::create_dir_all(&index_dir).map_err(|e| format!("创建索引目录失败: {e}"))?;
        let (schema, _) = schema();
        let index = Index::create_in_dir(&index_dir, schema.clone())
            .map_err(|e| format!("创建文档索引失败: {e}"))?;
        let f = schema_fields(&schema)?;
        (index, schema, f)
    } else {
        let index = Index::open_in_dir(&index_dir).map_err(|e| format!("打开文档索引失败: {e}"))?;
        let schema = index.schema();
        let f = schema_fields(&schema)?;
        (index, schema, f)
    };
    index.tokenizers().register(
        TOKENIZER,
        TextAnalyzer::builder(JiebaTokenizer)
            .filter(LowerCaser)
            .build(),
    );

    let conn = open_meta(base_dir)?;
    if rebuild {
        conn.execute("DELETE FROM files", [])
            .map_err(|e| format!("清空索引元数据失败: {e}"))?;
    }
    conn.execute_batch("DROP TABLE IF EXISTS temp.seen; CREATE TEMP TABLE seen(path TEXT PRIMARY KEY) WITHOUT ROWID;")
        .map_err(|e| format!("准备索引元数据失败: {e}"))?;

    let mut writer = index
        .writer_with_num_threads(1, 40_000_000)
        .map_err(|e| format!("创建索引写入器失败: {e}"))?;
    if rebuild {
        writer.delete_all_documents().map_err(|e| e.to_string())?;
    }

    let mut stats = BuildStats::default();
    let mut pending_meta: Vec<(String, FileMeta)> = Vec::new();
    let mut pending_docs = 0usize;
    let mut seen_stmt = conn
        .prepare("INSERT OR IGNORE INTO seen(path) VALUES (?1)")
        .map_err(|e| format!("准备索引元数据失败: {e}"))?;
    let mut meta_stmt = conn
        .prepare("SELECT size, mtime FROM files WHERE path = ?1")
        .map_err(|e| format!("准备索引元数据失败: {e}"))?;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if SKIP_DIRS.contains(&name.as_str()) {
                return false;
            }
            !util::is_hidden_path(e.path())
        });

    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            stats.cancelled = true;
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !supported_extension(&ext) {
            continue;
        }
        stats.scanned += 1;
        let path_str = path.display().to_string();
        if let Err(e) = seen_stmt.execute(params![&path_str]) {
            log(format!("记录扫描状态失败 {}: {e}", path.display()));
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                stats.skipped += 1;
                continue;
            }
        };
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .map(util::system_time_secs)
            .unwrap_or(0);
        let existing = meta_stmt
            .query_row(params![&path_str], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .unwrap_or(None);
        if let Some((old_size, old_mtime)) = existing {
            if old_size as u64 == size && old_mtime == mtime {
                stats.skipped += 1;
                progress(stats.scanned, stats.indexed_files, path);
                continue;
            }
        }
        let units = match extract_file(path) {
            Ok(u) => u,
            Err(e) => {
                stats.errors += 1;
                log(format!("{}: {e}", path.display()));
                continue;
            }
        };
        writer
            .delete_term(Term::from_field_text(f.path, &path_str));
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for unit in &units {
            let content = unit.text.as_str();
            let mut doc = TantivyDocument::default();
            doc.add_field_value(f.path, path_str.as_str());
            doc.add_field_value(f.name, name.as_str());
            doc.add_field_value(f.part, unit.label.as_str());
            doc.add_field_value(f.content, content);
            doc.add_field_value(f.size, size);
            doc.add_field_value(f.modified, mtime as u64);
            if let Err(e) = writer.add_document(doc) {
                stats.errors += 1;
                log(format!("写入索引失败 {}: {e}", path.display()));
                break;
            }
            stats.units += 1;
        }
        pending_meta.push((
            path_str,
            FileMeta { size, mtime, units: units.len() as u32 },
        ));
        stats.indexed_files += 1;
        pending_docs += units.len();
        if pending_docs >= 128 {
            if let Err(e) = writer.commit() {
                log(format!("提交索引失败: {e}"));
            }
            if let Err(e) = flush_meta(&conn, &mut pending_meta) {
                log(e);
            }
            pending_docs = 0;
        }
        progress(stats.scanned, stats.indexed_files, path);
    }

    if let Err(e) = writer.commit() {
        log(format!("提交索引失败: {e}"));
    }
    let _ = index.load_metas();
    if let Err(e) = flush_meta(&conn, &mut pending_meta) {
        log(e);
    }
    if !stats.cancelled {
        let stale = {
            match conn.prepare("SELECT path FROM files WHERE path NOT IN (SELECT path FROM seen)") {
                Ok(mut stmt) => stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map(|rows| rows.flatten().collect::<Vec<_>>())
                    .unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        };
        if !stale.is_empty() {
            let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
            for key in &stale {
                writer.delete_term(Term::from_field_text(f.path, key));
                let _ = tx.execute("DELETE FROM files WHERE path = ?1", params![key]);
                stats.removed += 1;
            }
            tx.commit().map_err(|e| e.to_string())?;
            let _ = writer.commit();
        }
    }
    drop(seen_stmt);
    drop(meta_stmt);
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    if !stats.cancelled {
        let legacy = base_dir.join("meta");
        if legacy.is_dir() {
            let _ = std::fs::remove_dir_all(&legacy);
        }
    }
    let _ = schema;
    Ok(stats)
}

fn open_meta(base_dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(base_dir).map_err(|e| e.to_string())?;
    let conn =
        Connection::open(base_dir.join(META_DB)).map_err(|e| format!("打开索引元数据失败: {e}"))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| format!("配置索引元数据失败: {e}"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("配置索引元数据失败: {e}"))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("配置索引元数据失败: {e}"))?;
    conn.pragma_update(None, "cache_size", -8192i64)
        .map_err(|e| format!("配置索引元数据失败: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files(
            path TEXT PRIMARY KEY,
            size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            units INTEGER NOT NULL
        ) WITHOUT ROWID;",
    )
    .map_err(|e| format!("创建索引元数据表失败: {e}"))?;
    Ok(conn)
}

fn flush_meta(conn: &Connection, pending: &mut Vec<(String, FileMeta)>) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("写入索引元数据失败: {e}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO files(path, size, mtime, units) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET
                    size = excluded.size,
                    mtime = excluded.mtime,
                    units = excluded.units",
            )
            .map_err(|e| format!("写入索引元数据失败: {e}"))?;
        for (path, meta) in pending.iter() {
            stmt.execute(params![path, meta.size as i64, meta.mtime, meta.units])
                .map_err(|e| format!("写入索引元数据失败: {e}"))?;
        }
    }
    tx.commit()
        .map_err(|e| format!("写入索引元数据失败: {e}"))?;
    pending.clear();
    Ok(())
}

fn jieba() -> &'static jieba_rs::Jieba {
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    JIEBA.get_or_init(jieba_rs::Jieba::new)
}

fn is_word(token: &str) -> bool {
    token.chars().any(|c| c.is_alphanumeric())
}

fn query_tokens(text: &str) -> Vec<String> {
    jieba()
        .cut(text, false)
        .into_iter()
        .filter(|t| is_word(t))
        .map(|t| t.to_lowercase())
        .collect()
}

/// Positive query terms only, deduplicated; used for occurrence counting.
fn frequency_terms(raw: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for (i, segment) in raw.split('"').enumerate() {
        if i % 2 == 1 {
            terms.extend(query_tokens(segment));
        } else {
            for part in segment.split_whitespace() {
                if part.starts_with('-') {
                    continue;
                }
                terms.extend(query_tokens(part));
            }
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

/// Count non-overlapping occurrences; ASCII terms respect word boundaries.
fn count_term(lower: &str, term: &str) -> usize {
    if term.is_empty() {
        return 0;
    }
    let ascii_word = term.chars().all(|c| c.is_ascii_alphanumeric());
    let mut count = 0;
    let mut from = 0;
    while from < lower.len() {
        let Some(pos) = lower[from..].find(term) else {
            break;
        };
        let start = from + pos;
        let end = start + term.len();
        let boundary_ok = if ascii_word {
            let before = lower[..start].chars().next_back();
            let after = lower[end..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
        } else {
            true
        };
        if boundary_ok {
            count += 1;
        }
        from = start + lower[start..].chars().next().map_or(1, char::len_utf8);
    }
    count
}

#[derive(Clone)]
struct JiebaTokenizer;

struct OwnedTokenStream {
    tokens: Vec<Token>,
    index: usize,
    current: Token,
}

impl TokenStream for OwnedTokenStream {
    fn advance(&mut self) -> bool {
        if self.index >= self.tokens.len() {
            return false;
        }
        self.current = self.tokens[self.index].clone();
        self.index += 1;
        true
    }

    fn token(&self) -> &Token {
        &self.current
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.current
    }
}

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = OwnedTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let mut tokens = Vec::new();
        let mut position = 0usize;
        let mut search_from = 0usize;
        for word in jieba().cut(text, false) {
            if !is_word(word) {
                continue;
            }
            let offset = text[search_from..]
                .find(word)
                .map(|i| search_from + i)
                .unwrap_or(search_from);
            let offset_to = offset + word.len();
            search_from = offset_to;
            tokens.push(Token {
                offset_from: offset,
                offset_to,
                position,
                text: word.to_string(),
                position_length: 1,
            });
            position += 1;
        }
        OwnedTokenStream {
            tokens,
            index: 0,
            current: Token::default(),
        }
    }
}

fn field_group(token: &str, content: Field, name: Field) -> Box<dyn Query> {
    let content_q = TermQuery::new(
        Term::from_field_text(content, token),
        IndexRecordOption::WithFreqs,
    );
    let name_q = TermQuery::new(
        Term::from_field_text(name, token),
        IndexRecordOption::WithFreqs,
    );
    Box::new(BooleanQuery::new(vec![
        (Occur::Should, Box::new(BoostQuery::new(Box::new(content_q), 1.0))),
        (Occur::Should, Box::new(BoostQuery::new(Box::new(name_q), 2.4))),
    ]))
}

fn phrase_group(words: &[String], content: Field, name: Field) -> Option<Box<dyn Query>> {
    if words.is_empty() {
        return None;
    }
    if words.len() == 1 {
        return Some(field_group(&words[0], content, name));
    }
    let c_terms = words
        .iter()
        .map(|w| Term::from_field_text(content, w))
        .collect();
    let n_terms = words
        .iter()
        .map(|w| Term::from_field_text(name, w))
        .collect();
    Some(Box::new(BooleanQuery::new(vec![
        (Occur::Should, Box::new(PhraseQuery::new(c_terms))),
        (Occur::Should, Box::new(PhraseQuery::new(n_terms))),
    ])))
}

fn build_query(raw: &str, all_terms: bool, content: Field, name: Field) -> Box<dyn Query> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Box::new(AllQuery);
    }
    let mut positives: Vec<Box<dyn Query>> = Vec::new();
    let mut negatives: Vec<Box<dyn Query>> = Vec::new();
    for (i, segment) in raw.split('"').enumerate() {
        if segment.trim().is_empty() {
            continue;
        }
        if i % 2 == 1 {
            let words = query_tokens(segment);
            if let Some(q) = phrase_group(&words, content, name) {
                positives.push(q);
            }
            continue;
        }
        for part in segment.split_whitespace() {
            let (negative, part) = if let Some(rest) = part.strip_prefix('-') {
                (true, rest)
            } else {
                (false, part)
            };
            if part.is_empty() {
                continue;
            }
            for token in query_tokens(part) {
                let q = field_group(&token, content, name);
                if negative {
                    negatives.push(q);
                } else {
                    positives.push(q);
                }
            }
        }
    }
    if positives.is_empty() && negatives.is_empty() {
        return Box::new(AllQuery);
    }
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    if positives.is_empty() {
        clauses.push((Occur::Must, Box::new(AllQuery)));
    } else {
        let occur = if all_terms { Occur::Must } else { Occur::Should };
        for q in positives {
            clauses.push((occur, q));
        }
    }
    for q in negatives {
        clauses.push((Occur::MustNot, q));
    }
    Box::new(BooleanQuery::new(clauses))
}

fn make_snippet(content: &str, terms: &[String], max_chars: usize) -> String {
    let lower = content.to_lowercase();
    make_snippet_with_lower(content, &lower, terms, max_chars)
}

/// Builds a snippet from a pre-lowercased copy so large documents are not
/// duplicated (or collapsed) in memory for every hit.
fn make_snippet_with_lower(
    content: &str,
    lower: &str,
    terms: &[String],
    max_chars: usize,
) -> String {
    if content.trim().is_empty() {
        return String::new();
    }
    let mut best: Option<usize> = None;
    for term in terms {
        if term.is_empty() {
            continue;
        }
        if let Some(byte_pos) = lower.find(term) {
            best = Some(best.map_or(byte_pos, |b| b.min(byte_pos)));
        }
    }
    let mut start_byte = best.unwrap_or(0).min(content.len());
    while start_byte > 0 && !content.is_char_boundary(start_byte) {
        start_byte -= 1;
    }
    let total_chars = content.chars().count();
    let start_char = content[..start_byte]
        .chars()
        .count()
        .saturating_sub(40);
    let taken = total_chars.saturating_sub(start_char).min(max_chars);
    let raw: String = content.chars().skip(start_char).take(taken).collect();
    let mut snippet = String::with_capacity(raw.len());
    let mut last_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !last_space {
                snippet.push(' ');
                last_space = true;
            }
        } else {
            snippet.push(ch);
            last_space = false;
        }
    }
    if start_char > 0 {
        snippet.insert(0, '…');
    }
    if start_char + taken < total_chars {
        snippet.push('…');
    }
    snippet
}

pub fn list_indexed(base_dir: &Path) -> Vec<(String, FileMeta)> {
    let Ok(conn) = Connection::open(base_dir.join(META_DB)) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare("SELECT path, size, mtime, units FROM files ORDER BY path") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            FileMeta {
                size: row.get::<_, i64>(1)? as u64,
                mtime: row.get::<_, i64>(2)?,
                units: row.get::<_, i64>(3)? as u32,
            },
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

pub fn db_size_bytes(base_dir: &Path) -> u64 {
    fn dir_size(dir: &Path) -> u64 {
        let mut total = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    total += dir_size(&path);
                } else if let Ok(m) = entry.metadata() {
                    total += m.len();
                }
            }
        }
        total
    }
    dir_size(base_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn builds_searches_and_updates_index() {
        let root = std::env::temp_dir().join(format!("fts_docs_root_{}", std::process::id()));
        let db = std::env::temp_dir().join(format!("fts_docs_db_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&db);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("项目说明.md"), "# 文件搜索工具\n这是一个中文文档内容，支持全文检索。\n").unwrap();
        fs::write(root.join("sub/rust.txt"), "Rust ownership and borrowing notes.\n").unwrap();
        fs::write(root.join("sub/ignore.bin"), [0u8, 1, 2]).unwrap();

        let cancel = AtomicBool::new(false);
        let stats = build_index(&db, &root, BuildMode::Rebuild, &cancel, &mut |_, _, _| {}, &mut |_| {})
            .unwrap();
        assert_eq!(stats.indexed_files, 2);
        assert!(stats.units >= 2);

        let store = DocStore::open(&db).unwrap();
        let hits = store
            .search("中文全文", &DocQueryOptions { all_terms: false, limit: 10 })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].path.contains("项目说明"));
        assert!(hits[0].snippet.contains("中文"));

        let hits = store.search("ownership", &DocQueryOptions::default()).unwrap();
        assert_eq!(hits.len(), 1);

        fs::write(root.join("项目说明.md"), "# 已更新\n现在内容变成了量子计算与文件工具。\n").unwrap();
        fs::remove_file(root.join("sub/rust.txt")).unwrap();
        let stats = build_index(&db, &root, BuildMode::Update, &cancel, &mut |_, _, _| {}, &mut |_| {})
            .unwrap();
        assert_eq!(stats.indexed_files, 1);
        assert_eq!(stats.removed, 1);
        let store = DocStore::open(&db).unwrap();
        assert!(store.search("量子计算", &DocQueryOptions::default()).unwrap().len() == 1);
        assert!(store.search("ownership", &DocQueryOptions::default()).unwrap().is_empty());
        drop(store);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&db);
    }

    #[test]
    fn snippets_and_tokens_work() {
        assert_eq!(query_tokens("文件搜索 and Rust"), vec!["文件", "搜索", "and", "rust"]);
        let s = make_snippet("前面很多文字 hello world 后面还有更多内容", &["hello".into()], 40);
        assert!(s.contains("hello"));
    }

    #[test]
    fn keyword_count_respects_word_boundaries() {
        assert_eq!(count_term("and sand and", "and"), 2);
        assert_eq!(count_term("文件搜索，文件搜索", "文件"), 2);
        assert_eq!(count_term(&"Rust rust RUST".to_lowercase(), "rust"), 3);
        assert_eq!(frequency_terms("项目 计划 -草稿 \"年度 报告\""), vec!["年度", "报告", "计划", "项目"]);
    }

    fn hit(name: &str, keyword_count: usize, score: f32, modified: u64) -> DocHit {
        DocHit {
            path: name.into(),
            name: name.into(),
            part: String::new(),
            score,
            keyword_count,
            snippet: String::new(),
            size: 0,
            modified,
        }
    }

    #[test]
    fn doc_sort_orders_hits() {
        let mut hits = vec![
            hit("b.txt", 1, 5.0, 100),
            hit("a.txt", 9, 1.0, 300),
            hit("c.txt", 4, 3.0, 200),
        ];
        DocSort::Relevance.apply(&mut hits);
        assert_eq!(hits[0].name, "b.txt");
        DocSort::Frequency.apply(&mut hits);
        assert_eq!(hits[0].name, "a.txt");
        DocSort::Newest.apply(&mut hits);
        assert_eq!(hits[0].name, "a.txt");
        DocSort::Oldest.apply(&mut hits);
        assert_eq!(hits[0].name, "b.txt");
        DocSort::NameAsc.apply(&mut hits);
        assert_eq!(hits[0].name, "a.txt");
        DocSort::NameDesc.apply(&mut hits);
        assert_eq!(hits[0].name, "c.txt");
    }
}
