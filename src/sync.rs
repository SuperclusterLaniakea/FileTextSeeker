use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::{ConflictChoice, SyncModeChoice};
use crate::util;

#[derive(Clone, Debug)]
pub struct SyncOptions {
    pub left: PathBuf,
    pub right: PathBuf,
    pub mode: SyncModeChoice,
    pub conflict: ConflictChoice,
    pub include_patterns: String,
    pub exclude_patterns: String,
    pub regex_exclude: String,
    pub min_size_kb: u64,
    pub max_size_mb: u64,
    pub exclude_hidden: bool,
    pub buffer_mb: usize,
    pub concurrent: usize,
    pub unbuffered: bool,
    pub verify: bool,
    pub follow_symlinks: bool,
    pub preserve_timestamps: bool,
    pub auto_retry: u8,
    pub dry_run: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            left: PathBuf::new(),
            right: PathBuf::new(),
            mode: SyncModeChoice::Mirror,
            conflict: ConflictChoice::Newer,
            include_patterns: String::new(),
            exclude_patterns: String::new(),
            regex_exclude: String::new(),
            min_size_kb: 0,
            max_size_mb: 0,
            exclude_hidden: false,
            buffer_mb: 2,
            concurrent: 4,
            unbuffered: false,
            verify: false,
            follow_symlinks: false,
            preserve_timestamps: true,
            auto_retry: 0,
            dry_run: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    CopyLeftToRight,
    CopyRightToLeft,
    DeleteRight,
    DeleteLeft,
    Conflict,
}

impl ActionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::CopyLeftToRight => "左侧 → 右侧",
            Self::CopyRightToLeft => "右侧 → 左侧",
            Self::DeleteRight => "删除右侧多余文件",
            Self::DeleteLeft => "删除左侧多余文件",
            Self::Conflict => "冲突",
        }
    }

    pub fn is_copy(self) -> bool {
        matches!(self, Self::CopyLeftToRight | Self::CopyRightToLeft)
    }

    pub fn is_delete(self) -> bool {
        matches!(self, Self::DeleteRight | Self::DeleteLeft)
    }
}

#[derive(Clone, Debug)]
pub struct SyncAction {
    pub rel: String,
    pub source: Option<PathBuf>,
    pub target: PathBuf,
    pub bytes: u64,
    pub kind: ActionKind,
    pub reason: String,
}

#[derive(Clone, Debug, Default)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub left_files: usize,
    pub right_files: usize,
    pub skipped: usize,
    pub conflicts: usize,
}

impl SyncPlan {
    pub fn copy_actions(&self) -> impl Iterator<Item = &SyncAction> {
        self.actions.iter().filter(|a| a.kind.is_copy())
    }

    pub fn delete_actions(&self) -> impl Iterator<Item = &SyncAction> {
        self.actions.iter().filter(|a| a.kind.is_delete())
    }

    pub fn copy_bytes(&self) -> u64 {
        self.copy_actions().map(|a| a.bytes).sum()
    }

    pub fn delete_bytes(&self) -> u64 {
        self.delete_actions().map(|a| a.bytes).sum()
    }
}

#[derive(Clone, Debug)]
pub struct SyncProgress {
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Clone, Debug)]
struct Entry {
    size: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug, Default)]
pub struct SyncReport {
    pub copied: usize,
    pub updated: usize,
    pub deleted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub bytes: u64,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub dry_run: bool,
    pub planned: usize,
}

fn normalized_relative(path: &Path) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

fn wildcard_patterns(raw: &str) -> Vec<glob::Pattern> {
    raw.split([';', ',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| glob::Pattern::new(s).ok())
        .collect()
}

fn is_hidden(path: &Path) -> bool {
    let name_hidden = path
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false);
    if name_hidden {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            return meta.file_attributes() & 0x2 != 0;
        }
    }
    false
}

fn matches_filters(rel: &str, path: &Path, opts: &SyncOptions, includes: &[glob::Pattern], excludes: &[glob::Pattern], regex: Option<&regex::Regex>) -> bool {
    if !includes.is_empty() && !includes.iter().any(|p| p.matches(rel)) {
        return false;
    }
    if excludes.iter().any(|p| p.matches(rel)) {
        return false;
    }
    if let Some(regex) = regex {
        if regex.is_match(rel) {
            return false;
        }
    }
    if opts.exclude_hidden && is_hidden(path) {
        return false;
    }
    true
}

