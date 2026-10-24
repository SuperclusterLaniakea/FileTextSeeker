use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaseMode {
    #[default]
    Upper,
    Lower,
    Title,
    Sentence,
    Camel,
    Pascal,
    Snake,
    Kebab,
}

impl CaseMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Upper => "全部大写",
            Self::Lower => "全部小写",
            Self::Title => "首字母大写",
            Self::Sentence => "句首大写",
            Self::Camel => "驼峰 camelCase",
            Self::Pascal => "帕斯卡 PascalCase",
            Self::Snake => "下划线 snake_case",
            Self::Kebab => "连字符 kebab-case",
        }
    }

    pub const ALL: [Self; 8] = [
        Self::Upper,
        Self::Lower,
        Self::Title,
        Self::Sentence,
        Self::Camel,
        Self::Pascal,
        Self::Snake,
        Self::Kebab,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PatternPos {
    #[default]
    Start,
    End,
}

impl PatternPos {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "名称前",
            Self::End => "名称后",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    #[default]
    Modified,
    Created,
    Accessed,
}

impl DateSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "修改时间",
            Self::Created => "创建时间",
            Self::Accessed => "访问时间",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtAction {
    #[default]
    Set,
    Remove,
    Lower,
    Upper,
}

impl ExtAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Set => "改为",
            Self::Remove => "移除扩展名",
            Self::Lower => "扩展名小写",
            Self::Upper => "扩展名大写",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpKind {
    Replace {
        find: String,
        replace: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        case_sensitive: bool,
    },
    Delete {
        find: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        case_sensitive: bool,
    },
    Insert {
        text: String,
        at: usize,
        #[serde(default)]
        from_end: bool,
    },
    Prefix(String),
    Suffix(String),
    Case(CaseMode),
    Trim {
        #[serde(default)]
        collapse_spaces: bool,
    },
    Numbering {
        start: u64,
        step: u64,
        pad: usize,
        separator: String,
        position: PatternPos,
        #[serde(default)]
        reset_per_dir: bool,
    },
    Date {
        source: DateSource,
        format: String,
        separator: String,
        position: PatternPos,
    },
    Extension {
        action: ExtAction,
        value: String,
    },
    Template(String),
}

impl OpKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Replace { .. } => "查找替换",
            Self::Delete { .. } => "删除文本",
            Self::Insert { .. } => "插入文本",
            Self::Prefix(_) => "添加前缀",
            Self::Suffix(_) => "添加后缀",
            Self::Case(_) => "大小写",
            Self::Trim { .. } => "清理空白",
            Self::Numbering { .. } => "编号",
            Self::Date { .. } => "日期时间",
            Self::Extension { .. } => "扩展名",
            Self::Template(_) => "模板",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenameOp {
    pub enabled: bool,
    pub kind: OpKind,
}

impl RenameOp {
    pub fn new(kind: OpKind) -> Self {
        Self { enabled: true, kind }
    }
}

pub use crate::config::{CollisionRule, RenameMode};

#[derive(Clone, Debug)]
pub struct RenameRequest {
    pub files: Vec<PathBuf>,
    pub root: PathBuf,
    pub ops: Vec<RenameOp>,
    pub mode: RenameMode,
    pub target_dir: PathBuf,
    pub collision: CollisionRule,
    pub preserve_structure: bool,
}

#[derive(Clone, Debug)]
pub struct PlanItem {
    pub source: PathBuf,
    pub target: PathBuf,
    pub original_name: String,
    pub new_name: String,
    pub action: PlanAction,
    pub note: String,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanAction {
    Ready,
    Unchanged,
    Skipped,
}

#[derive(Clone, Debug, Default)]
pub struct RenamePlan {
    pub items: Vec<PlanItem>,
    pub errors: Vec<String>,
    pub conflicts: usize,
}

impl RenamePlan {
    pub fn ready(&self) -> impl Iterator<Item = &PlanItem> {
        self.items.iter().filter(|i| i.action == PlanAction::Ready)
    }

    pub fn changed_count(&self) -> usize {
        self.items.iter().filter(|i| i.action != PlanAction::Unchanged).count()
    }
}

fn regex_compile(pattern: &str, case_sensitive: bool) -> Result<regex::Regex, String> {
    let mut builder = regex::RegexBuilder::new(pattern);
    builder.case_insensitive(!case_sensitive);
    builder.build().map_err(|e| format!("正则表达式无效: {e}"))
}

fn strip_punct_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && previous_lower && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous_lower = ch.is_lowercase();
            current.push(ch);
        } else {
            previous_lower = false;
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn apply_case(text: &str, mode: CaseMode) -> String {
    match mode {
        CaseMode::Upper => text.to_uppercase(),
        CaseMode::Lower => text.to_lowercase(),
        CaseMode::Title => text
            .split_whitespace()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        CaseMode::Sentence => {
            let lower = text.to_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        CaseMode::Camel | CaseMode::Pascal | CaseMode::Snake | CaseMode::Kebab => {
            let words = strip_punct_words(text);
            let joined = words
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    if i == 0 && mode == CaseMode::Camel {
                        w.to_lowercase()
                    } else {
                        let mut chars = w.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                            None => String::new(),
                        }
                    }
                })
                .collect::<String>();
            match mode {
                CaseMode::Snake => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_"),
                CaseMode::Kebab => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("-"),
                _ => joined,
            }
        }
    }
}

