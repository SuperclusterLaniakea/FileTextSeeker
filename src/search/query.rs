use std::borrow::Cow;
use std::cmp::Ordering;

use chrono::{Datelike, Local, NaiveDate, TimeZone};
use regex::{Regex, RegexBuilder};

use super::index::FileRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortField {
    Name,
    Path,
    Size,
    Type,
    Modified,
    Extension,
}

impl SortField {
    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Path => "path",
            Self::Size => "size",
            Self::Type => "type",
            Self::Modified => "modified",
            Self::Extension => "extension",
        }
    }
    pub fn from_id(id: &str) -> Self {
        match id {
            "path" => Self::Path,
            "size" => Self::Size,
            "type" => Self::Type,
            "modified" => Self::Modified,
            "extension" => Self::Extension,
            _ => Self::Name,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::Path => "路径",
            Self::Size => "大小",
            Self::Type => "类型",
            Self::Modified => "修改时间",
            Self::Extension => "扩展名",
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueryOptions {
    pub match_case: bool,
    pub match_path: bool,
    pub whole_word: bool,
    pub regex_mode: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            match_case: false,
            match_path: false,
            whole_word: false,
            regex_mode: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Compare {
    Eq(u64),
    Gt(u64),
    Ge(u64),
    Lt(u64),
    Le(u64),
    Range(u64, u64),
}

impl Compare {
    fn matches(&self, value: u64) -> bool {
        match *self {
            Self::Eq(v) => value == v,
            Self::Gt(v) => value > v,
            Self::Ge(v) => value >= v,
            Self::Lt(v) => value < v,
            Self::Le(v) => value <= v,
            Self::Range(a, b) => value >= a.min(b) && value <= a.max(b),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DateField {
    Modified,
    Created,
    Accessed,
}

#[derive(Debug)]
enum Term {
    Text {
        needle: String,
        lower_needle: String,
        case_sensitive: bool,
        target_path: bool,
        whole_word: Option<Regex>,
        wildcard: Option<Regex>,
    },
    Size(Compare),
    Date {
        field: DateField,
        from: Option<i64>,
        to: Option<i64>,
    },
    Ext(Vec<String>),
    IsDir(bool),
    NameContains(String),
    PathContains(String),
    StartsWith(String),
    EndsWith(String),
    Empty(bool),
    Len(Compare),
    Depth(Compare),
    RootPrefix(String),
    Always(bool),
}

impl Term {
    fn matches(&self, record: &FileRecord) -> bool {
        match self {
            Self::Text {
                needle,
                lower_needle,
                case_sensitive,
                target_path,
                whole_word,
                wildcard,
            } => {
                let hay: Cow<'_, str> = if *target_path {
                    if *case_sensitive {
                        record.path.to_string_lossy()
                    } else {
                        Cow::Borrowed(record.lower_path.as_str())
                    }
                } else if *case_sensitive {
                    Cow::Borrowed(record.name.as_str())
                } else {
                    Cow::Borrowed(record.lower_name.as_str())
                };
                if let Some(regex) = wildcard {
                    return regex.is_match(&hay);
                }
                if let Some(regex) = whole_word {
                    return regex.is_match(&hay);
                }
                if *case_sensitive {
                    hay.contains(needle)
                } else {
                    hay.contains(lower_needle)
                }
            }
            Self::Size(cmp) => cmp.matches(record.size),
            Self::Date { field, from, to } => {
                let value = match field {
                    DateField::Modified => record.modified,
                    DateField::Created => record.created,
                    DateField::Accessed => record.accessed,
                };
                from.map(|f| value >= f).unwrap_or(true) && to.map(|t| value <= t).unwrap_or(true)
            }
            Self::Ext(exts) => exts.iter().any(|e| e == &record.ext),
            Self::IsDir(want) => record.is_dir == *want,
            Self::NameContains(s) => record.lower_name.contains(s),
            Self::PathContains(s) => record.lower_path.contains(s),
            Self::StartsWith(s) => record.lower_name.starts_with(s),
            Self::EndsWith(s) => record.lower_name.ends_with(s),
            Self::Empty(true) => !record.is_dir && record.size == 0,
            Self::Empty(false) => !record.is_dir && record.size != 0,
            Self::Len(cmp) => cmp.matches(record.name.chars().count() as u64),
            Self::Depth(cmp) => cmp.matches(record.depth as u64),
            Self::RootPrefix(s) => record.lower_path.starts_with(s),
            Self::Always(v) => *v,
        }
    }
}

#[derive(Debug)]
enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Term(Term),
}

impl Expr {
    fn matches(&self, record: &FileRecord) -> bool {
        match self {
            Self::And(items) => items.iter().all(|e| e.matches(record)),
            Self::Or(items) => items.iter().any(|e| e.matches(record)),
            Self::Not(inner) => !inner.matches(record),
            Self::Term(term) => term.matches(record),
        }
    }
}

pub struct QueryProgram {
    raw: String,
    expr: Option<Expr>,
    regex: Option<Regex>,
    regex_path: bool,
}

impl QueryProgram {
    pub fn parse(raw: &str, options: &QueryOptions) -> Result<Self, String> {
        let raw = raw.trim().to_string();
        if raw.is_empty() {
            return Ok(Self { raw, expr: None, regex: None, regex_path: options.match_path });
        }
        if options.regex_mode {
            let regex = RegexBuilder::new(&raw)
                .case_insensitive(!options.match_case)
                .size_limit(2 * 1024 * 1024)
                .build()
                .map_err(|e| format!("正则表达式无效: {e}"))?;
            return Ok(Self { raw, expr: None, regex: Some(regex), regex_path: options.match_path });
        }
        let tokens = tokenize(&raw)?;
        if tokens.is_empty() {
            return Ok(Self { raw, expr: None, regex: None, regex_path: options.match_path });
        }
        let mut parser = Parser { tokens, pos: 0, options };
        let expr = parser.parse_or()?;
        if parser.pos < parser.tokens.len() {
            return Err("查询语法错误：存在未解析的表达式".into());
        }
        Ok(Self { raw, expr: Some(expr), regex: None, regex_path: options.match_path })
    }

    pub fn is_empty(&self) -> bool {
        self.raw.trim().is_empty()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn matches(&self, record: &FileRecord) -> bool {
        if let Some(regex) = &self.regex {
            let hay: Cow<'_, str> = if self.regex_path {
                record.path.to_string_lossy()
            } else {
                Cow::Borrowed(record.name.as_str())
            };
            return regex.is_match(&hay);
        }
        match &self.expr {
            None => true,
            Some(expr) => expr.matches(record),
        }
    }

    pub fn search(
        &self,
        records: &[FileRecord],
        sort: SortField,
        ascending: bool,
        limit: usize,
    ) -> Vec<usize> {
        let mut ids: Vec<usize> = records
            .iter()
            .enumerate()
            .filter(|(_, r)| self.matches(r))
            .map(|(i, _)| i)
            .collect();
        ids.sort_by(|a, b| compare_records(&records[*a], &records[*b], sort));
        if !ascending {
            ids.reverse();
        }
        if limit > 0 && ids.len() > limit {
            ids.truncate(limit);
        }
        ids
    }
}

pub(crate) fn compare_records(a: &FileRecord, b: &FileRecord, sort: SortField) -> Ordering {
    let primary = match sort {
        SortField::Name => a.lower_name.cmp(&b.lower_name),
        SortField::Path => a.lower_path.cmp(&b.lower_path),
        SortField::Size => a.size.cmp(&b.size),
        SortField::Type => a.is_dir.cmp(&b.is_dir).then(a.ext.cmp(&b.ext)),
        SortField::Modified => a.modified.cmp(&b.modified),
        SortField::Extension => a.ext.cmp(&b.ext).then(a.lower_name.cmp(&b.lower_name)),
    };
    primary.then_with(|| a.lower_path.cmp(&b.lower_path))
}

#[derive(Debug, Clone)]
enum Token {
    LParen,
    RParen,
    Or,
    And,
    Not,
    Atom(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut depth = 0i32;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            c if c.is_whitespace() => i += 1,
            '(' => {
                depth += 1;
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("查询语法错误：括号不匹配".into());
                }
                tokens.push(Token::RParen);
                i += 1;
            }
            '|' => {
                tokens.push(Token::Or);
                i += 1;
            }
            '&' => {
                tokens.push(Token::And);
                i += 1;
            }
            '!' => {
                tokens.push(Token::Not);
                i += 1;
            }
            '"' => {
                let mut text = String::new();
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == '"' {
                        closed = true;
                        i += 1;
                        break;
                    }
                    text.push(chars[i]);
                    i += 1;
                }
                if !closed {
                    return Err("查询语法错误：引号未闭合".into());
                }
                tokens.push(Token::Atom(text));
            }
            _ => {
                let start = i;
                let mut text = String::new();
                while i < chars.len() {
                    let c = chars[i];
                    if c == '"' {
                        i += 1;
                        while i < chars.len() && chars[i] != '"' {
                            text.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() {
                            i += 1;
                        }
                        continue;
                    }
                    if c.is_whitespace() || matches!(c, '(' | ')' | '|' | '&' | '!') {
                        break;
                    }
                    text.push(c);
                    i += 1;
                }
                if i == start {
                    i += 1;
                    continue;
                }
                match text.as_str() {
                    "OR" | "or" => tokens.push(Token::Or),
                    "AND" | "and" => tokens.push(Token::And),
                    "NOT" | "not" => tokens.push(Token::Not),
                    _ => tokens.push(Token::Atom(text)),
                }
            }
        }
    }
    if depth != 0 {
        return Err("查询语法错误：括号不匹配".into());
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    options: &'a QueryOptions,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut items = vec![self.parse_and()?];
        while matches!(self.peek(), Some(Token::Or)) {
            self.pos += 1;
            items.push(self.parse_and()?);
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(Expr::Or(items))
        }
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut items = vec![self.parse_unary()?];
        loop {
            match self.peek() {
                Some(Token::And) => {
                    self.pos += 1;
                    items.push(self.parse_unary()?);
                }
                Some(Token::Atom(_)) | Some(Token::Not) | Some(Token::LParen) => {
                    items.push(self.parse_unary()?);
                }
                _ => break,
            }
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(Expr::And(items))
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Token::Not) => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.tokens.get(self.pos).cloned() {
            Some(Token::LParen) => {
                self.pos += 1;
                let expr = self.parse_or()?;
                match self.tokens.get(self.pos) {
                    Some(Token::RParen) => {
                        self.pos += 1;
                        Ok(expr)
                    }
                    _ => Err("查询语法错误：缺少右括号".into()),
                }
            }
            Some(Token::Atom(atom)) => {
                self.pos += 1;
                self.parse_term(&atom).map(Expr::Term)
            }
            _ => Err("查询语法错误：缺少表达式".into()),
        }
    }

    fn parse_term(&self, atom: &str) -> Result<Term, String> {
        let (concept, value) = match atom.split_once(':') {
            Some((concept, value)) => (concept.to_lowercase(), value.trim().to_string()),
            None => (String::new(), atom.trim().to_string()),
        };
        let lower_value = value.to_lowercase();
        match concept.as_str() {
            "size" => Ok(Term::Size(parse_size_compare(&lower_value)?)),
            "dm" | "dc" | "da" => {
                let field = match concept.as_str() {
                    "dc" => DateField::Created,
                    "da" => DateField::Accessed,
                    _ => DateField::Modified,
                };
                let (from, to) = parse_date_filter(&lower_value)?;
                Ok(Term::Date { field, from, to })
            }
            "ext" => {
                let mut exts: Vec<String> = value
                    .split([';', ',', '|'])
                    .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if exts.is_empty() {
                    return Err("ext: 需要提供扩展名".into());
                }
                exts.sort();
                exts.dedup();
                Ok(Term::Ext(exts))
            }
            "type" => match lower_value.as_str() {
                "file" | "files" | "文件" => Ok(Term::IsDir(false)),
                "dir" | "dirs" | "folder" | "folders" | "目录" | "文件夹" => Ok(Term::IsDir(true)),
                _ => Err("type: 只支持 file 或 dir".into()),
            },
            "name" => Ok(Term::NameContains(lower_value)),
            "path" => Ok(Term::PathContains(lower_value)),
            "startwith" | "sw" => Ok(Term::StartsWith(lower_value)),
            "endwith" | "ew" => Ok(Term::EndsWith(lower_value)),
            "len" => Ok(Term::Len(parse_u64_compare(&lower_value, "len")?)),
            "depth" => Ok(Term::Depth(parse_u64_compare(&lower_value, "depth")?)),
            "root" => Ok(Term::RootPrefix(lower_value.trim_end_matches('\\').to_string())),
            "empty" => Ok(Term::Empty(!matches!(lower_value.as_str(), "false" | "0" | "no"))),
            "audio" | "doc" | "pic" | "video" | "zip" | "exe" | "image" | "music" => {
                let macro_name = if concept == "image" { "pic" } else if concept == "music" { "audio" } else { concept.as_str() };
                let exts = macro_extensions(macro_name);
                if exts.is_empty() {
                    Ok(Term::Always(true))
                } else {
                    Ok(Term::Ext(exts))
                }
            }
            "" => self.text_term(&value),
            _ => self.text_term(atom),
        }
    }

    fn text_term(&self, text: &str) -> Result<Term, String> {
        let text = text.trim().to_string();
        let case_sensitive = self.options.match_case;
        let target_path = self.options.match_path;
        let wildcard = if text.contains('*') || text.contains('?') {
            let mut pattern = String::from("(?i)^");
            for ch in text.to_lowercase().chars() {
                match ch {
                    '*' => pattern.push_str(".*"),
                    '?' => pattern.push('.'),
                    c => pattern.push_str(&regex::escape(&c.to_string())),
                }
            }
            pattern.push('$');
            Some(Regex::new(&pattern).map_err(|e| format!("通配符无效: {e}"))?)
        } else {
            None
        };
        let whole_word = if self.options.whole_word && text.chars().all(|c| c.is_ascii()) {
            let escaped = regex::escape(&text);
            let pattern = format!(r"(?i)(^|[^a-zA-Z0-9_]){escaped}([^a-zA-Z0-9_]|$)");
            Regex::new(&pattern).ok()
        } else {
            None
        };
        Ok(Term::Text {
            needle: text.clone(),
            lower_needle: text.to_lowercase(),
            case_sensitive,
            target_path,
            whole_word,
            wildcard,
        })
    }
}

fn macro_extensions(name: &str) -> Vec<String> {
    let list: &[&str] = match name {
        "audio" => &["mp3", "flac", "wav", "m4a", "aac", "ogg", "wma", "ape", "opus", "aiff"],
        "doc" => &["doc", "docx", "xls", "xlsx", "ppt", "pptx", "pdf", "txt", "md", "rtf", "odt", "csv"],
        "pic" => &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "svg", "ico", "heic", "raw", "psd"],
        "video" => &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "3gp"],
        "zip" => &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso", "cab"],
        "exe" => &["exe", "msi", "bat", "cmd", "com", "ps1", "dll", "sys"],
        _ => &[],
    };
    list.iter().map(|s| s.to_string()).collect()
}