fn scan_side(
    root: &Path,
    opts: &SyncOptions,
    includes: &[glob::Pattern],
    excludes: &[glob::Pattern],
    regex: Option<&regex::Regex>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(SyncProgress),
    errors: &mut Vec<String>,
    stage: &'static str,
) -> std::collections::HashMap<String, Entry> {
    let mut files = std::collections::HashMap::new();
    if !root.exists() {
        return files;
    }
    let mut visited = 0u64;
    for entry in WalkDir::new(root)
        .follow_links(opts.follow_symlinks)
        .into_iter()
        .filter_map(Result::ok)
    {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(rel_path) = path.strip_prefix(root) else { continue };
        let Some(rel) = normalized_relative(rel_path) else { continue };
        if !matches_filters(&rel, path, opts, includes, excludes, regex) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                if errors.len() < 100 {
                    errors.push(format!("{}: {e}", path.display()));
                }
                continue;
            }
        };
        let size = meta.len();
        if opts.min_size_kb > 0 && size < opts.min_size_kb * 1024 {
            continue;
        }
        if opts.max_size_mb > 0 && size > opts.max_size_mb * 1024 * 1024 {
            continue;
        }
        files.insert(
            rel,
            Entry { size, modified: meta.modified().ok() },
        );
        visited += 1;
        if visited % 500 == 0 {
            progress(SyncProgress {
                stage,
                done: visited,
                total: 0,
                message: format!("已扫描 {visited} 个文件"),
            });
        }
    }
    files
}

fn modified_differs(a: Option<SystemTime>, b: Option<SystemTime>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            let da = a.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
            let db = b.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
            let diff = if da > db { da - db } else { db - da };
            diff > Duration::from_secs(2)
        }
        _ => false,
    }
}

fn is_newer(a: Option<SystemTime>, b: Option<SystemTime>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    }
}

fn ensure_distinct(left: &Path, right: &Path) -> Result<(), String> {
    let canon = |p: &Path| util::canonical_or_absolute(p);
    let l = canon(left);
    let r = canon(right);
    if l == r {
        return Err("源目录与目标目录不能相同".into());
    }
    if l.starts_with(&r) || r.starts_with(&l) {
        return Err("源目录与目标目录不能互相包含".into());
    }
    Ok(())
}