fn system_time_of(meta: &std::fs::Metadata, source: DateSource) -> Option<std::time::SystemTime> {
    match source {
        DateSource::Modified => meta.modified().ok(),
        DateSource::Created => meta.created().ok(),
        DateSource::Accessed => meta.accessed().ok(),
    }
}

fn render_template(template: &str, original: &str, stem: &str, ext: &str, parent: &str, index: usize) -> String {
    let now = chrono::Local::now();
    template
        .replace("{name}", stem)
        .replace("{original}", original)
        .replace("{ext}", ext)
        .replace("{parent}", parent)
        .replace("{index}", &(index + 1).to_string())
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{time}", &now.format("%H%M%S").to_string())
}

pub fn build_plan(req: &RenameRequest) -> RenamePlan {
    let mut plan = RenamePlan::default();
    if req.files.is_empty() {
        plan.errors.push("请先选择需要处理的文件".into());
        return plan;
    }
    if matches!(req.mode, RenameMode::Copy | RenameMode::Move) && req.target_dir.as_os_str().is_empty() {
        plan.errors.push("请选择目标目录".into());
        return plan;
    }
    let mut used: HashSet<String> = HashSet::new();
    let parent_fallback = req
        .files
        .first()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| req.root.clone());
    let mut per_dir_counters: std::collections::HashMap<PathBuf, u64> = std::collections::HashMap::new();
    let mut items: Vec<PlanItem> = Vec::new();

    for (index, source) in req.files.iter().enumerate() {
        let original = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let (base_stem, base_ext) = util::split_name_ext(&original);
        let mut stem = base_stem.to_string();
        let mut extension = base_ext.to_string();
        let size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
        let mut note = String::new();
        let mut op_error: Option<String> = None;

        for op in req.ops.iter().filter(|o| o.enabled) {
            match &op.kind {
                OpKind::Replace { find, replace, regex, case_sensitive } => {
                    if find.is_empty() {
                        continue;
                    }
                    if *regex {
                        match regex_compile(find, *case_sensitive) {
                            Ok(re) => stem = re.replace_all(&stem, replace.as_str()).into_owned(),
                            Err(e) => {
                                op_error = Some(e);
                                break;
                            }
                        }
                    } else if *case_sensitive {
                        stem = stem.replace(find.as_str(), replace);
                    } else {
                        stem = regex::RegexBuilder::new(&regex::escape(find))
                            .case_insensitive(true)
                            .build()
                            .map(|re| re.replace_all(&stem, replace.as_str()).into_owned())
                            .unwrap_or(stem);
                    }
                }
                OpKind::Delete { find, regex, case_sensitive } => {
                    if find.is_empty() {
                        continue;
                    }
                    if *regex {
                        match regex_compile(find, *case_sensitive) {
                            Ok(re) => stem = re.replace_all(&stem, "").into_owned(),
                            Err(e) => {
                                op_error = Some(e);
                                break;
                            }
                        }
                    } else if *case_sensitive {
                        stem = stem.replace(find.as_str(), "");
                    } else {
                        stem = regex::RegexBuilder::new(&regex::escape(find))
                            .case_insensitive(true)
                            .build()
                            .map(|re| re.replace_all(&stem, "").into_owned())
                            .unwrap_or(stem);
                    }
                }
                OpKind::Insert { text, at, from_end } => {
                    let mut chars: Vec<char> = stem.chars().collect();
                    let index = if *from_end {
                        chars.len().saturating_sub(*at)
                    } else {
                        (*at).min(chars.len())
                    };
                    let insert: Vec<char> = text.chars().collect();
                    chars.splice(index..index, insert);
                    stem = chars.into_iter().collect();
                }
                OpKind::Prefix(text) => stem = format!("{text}{stem}"),
                OpKind::Suffix(text) => stem = format!("{stem}{text}"),
                OpKind::Case(mode) => stem = apply_case(&stem, *mode),
                OpKind::Trim { collapse_spaces } => {
                    let trimmed = stem.trim().to_string();
                    stem = if *collapse_spaces {
                        let mut out = String::new();
                        let mut space = false;
                        for ch in trimmed.chars() {
                            if ch.is_whitespace() {
                                if !space {
                                    out.push(' ');
                                    space = true;
                                }
                            } else {
                                space = false;
                                out.push(ch);
                            }
                        }
                        out
                    } else {
                        trimmed
                    };
                }
                OpKind::Numbering { start, step, pad, separator, position, reset_per_dir } => {
                    let value = if *reset_per_dir {
                        let dir = source.parent().unwrap_or(&parent_fallback).to_path_buf();
                        let entry = per_dir_counters.entry(dir).or_insert(*start);
                        let v = *entry;
                        *entry += *step;
                        v
                    } else {
                        *start + index as u64 * *step
                    };
                    let number = if *pad > 0 {                        format!("{value:0width$}", width = *pad)
                    } else {
                        value.to_string()
                    };
                    let piece = number;
                    stem = match position {
                        PatternPos::Start => format!("{piece}{separator}{stem}"),
                        PatternPos::End => format!("{stem}{separator}{piece}"),
                    };
                }
                OpKind::Date { source: ds, format, separator, position } => {
                    let meta = match std::fs::metadata(source) {
                        Ok(m) => m,
                        Err(e) => {
                            op_error = Some(format!("无法读取时间: {e}"));
                            break;
                        }
                    };
                    match system_time_of(&meta, *ds) {
                        Some(time) => {
                            let dt: chrono::DateTime<chrono::Local> = time.into();
                            let formatted = dt.format(format).to_string();
                            stem = match position {
                                PatternPos::Start => format!("{formatted}{separator}{stem}"),
                                PatternPos::End => format!("{stem}{separator}{formatted}"),
                            };
                        }
                        None => note = "无法获取时间".into(),
                    }
                }
                OpKind::Extension { action, value } => match action {
                    ExtAction::Set => extension = value.trim_start_matches('.').to_string(),
                    ExtAction::Remove => extension.clear(),
                    ExtAction::Lower => extension = extension.to_lowercase(),
                    ExtAction::Upper => extension = extension.to_uppercase(),
                },
                OpKind::Template(template) => {
                    let parent = source
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    stem = render_template(template, &original, &stem, &extension, &parent, index);
                }
            }
        }

        if let Some(error) = op_error {
            plan.errors.push(format!("{original}: {error}"));
            items.push(PlanItem {
                source: source.clone(),
                target: source.clone(),
                original_name: original.clone(),
                new_name: original,
                action: PlanAction::Skipped,
                note: "规则执行失败".into(),
                size,
            });
            continue;
        }

        let new_name = if extension.is_empty() {
            if stem.is_empty() {
                original.clone()
            } else {
                stem.clone()
            }
        } else {
            format!("{stem}.{extension}")
        };
        let unchanged = new_name == original;
        let validation = util::valid_filename(&new_name);
        if unchanged || validation.is_err() {
            let note = if unchanged {
                "名称未变化".to_string()
            } else {
                validation.err().unwrap_or_else(|| "文件名无效".into())
            };
            items.push(PlanItem {
                source: source.clone(),
                target: source.clone(),
                original_name: original.clone(),
                new_name: if unchanged { new_name.clone() } else { original.clone() },
                action: if unchanged { PlanAction::Unchanged } else { PlanAction::Skipped },
                note,
                size,
            });
            continue;
        }

        let target = match req.mode {
            RenameMode::Rename => source.with_file_name(&new_name),
            RenameMode::Copy | RenameMode::Move => {
                let base = if req.preserve_structure {
                    source
                        .strip_prefix(&req.root)
                        .ok()
                        .and_then(|p| p.parent())
                        .map(Path::to_path_buf)
                        .unwrap_or_default()
                } else {
                    PathBuf::new()
                };
                req.target_dir.join(base).join(&new_name)
            }
        };
        items.push(PlanItem {
            source: source.clone(),
            target,
            original_name: original,
            new_name,
            action: PlanAction::Ready,
            note,
            size,
        });
    }

    for item in items.iter_mut() {
        if item.action != PlanAction::Ready {
            continue;
        }
        let mut target = item.target.clone();
        let mut key = target.to_string_lossy().to_lowercase();
        let mut attempt = 0usize;
        let conflicting = |path: &Path, used: &HashSet<String>, req: &RenameRequest, source: &Path| {
            let k = path.to_string_lossy().to_lowercase();
            if used.contains(&k) {
                return true;
            }
            if !path.exists() {
                return false;
            }
            if path == source {
                return false;
            }
            if req.mode == RenameMode::Rename {
                let sources: HashSet<PathBuf> = req.files.iter().cloned().collect();
                if sources.contains(path) {
                    return false;
                }
            }
            true
        };
        if conflicting(&target, &used, req, &item.source) {
            match req.collision {
                CollisionRule::Skip => {
                    item.action = PlanAction::Skipped;
                    item.note = "目标文件已存在".into();
                    plan.conflicts += 1;
                    continue;
                }
                CollisionRule::Overwrite => {
                    plan.conflicts += 1;
                }
                CollisionRule::AutoNumber => {
                    plan.conflicts += 1;
                    loop {
                        attempt += 1;
                        let (stem, ext) = util::split_name_ext(&item.new_name);
                        let candidate = if ext.is_empty() {
                            format!("{stem} ({attempt})")
                        } else {
                            format!("{stem} ({attempt}).{ext}")
                        };
                        let candidate_path = target.with_file_name(&candidate);
                        let candidate_key = candidate_path.to_string_lossy().to_lowercase();
                        if !used.contains(&candidate_key) && !candidate_path.exists() {
                            target = candidate_path;
                            break;
                        }
                        if attempt > 10_000 {
                            item.action = PlanAction::Skipped;
                            item.note = "无法生成不冲突的名称".into();
                            break;
                        }
                    }
                    if item.action == PlanAction::Skipped {
                        continue;
                    }
                    key = target.to_string_lossy().to_lowercase();
                    item.note = format!("自动编号 {}", attempt);
                    item.target = target.clone();
                    item.new_name = target
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| item.new_name.clone());
                }
            }
        }
        if target == item.source && req.mode == RenameMode::Rename {
            item.action = PlanAction::Unchanged;
            item.note = "路径未变化".into();
            continue;
        }
        used.insert(key);
    }

    plan.items = items;
    plan
}