fn parse_size_compare(value: &str) -> Result<Compare, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("size: 需要提供大小".into());
    }
    if let Some((a, b)) = value.split_once("..") {
        return Ok(Compare::Range(parse_size(a)?, parse_size(b)?));
    }
    for (prefix, builder) in [
        (">=", 0u8),
        ("<=", 1),
        (">", 2),
        ("<", 3),
        ("=", 4),
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            let n = parse_size(rest)?;
            return Ok(match builder {
                0 => Compare::Ge(n),
                1 => Compare::Le(n),
                2 => Compare::Gt(n),
                3 => Compare::Lt(n),
                _ => Compare::Eq(n),
            });
        }
    }
    Ok(Compare::Eq(parse_size(value)?))
}

fn parse_size(value: &str) -> Result<u64, String> {
    let value = value.trim().trim_end_matches('b');
    let mut digits = String::new();
    let mut unit = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            if !unit.is_empty() {
                return Err(format!("大小格式无效: {value}"));
            }
            digits.push(ch);
        } else if !ch.is_whitespace() {
            unit.push(ch.to_ascii_lowercase());
        }
    }
    let number: f64 = digits.parse().map_err(|_| format!("大小格式无效: {value}"))?;
    let multiplier = match unit.as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return Err(format!("未知大小单位: {unit}")),
    };
    Ok((number * multiplier) as u64)
}