pub fn build_plan(opts: &SyncOptions, cancel: &AtomicBool, progress: &mut dyn FnMut(SyncProgress)) -> SyncPlan {
    let mut plan = SyncPlan::default();
    if opts.left.as_os_str().is_empty() || opts.right.as_os_str().is_empty() {
        plan.errors.push("请选择左侧和右侧目录".into());
        return plan;
    }
    if let Err(e) = ensure_distinct(&opts.left, &opts.right) {
        plan.errors.push(e);
        return plan;
    }
    let includes = wildcard_patterns(&opts.include_patterns);
    let excludes = wildcard_patterns(&opts.exclude_patterns);
    let regex = if opts.regex_exclude.trim().is_empty() {
        None
    } else {
        match regex::Regex::new(opts.regex_exclude.trim()) {
            Ok(re) => Some(re),
            Err(e) => {
                plan.errors.push(format!("排除正则无效: {e}"));
                return plan;
            }
        }
    };
    progress(SyncProgress {
        stage: "扫描左侧",
        done: 0,
        total: 0,
        message: opts.left.display().to_string(),
    });
    let left = scan_side(
        &opts.left,
        opts,
        &includes,
        &excludes,
        regex.as_ref(),
        cancel,
        progress,
        &mut plan.errors,
        "扫描左侧",
    );
    progress(SyncProgress {
        stage: "扫描右侧",
        done: 0,
        total: 0,
        message: opts.right.display().to_string(),
    });
    let right = scan_side(
        &opts.right,
        opts,
        &includes,
        &excludes,
        regex.as_ref(),
        cancel,
        progress,
        &mut plan.errors,
        "扫描右侧",
    );
    plan.left_files = left.len();
    plan.right_files = right.len();
    if cancel.load(Ordering::Relaxed) {
        return plan;
    }

    let mut actions: Vec<SyncAction> = Vec::new();
    let diff = |l: &Entry, r: &Entry| l.size != r.size || modified_differs(l.modified, r.modified);

    match opts.mode {
        SyncModeChoice::Mirror => {
            for (rel, l) in &left {
                let target = opts.right.join(rel);
                match right.get(rel) {
                    Some(r) if !diff(l, r) => {}
                    Some(_) => actions.push(SyncAction {
                        rel: rel.clone(),
                        source: Some(opts.left.join(rel)),
                        target,
                        bytes: l.size,
                        kind: ActionKind::CopyLeftToRight,
                        reason: "内容或时间不同，镜像覆盖".into(),
                    }),
                    None => actions.push(SyncAction {
                        rel: rel.clone(),
                        source: Some(opts.left.join(rel)),
                        target,
                        bytes: l.size,
                        kind: ActionKind::CopyLeftToRight,
                        reason: "右侧缺失".into(),
                    }),
                }
            }
            for (rel, r) in &right {
                if !left.contains_key(rel) {
                    actions.push(SyncAction {
                        rel: rel.clone(),
                        source: None,
                        target: opts.right.join(rel),
                        bytes: r.size,
                        kind: ActionKind::DeleteRight,
                        reason: "镜像模式下左侧不存在".into(),
                    });
                }
            }
        }
        SyncModeChoice::Update => {
            for (rel, l) in &left {
                let target = opts.right.join(rel);
                match right.get(rel) {
                    None => actions.push(SyncAction {
                        rel: rel.clone(),
                        source: Some(opts.left.join(rel)),
                        target,
                        bytes: l.size,
                        kind: ActionKind::CopyLeftToRight,
                        reason: "右侧缺失".into(),
                    }),
                    Some(r) => {
                        if l.size != r.size {
                            actions.push(SyncAction {
                                rel: rel.clone(),
                                source: Some(opts.left.join(rel)),
                                target,
                                bytes: l.size,
                                kind: ActionKind::CopyLeftToRight,
                                reason: "大小不同".into(),
                            });
                        } else if is_newer(l.modified, r.modified) {
                            actions.push(SyncAction {
                                rel: rel.clone(),
                                source: Some(opts.left.join(rel)),
                                target,
                                bytes: l.size,
                                kind: ActionKind::CopyLeftToRight,
                                reason: "左侧较新".into(),
                            });
                        } else {
                            plan.skipped += 1;
                        }
                    }
                }
            }
        }
        SyncModeChoice::Twoway | SyncModeChoice::Custom => {
            let custom = opts.mode == SyncModeChoice::Custom;
            for (rel, l) in &left {
                let l_path = opts.left.join(rel);
                let target = opts.right.join(rel);
                match right.get(rel) {
                    None => actions.push(SyncAction {
                        rel: rel.clone(),
                        source: Some(l_path),
                        target,
                        bytes: l.size,
                        kind: ActionKind::CopyLeftToRight,
                        reason: "右侧缺失".into(),
                    }),
                    Some(r) if diff(l, r) => {
                        let decision = if custom {
                            ConflictChoice::Newer
                        } else {
                            opts.conflict
                        };
                        match decision {
                            ConflictChoice::Newer => {
                                if is_newer(l.modified, r.modified) {
                                    actions.push(SyncAction {
                                        rel: rel.clone(),
                                        source: Some(l_path),
                                        target,
                                        bytes: l.size,
                                        kind: ActionKind::CopyLeftToRight,
                                        reason: "冲突：左侧较新".into(),
                                    });
                                } else if is_newer(r.modified, l.modified) {
                                    actions.push(SyncAction {
                                        rel: rel.clone(),
                                        source: Some(opts.right.join(rel)),
                                        target: l_path,
                                        bytes: r.size,
                                        kind: ActionKind::CopyRightToLeft,
                                        reason: "冲突：右侧较新".into(),
                                    });
                                } else {
                                    plan.skipped += 1;
                                    plan.warnings.push(format!("{rel}: 时间相同但内容不同，已跳过"));
                                }
                            }
                            ConflictChoice::Left => actions.push(SyncAction {
                                rel: rel.clone(),
                                source: Some(l_path),
                                target,
                                bytes: l.size,
                                kind: ActionKind::CopyLeftToRight,
                                reason: "冲突：按规则保留左侧".into(),
                            }),
                            ConflictChoice::Right => actions.push(SyncAction {
                                rel: rel.clone(),
                                source: Some(opts.right.join(rel)),
                                target: l_path,
                                bytes: r.size,
                                kind: ActionKind::CopyRightToLeft,
                                reason: "冲突：按规则保留右侧".into(),
                            }),
                            ConflictChoice::Skip => {
                                plan.conflicts += 1;
                                actions.push(SyncAction {
                                    rel: rel.clone(),
                                    source: None,
                                    target,
                                    bytes: r.size,
                                    kind: ActionKind::Conflict,
                                    reason: "冲突：已跳过".into(),
                                });
                            }
                        }
                    }
                    Some(_) => plan.skipped += 1,
                }
            }
            for (rel, r) in &right {
                if !left.contains_key(rel) {
                    actions.push(SyncAction {
                        rel: rel.clone(),
                        source: Some(opts.right.join(rel)),
                        target: opts.left.join(rel),
                        bytes: r.size,
                        kind: ActionKind::CopyRightToLeft,
                        reason: "左侧缺失".into(),
                    });
                }
            }
        }
    }

    actions.sort_by(|a, b| a.rel.cmp(&b.rel));
    plan.actions = actions;
    plan
}