#[derive(Clone, Debug, Default)]
pub struct ExecReport {
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub cancelled: bool,
}

pub fn execute(plan: &RenamePlan, req: &RenameRequest, cancel: &std::sync::atomic::AtomicBool) -> ExecReport {
    let mut report = ExecReport {
        skipped: plan.items.iter().filter(|i| i.action != PlanAction::Ready).count(),
        ..Default::default()
    };
    let items: Vec<&PlanItem> = plan.ready().collect();
    if req.mode == RenameMode::Rename {
        let mut staged: Vec<(&PlanItem, PathBuf)> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                report.cancelled = true;
                break;
            }
            let temp = item.source.with_file_name(format!(
                ".fts-rn-{}-{i}-{}",
                std::process::id(),
                item.source.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            ));
            match std::fs::rename(&item.source, &temp) {
                Ok(()) => staged.push((item, temp)),
                Err(e) => {
                    report.failed += 1;
                    if report.errors.len() < 100 {
                        report.errors.push(format!("{}: {e}", item.source.display()));
                    }
                }
            }
        }
        for (item, temp) in staged {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                report.cancelled = true;
                let _ = std::fs::rename(&temp, &item.source);
                continue;
            }
            if let Some(parent) = item.target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if item.target.exists() && !item.source.exists() {
                let _ = std::fs::remove_file(&item.target);
            }
            match std::fs::rename(&temp, &item.target) {
                Ok(()) => {
                    report.done += 1;
                    report.moves.push((item.source.clone(), item.target.clone()));
                }
                Err(e) => {
                    let _ = std::fs::rename(&temp, &item.source);
                    report.failed += 1;
                    if report.errors.len() < 100 {
                        report.errors.push(format!("{} -> {}: {e}", item.source.display(), item.target.display()));
                    }
                }
            }
        }
        return report;
    }

    for (i, item) in items.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        if let Some(parent) = item.target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push(format!("创建目录 {}: {e}", parent.display()));
                }
                continue;
            }
        }
        if item.target.exists() && item.target != item.source {
            let _ = std::fs::remove_file(&item.target);
        }
        let result = if req.mode == RenameMode::Copy {
            std::fs::copy(&item.source, &item.target).map(|_| ())
        } else {
            std::fs::rename(&item.source, &item.target)
        };
        match result {
            Ok(()) => {
                report.done += 1;
                report.moves.push((item.source.clone(), item.target.clone()));
            }
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    let action = if req.mode == RenameMode::Copy { "复制" } else { "移动" };
                    report.errors.push(format!("{action} {} (第 {i} 项): {e}", item.source.display()));
                }
            }
        }
    }
    report
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UndoLog {
    pub mode: RenameMode,
    pub created_at: String,
    pub moves: Vec<(String, String)>,
}