fn parse_u64_compare(value: &str, name: &str) -> Result<Compare, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{name}: 需要提供数字"));
    }
    if let Some((a, b)) = value.split_once("..") {
        return Ok(Compare::Range(
            a.trim().parse().map_err(|_| format!("{name}: 数字无效"))?,
            b.trim().parse().map_err(|_| format!("{name}: 数字无效"))?,
        ));
    }
    for (prefix, kind) in [(">=", 0u8), ("<=", 1), (">", 2), ("<", 3), ("=", 4)] {
        if let Some(rest) = value.strip_prefix(prefix) {
            let n: u64 = rest.trim().parse().map_err(|_| format!("{name}: 数字无效"))?;
            return Ok(match kind {
                0 => Compare::Ge(n),
                1 => Compare::Le(n),
                2 => Compare::Gt(n),
                3 => Compare::Lt(n),
                _ => Compare::Eq(n),
            });
        }
    }
    Ok(Compare::Eq(value.parse().map_err(|_| format!("{name}: 数字无效"))?))
}

fn day_start(naive: NaiveDate) -> Option<i64> {
    Local
        .from_local_datetime(&naive.and_hms_opt(0, 0, 0)?)
        .single()
        .map(|dt| dt.timestamp())
}

fn day_end(naive: NaiveDate) -> Option<i64> {
    Local
        .from_local_datetime(&naive.and_hms_opt(23, 59, 59)?)
        .single()
        .map(|dt| dt.timestamp())
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%Y.%m.%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(value, fmt) {
            return Some(date);
        }
    }
    None
}