fn digest(path: &Path) -> std::io::Result<blake3::Hash> {
    let mut reader = BufReader::with_capacity(256 * 1024, std::fs::File::open(path)?);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize())
}

fn copy_file(source: &Path, target: &Path, opts: &SyncOptions) -> std::io::Result<()> {
    if opts.unbuffered || opts.buffer_mb == 0 {
        std::fs::copy(source, target)?;
        return Ok(());
    }
    let capacity = (opts.buffer_mb * 1024 * 1024).clamp(64 * 1024, 64 * 1024 * 1024);
    let mut reader = BufReader::with_capacity(capacity, std::fs::File::open(source)?);
    let mut writer = BufWriter::with_capacity(capacity, std::fs::File::create(target)?);
    std::io::copy(&mut reader, &mut writer)?;
    writer.flush()?;
    Ok(())
}

fn apply_times(source: &Path, target: &Path) {
    let Ok(meta) = std::fs::metadata(source) else { return };
    let mut times = std::fs::FileTimes::new();
    if let Ok(modified) = meta.modified() {
        times = times.set_modified(modified);
    }
    if let Ok(accessed) = meta.accessed() {
        times = times.set_accessed(accessed);
    }
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(target) {
        let _ = file.set_times(times);
    }
}

fn execute_copy(action: &SyncAction, opts: &SyncOptions) -> Result<bool, String> {
    let Some(source) = action.source.as_ref() else {
        return Err("复制动作缺少源文件".into());
    };
    let existed = action.target.exists();
    if let Some(parent) = action.target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let attempts = opts.auto_retry as usize + 1;
    let mut last_error = String::new();
    for attempt in 0..attempts {
        match copy_file(source, &action.target, opts) {
            Ok(()) => {
                if opts.verify {
                    let source_hash = digest(source).map_err(|e| format!("校验源文件失败: {e}"))?;
                    let target_hash = digest(&action.target).map_err(|e| format!("校验目标失败: {e}"))?;
                    if source_hash != target_hash {
                        last_error = "校验失败：文件内容不一致".into();
                        let _ = std::fs::remove_file(&action.target);
                        if attempt + 1 < attempts {
                            std::thread::sleep(Duration::from_millis(120));
                        }
                        continue;
                    }
                }
                if opts.preserve_timestamps {
                    apply_times(source, &action.target);
                }
                return Ok(existed);
            }
            Err(e) => {
                last_error = e.to_string();
                if attempt + 1 < attempts {
                    std::thread::sleep(Duration::from_millis(120));
                }
            }
        }
    }
    Err(last_error)
}