pub fn save_undo(path: &Path, mode: RenameMode, moves: &[(PathBuf, PathBuf)]) -> std::io::Result<()> {
    let log = UndoLog {
        mode,
        created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        moves: moves
            .iter()
            .map(|(a, b)| (a.display().to_string(), b.display().to_string()))
            .collect(),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&log)?)
}

pub fn load_undo(path: &Path) -> Result<UndoLog, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub fn undo(log: &UndoLog) -> ExecReport {
    let mut report = ExecReport::default();
    for (from, to) in log.moves.iter().rev() {
        let from = PathBuf::from(from);
        let to = PathBuf::from(to);
        if !to.exists() {
            report.failed += 1;
            report.errors.push(format!("目标已不存在: {}", to.display()));
            continue;
        }
        let result = if log.mode == RenameMode::Copy {
            std::fs::remove_file(&to)
        } else {
            if let Some(parent) = from.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::rename(&to, &from)
        };
        match result {
            Ok(()) => report.done += 1,
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push(format!("撤销 {}: {e}", to.display()));
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fts_rename_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn pipeline_builds_expected_names() {
        let root = temp("plan");
        let a = root.join("alpha report.txt");
        let b = root.join("beta report.txt");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "bb").unwrap();
        let req = RenameRequest {
            files: vec![a.clone(), b.clone()],
            root: root.clone(),
            ops: vec![
                RenameOp::new(OpKind::Replace { find: "report".into(), replace: "摘要".into(), regex: false, case_sensitive: false }),
                RenameOp::new(OpKind::Case(CaseMode::Title)),
                RenameOp::new(OpKind::Numbering { start: 1, step: 1, pad: 3, separator: "-".into(), position: PatternPos::Start, reset_per_dir: false }),
                RenameOp::new(OpKind::Extension { action: ExtAction::Set, value: "md".into() }),
            ],
            mode: RenameMode::Rename,
            target_dir: PathBuf::new(),
            collision: CollisionRule::AutoNumber,
            preserve_structure: true,
        };
        let plan = build_plan(&req);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert_eq!(plan.ready().count(), 2);
        assert_eq!(plan.items[0].new_name, "001-Alpha 摘要.md");
        assert_eq!(plan.items[1].new_name, "002-Beta 摘要.md");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn collision_auto_numbers_and_skips() {
        let root = temp("collision");
        let a = root.join("same.txt");
        fs::write(&a, "a").unwrap();
        fs::write(root.join("same (1).txt"), "existing").unwrap();
        let req = RenameRequest {
            files: vec![a.clone()],
            root: root.clone(),
            ops: vec![RenameOp::new(OpKind::Replace {
                find: "same".into(),
                replace: "same (1)".into(),
                regex: false,
                case_sensitive: false,
            })],
            mode: RenameMode::Rename,
            target_dir: PathBuf::new(),
            collision: CollisionRule::AutoNumber,
            preserve_structure: true,
        };
        let plan = build_plan(&req);
        assert_eq!(plan.items[0].action, PlanAction::Ready);
        assert_eq!(plan.items[0].new_name, "same (1) (1).txt");
        let req2 = RenameRequest { collision: CollisionRule::Skip, ..req };
        let plan2 = build_plan(&req2);
        assert_eq!(plan2.items[0].action, PlanAction::Skipped);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_execute_and_undo() {
        let root = temp("exec");
        let a = root.join("one.txt");
        let b = root.join("two.txt");
        fs::write(&a, "1").unwrap();
        fs::write(&b, "2").unwrap();
        let req = RenameRequest {
            files: vec![a.clone(), b.clone()],
            root: root.clone(),
            ops: vec![RenameOp::new(OpKind::Suffix("-new".into()))],
            mode: RenameMode::Rename,
            target_dir: PathBuf::new(),
            collision: CollisionRule::AutoNumber,
            preserve_structure: true,
        };
        let plan = build_plan(&req);
        let report = execute(&plan, &req, &AtomicBool::new(false));
        assert_eq!(report.done, 2, "{:?}", report.errors);
        assert!(root.join("one-new.txt").exists());
        let log_path = root.join("undo.json");
        save_undo(&log_path, RenameMode::Rename, &report.moves).unwrap();
        let log = load_undo(&log_path).unwrap();
        let undo_report = undo(&log);
        assert_eq!(undo_report.done, 2, "{:?}", undo_report.errors);
        assert!(a.exists() && b.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_preserves_structure() {
        let root = temp("copy");
        let source_root = root.join("src");
        let target_root = root.join("dst");
        fs::create_dir_all(source_root.join("sub")).unwrap();
        let a = source_root.join("sub/data.txt");
        fs::write(&a, "hello").unwrap();
        let req = RenameRequest {
            files: vec![a.clone()],
            root: source_root.clone(),
            ops: vec![RenameOp::new(OpKind::Extension { action: ExtAction::Set, value: "bak".into() })],
            mode: RenameMode::Copy,
            target_dir: target_root.clone(),
            collision: CollisionRule::Overwrite,
            preserve_structure: true,
        };
        let plan = build_plan(&req);
        assert_eq!(plan.items[0].target, target_root.join("sub/data.bak"));
        let report = execute(&plan, &req, &AtomicBool::new(false));
        assert_eq!(report.done, 1);
        assert_eq!(fs::read_to_string(target_root.join("sub/data.bak")).unwrap(), "hello");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn regex_replace_and_template() {
        let root = temp("regex");
        let a = root.join("IMG_2024_001.JPG");
        fs::write(&a, "x").unwrap();
        let req = RenameRequest {
            files: vec![a.clone()],
            root: root.clone(),
            ops: vec![
                RenameOp::new(OpKind::Replace { find: r"IMG_(\d+)_(\d+)".into(), replace: "photo-$1-$2".into(), regex: true, case_sensitive: true }),
                RenameOp::new(OpKind::Template("trip-{name}-{parent}".into())),
                RenameOp::new(OpKind::Extension { action: ExtAction::Lower, value: String::new() }),
            ],
            mode: RenameMode::Rename,
            target_dir: PathBuf::new(),
            collision: CollisionRule::AutoNumber,
            preserve_structure: true,
        };
        let plan = build_plan(&req);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        let name = &plan.items[0].new_name;
        assert!(name.starts_with("trip-photo-2024-001-"), "{name}");
        assert!(name.ends_with(".jpg"));
        let _ = fs::remove_dir_all(&root);
    }
}