fn parse_date_filter(value: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("日期过滤需要参数".into());
    }
    let now = Local::now();
    let today = now.date_naive();
    let today_start = day_start(today).unwrap_or(0);
    let today_end = day_end(today).unwrap_or(i64::MAX);
    let result = match value {
        "today" | "今天" => (Some(today_start), Some(today_end)),
        "yesterday" | "昨天" => {
            let d = today.pred_opt().unwrap_or(today);
            (day_start(d), day_end(d))
        }
        "thisweek" | "本周" => {
            let offset = today.weekday().num_days_from_monday() as i64;
            let start = today - chrono::Duration::days(offset);
            (day_start(start), Some(today_end))
        }
        "last7days" | "最近7天" | "last7" => {
            let start = today - chrono::Duration::days(6);
            (day_start(start), Some(today_end))
        }
        "thismonth" | "本月" => {
            let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
            (day_start(start), Some(today_end))
        }
        "thisyear" | "今年" => {
            let start = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
            (day_start(start), Some(today_end))
        }
        _ => {
            if let Some((a, b)) = value.split_once("..") {
                let from = if a.trim().is_empty() {
                    None
                } else {
                    parse_date(a.trim()).and_then(day_start)
                };
                let to = if b.trim().is_empty() {
                    None
                } else {
                    parse_date(b.trim()).and_then(day_end)
                };
                if from.is_none() && to.is_none() {
                    return Err(format!("无法识别日期: {value}"));
                }
                (from, to)
            } else if let Some(rest) = value.strip_prefix(">=") {
                (parse_date(rest.trim()).and_then(day_start), None)
            } else if let Some(rest) = value.strip_prefix("<=") {
                (None, parse_date(rest.trim()).and_then(day_end))
            } else if let Some(rest) = value.strip_prefix('>') {
                let d = parse_date(rest.trim()).and_then(day_end).ok_or_else(|| format!("无法识别日期: {}", rest.trim()))?;
                (Some(d + 1), None)
            } else if let Some(rest) = value.strip_prefix('<') {
                let d = parse_date(rest.trim()).and_then(day_start).ok_or_else(|| format!("无法识别日期: {}", rest.trim()))?;
                (None, Some(d - 1))
            } else if let Some(d) = parse_date(value) {
                (day_start(d), day_end(d))
            } else {
                return Err(format!("无法识别日期: {value}"));
            }
        }
    };
    Ok(result)
}