pub fn execute<F>(plan: &SyncPlan, opts: &SyncOptions, cancel: &AtomicBool, progress: F) -> SyncReport
where
    F: Fn(SyncProgress) + Sync + Send,
{
    let mut report = SyncReport {
        dry_run: opts.dry_run,
        skipped: plan.skipped + plan.conflicts,
        planned: plan.actions.len(),
        ..Default::default()
    };
    if opts.dry_run {
        for action in &plan.actions {
            match action.kind {
                ActionKind::CopyLeftToRight | ActionKind::CopyRightToLeft => {
                    report.copied += 1;
                    report.bytes += action.bytes;
                }
                ActionKind::DeleteLeft | ActionKind::DeleteRight => report.deleted += 1,
                ActionKind::Conflict => {}
            }
        }
        return report;
    }

    let mut copy_actions: Vec<&SyncAction> = plan.copy_actions().collect();
    copy_actions.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let total = copy_actions.len() as u64;
    let done = AtomicUsize::new(0);

    let results: Vec<Result<(ActionKind, bool, u64), String>> = if opts.concurrent > 1 && copy_actions.len() > 1 {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(opts.concurrent.clamp(2, 16))
            .build();
        match pool {
            Ok(pool) => pool.install(|| {
                copy_actions
                    .par_iter()
                    .map(|action| {
                        if cancel.load(Ordering::Relaxed) {
                            return Err("__cancelled__".into());
                        }
                        let result = execute_copy(action, opts)
                            .map(|updated| (action.kind, updated, action.bytes));
                        let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                        progress(SyncProgress {
                            stage: "复制文件",
                            done: finished as u64,
                            total,
                            message: action.rel.clone(),
                        });
                        result
                    })
                    .collect()
            }),
            Err(e) => {
                report.errors.push(format!("并发线程池创建失败，已改为单线程: {e}"));
                copy_actions
                    .iter()
                    .map(|action| execute_copy(action, opts).map(|updated| (action.kind, updated, action.bytes)))
                    .collect()
            }
        }
    } else {
        copy_actions
            .iter()
            .map(|action| {
                if cancel.load(Ordering::Relaxed) {
                    return Err("__cancelled__".into());
                }
                let result = execute_copy(action, opts).map(|updated| (action.kind, updated, action.bytes));
                let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(SyncProgress {
                    stage: "复制文件",
                    done: finished as u64,
                    total,
                    message: action.rel.clone(),
                });
                result
            })
            .collect()
    };

    for result in results {
        match result {
            Ok((kind, updated, bytes)) => {
                report.copied += 1;
                if updated {
                    report.updated += 1;
                }
                report.bytes += bytes;
                let _ = kind;
            }
            Err(e) if e == "__cancelled__" => report.cancelled = true,
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push(e);
                }
            }
        }
    }

    for action in plan.delete_actions() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let target = &action.target;
        if util::deletion_blocked(target) {
            report.skipped += 1;
            report.errors.push(format!("受保护路径已跳过: {}", target.display()));
            continue;
        }
        match std::fs::remove_file(target) {
            Ok(()) => report.deleted += 1,
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push(format!("删除 {}: {e}", target.display()));
                }
            }
        }
    }

    if !report.cancelled && opts.mode == SyncModeChoice::Mirror {
        let mut dirs: Vec<PathBuf> = WalkDir::new(&opts.right)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_dir() && e.depth() > 0)
            .map(|e| e.into_path())
            .collect();
        dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for dir in dirs {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    report
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncJob {
    pub name: String,
    pub left: String,
    pub right: String,
    pub mode: SyncModeChoice,
    pub conflict: ConflictChoice,
    pub include_patterns: String,
    pub exclude_patterns: String,
    pub regex_exclude: String,
    pub min_size_kb: u64,
    pub max_size_mb: u64,
    pub exclude_hidden: bool,
    pub buffer_mb: usize,
    pub concurrent: usize,
    pub verify: bool,
    pub preserve_timestamps: bool,
    pub dry_run: bool,
}

impl SyncJob {
    pub fn from_options(name: &str, opts: &SyncOptions) -> Self {
        Self {
            name: name.to_string(),
            left: opts.left.display().to_string(),
            right: opts.right.display().to_string(),
            mode: opts.mode,
            conflict: opts.conflict,
            include_patterns: opts.include_patterns.clone(),
            exclude_patterns: opts.exclude_patterns.clone(),
            regex_exclude: opts.regex_exclude.clone(),
            min_size_kb: opts.min_size_kb,
            max_size_mb: opts.max_size_mb,
            exclude_hidden: opts.exclude_hidden,
            buffer_mb: opts.buffer_mb,
            concurrent: opts.concurrent,
            verify: opts.verify,
            preserve_timestamps: opts.preserve_timestamps,
            dry_run: opts.dry_run,
        }
    }

    pub fn to_options(&self) -> SyncOptions {
        SyncOptions {
            left: PathBuf::from(&self.left),
            right: PathBuf::from(&self.right),
            mode: self.mode,
            conflict: self.conflict,
            include_patterns: self.include_patterns.clone(),
            exclude_patterns: self.exclude_patterns.clone(),
            regex_exclude: self.regex_exclude.clone(),
            min_size_kb: self.min_size_kb,
            max_size_mb: self.max_size_mb,
            exclude_hidden: self.exclude_hidden,
            buffer_mb: self.buffer_mb,
            concurrent: self.concurrent,
            unbuffered: false,
            verify: self.verify,
            follow_symlinks: false,
            preserve_timestamps: self.preserve_timestamps,
            auto_retry: 0,
            dry_run: self.dry_run,
        }
    }
}

pub fn save_job(path: &Path, job: &SyncJob) -> Result<(), String> {
    let text = toml::to_string_pretty(job).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn load_job(path: &Path) -> Result<SyncJob, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fts_sync_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("left")).unwrap();
        fs::create_dir_all(path.join("right")).unwrap();
        path
    }

    fn run(opts: &SyncOptions) -> (SyncPlan, SyncReport) {
        let plan = build_plan(opts, &AtomicBool::new(false), &mut |_| {});
        let report = execute(&plan, opts, &AtomicBool::new(false), |_| {});
        (plan, report)
    }

    #[test]
    fn mirror_copies_and_removes_extras() {
        let root = temp("mirror");
        let left = root.join("left");
        let right = root.join("right");
        fs::write(left.join("a.txt"), "alpha").unwrap();
        fs::create_dir_all(left.join("sub")).unwrap();
        fs::write(left.join("sub/b.txt"), "beta").unwrap();
        fs::write(right.join("a.txt"), "old").unwrap();
        fs::write(right.join("extra.txt"), "remove me").unwrap();
        let opts = SyncOptions { left: left.clone(), right: right.clone(), mode: SyncModeChoice::Mirror, ..Default::default() };
        let (plan, report) = run(&opts);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert_eq!(report.copied, 2, "{:?}", report.errors);
        assert_eq!(report.deleted, 1);
        assert_eq!(fs::read_to_string(right.join("sub/b.txt")).unwrap(), "beta");
        assert!(!right.join("extra.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_copies_missing_and_newer_only() {
        let root = temp("update");
        let left = root.join("left");
        let right = root.join("right");
        fs::write(left.join("new.txt"), "left-new").unwrap();
        fs::write(right.join("new.txt"), "right-old").unwrap();
        fs::write(left.join("missing.txt"), "left").unwrap();
        fs::write(right.join("extra.txt"), "keep").unwrap();
        let opts = SyncOptions { left: left.clone(), right: right.clone(), mode: SyncModeChoice::Update, ..Default::default() };
        let (_, report) = run(&opts);
        assert_eq!(report.copied, 2, "{:?}", report.errors);
        assert_eq!(report.deleted, 0);
        assert_eq!(fs::read_to_string(right.join("new.txt")).unwrap(), "left-new");
        assert!(right.join("extra.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn twoway_merges_both_sides_and_respects_conflict() {
        let root = temp("twoway");
        let left = root.join("left");
        let right = root.join("right");
        fs::write(left.join("only_left.txt"), "L").unwrap();
        fs::write(right.join("only_right.txt"), "R").unwrap();
        fs::write(left.join("clash.txt"), "left-version").unwrap();
        fs::write(right.join("clash.txt"), "right-version").unwrap();
        let opts = SyncOptions {
            left: left.clone(),
            right: right.clone(),
            mode: SyncModeChoice::Twoway,
            conflict: ConflictChoice::Left,
            ..Default::default()
        };
        let (plan, report) = run(&opts);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert_eq!(report.copied, 3, "{:?}", report.errors);
        assert_eq!(fs::read_to_string(right.join("clash.txt")).unwrap(), "left-version");
        assert!(left.join("only_right.txt").exists());
        assert!(right.join("only_left.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn filters_and_dry_run() {
        let root = temp("filter");
        let left = root.join("left");
        let right = root.join("right");
        fs::write(left.join("keep.txt"), "keep").unwrap();
        fs::write(left.join("skip.log"), "log").unwrap();
        fs::write(left.join(".hidden.txt"), "hidden").unwrap();
        let opts = SyncOptions {
            left: left.clone(),
            right: right.clone(),
            exclude_patterns: "*.log".into(),
            exclude_hidden: true,
            dry_run: true,
            ..Default::default()
        };
        let (plan, report) = run(&opts);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(report.copied, 1);
        assert!(report.dry_run);
        assert!(!right.join("keep.txt").exists());
        assert!(!right.join("skip.log").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn job_roundtrip() {
        let root = temp("job");
        let job = SyncJob::from_options(
            "备份",
            &SyncOptions { left: root.join("left"), right: root.join("right"), verify: true, concurrent: 2, ..Default::default() },
        );
        let path = root.join("job.toml");
        save_job(&path, &job).unwrap();
        let loaded = load_job(&path).unwrap();
        assert_eq!(loaded.name, "备份");
        assert!(loaded.verify);
        assert_eq!(loaded.concurrent, 2);
        assert_eq!(loaded.to_options().left, root.join("left"));
        let _ = fs::remove_dir_all(&root);
    }
}