pub fn macros_help() -> &'static str {
    "audio: doc: pic: video: zip: exe: · size:>100mb · dm:today · ext:rs;toml"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn record(name: &str, size: u64, modified: i64, is_dir: bool) -> FileRecord {
        let path = PathBuf::from(format!("C:/demo/{name}"));
        FileRecord {
            path: path.clone(),
            name: name.to_string(),
            ext: if is_dir {
                String::new()
            } else {
                name.rsplit('.').next().unwrap_or("").to_lowercase()
            },
            lower_name: name.to_lowercase(),
            lower_path: path.to_string_lossy().to_lowercase(),
            size,
            modified,
            created: modified,
            accessed: modified,
            is_dir,
            depth: 2,
            attributes: 0,
        }
    }

    fn opts() -> QueryOptions {
        QueryOptions::default()
    }

    #[test]
    fn substring_and_or_not() {
        let records = vec![
            record("main.rs", 12, util_now(), false),
            record("lib.rs", 34, util_now(), false),
            record("notes.txt", 1, util_now(), false),
        ];
        let q = QueryProgram::parse("rs !lib", &opts()).unwrap();
        let hits = q.search(&records, SortField::Name, true, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(records[hits[0]].name, "main.rs");

        let q = QueryProgram::parse("main | notes", &opts()).unwrap();
        assert_eq!(q.search(&records, SortField::Name, true, 10).len(), 2);
    }

    #[test]
    fn wildcard_and_size_and_ext() {
        let records = vec![
            record("report-2024.pdf", 2 * 1024 * 1024, util_now(), false),
            record("report-2023.pdf", 10, util_now(), false),
            record("image.PNG", 5 * 1024 * 1024, util_now(), false),
        ];
        let q = QueryProgram::parse("*.pdf size:>1mb", &opts()).unwrap();
        let hits = q.search(&records, SortField::Name, true, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(records[hits[0]].name, "report-2024.pdf");

        let q = QueryProgram::parse("pic: size:>1mb", &opts()).unwrap();
        assert_eq!(q.search(&records, SortField::Name, true, 10).len(), 1);

        let q = QueryProgram::parse("ext:pdf;txt", &opts()).unwrap();
        assert_eq!(q.search(&records, SortField::Name, true, 10).len(), 2);
    }

    #[test]
    fn dates_and_quoted_phrases() {
        let now = util_now();
        let records = vec![
            record("holiday photo 2024.jpg", 10, now, false),
            record("holiday old.jpg", 10, now - 86400 * 20, false),
        ];
        let q = QueryProgram::parse("\"holiday photo\" dm:today", &opts()).unwrap();
        let hits = q.search(&records, SortField::Name, true, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(records[hits[0]].name, "holiday photo 2024.jpg");
    }

    #[test]
    fn regex_mode_and_types() {
        let records = vec![
            record("abc123.log", 5, util_now(), false),
            record("abcXYZ.log", 5, util_now(), false),
            record("folder", 0, util_now(), true),
        ];
        let o = QueryOptions { regex_mode: true, ..Default::default() };
        let q = QueryProgram::parse(r"^abc\d+\.log$", &o).unwrap();
        let hits = q.search(&records, SortField::Name, true, 10);
        assert_eq!(hits.len(), 1);

        let q = QueryProgram::parse("type:dir", &opts()).unwrap();
        assert_eq!(q.search(&records, SortField::Name, true, 10).len(), 1);
    }

    #[test]
    fn invalid_queries_are_reported() {
        assert!(QueryProgram::parse("(unclosed", &opts()).is_err());
        assert!(QueryProgram::parse("size:abc", &opts()).is_err());
        assert!(QueryProgram::parse("dm:notadate", &opts()).is_err());
    }

    fn util_now() -> i64 {
        chrono::Local::now().timestamp()
    }
}
