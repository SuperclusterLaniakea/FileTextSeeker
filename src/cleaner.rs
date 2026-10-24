use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use walkdir::WalkDir;

use crate::config::{CleanKind, CleanerSettings, CustomCleanRule};
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    DirContents,
    File,
    Glob,
}

#[derive(Clone, Debug)]
pub struct CleanRule {
    pub id: String,
    pub name: String,
    pub group: String,
    pub path: String,
    pub kind: RuleKind,
    pub pattern: String,
    pub note: String,
    pub dangerous: bool,
}

struct BuiltinRule {
    id: &'static str,
    name: &'static str,
    group: &'static str,
    path: &'static str,
    kind: RuleKind,
    pattern: &'static str,
    note: &'static str,
    dangerous: bool,
}

const fn rule(
    id: &'static str,
    name: &'static str,
    group: &'static str,
    path: &'static str,
    kind: RuleKind,
    pattern: &'static str,
    note: &'static str,
    dangerous: bool,
) -> BuiltinRule {
    BuiltinRule { id, name, group, path, kind, pattern, note, dangerous }
}

const BUILTINS: &[BuiltinRule] = &[
    rule("user-temp", "用户临时文件", "系统", "%TEMP%", RuleKind::DirContents, "", "应用运行产生的临时文件，可安全清理", false),
    rule("windows-temp", "Windows 临时文件", "系统", r"%WINDIR%\Temp", RuleKind::DirContents, "", "系统级临时文件", false),
    rule("update-download", "Windows 更新下载缓存", "系统", r"%WINDIR%\SoftwareDistribution\Download", RuleKind::DirContents, "", "已下载的更新安装包，删除后需要时会重新下载", false),
    rule("delivery-optimization", "传递优化缓存", "系统", r"%WINDIR%\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization", RuleKind::DirContents, "", "Windows 更新点对点分发缓存", false),
    rule("prefetch", "预读取缓存", "系统", r"%WINDIR%\Prefetch", RuleKind::DirContents, "", "程序启动预读数据，删除后会自动重建", false),
    rule("wer-user", "错误报告（用户）", "系统", r"%LOCALAPPDATA%\Microsoft\Windows\WER", RuleKind::DirContents, "", "应用崩溃报告和转储", false),
    rule("wer-machine", "错误报告（系统）", "系统", r"%PROGRAMDATA%\Microsoft\Windows\WER", RuleKind::DirContents, "", "系统级错误报告", false),
    rule("crashdumps", "应用崩溃转储", "系统", r"%LOCALAPPDATA%\CrashDumps", RuleKind::DirContents, "", "崩溃时生成的内存转储", false),
    rule("minidump", "小型内存转储", "系统", r"%WINDIR%\Minidump", RuleKind::DirContents, "", "蓝屏时生成的小型转储", false),
    rule("memory-dmp", "完整内存转储", "系统", r"%WINDIR%\MEMORY.DMP", RuleKind::File, "", "蓝屏完整内存转储，通常很大", false),
    rule("thumbcache", "缩略图缓存", "系统", r"%LOCALAPPDATA%\Microsoft\Windows\Explorer", RuleKind::Glob, "thumbcache_*.db", "资源管理器缩略图数据库", false),
    rule("iconcache", "图标缓存", "系统", r"%LOCALAPPDATA%\Microsoft\Windows\Explorer", RuleKind::Glob, "iconcache_*.db", "图标缓存数据库，删除后自动重建", false),
    rule("inetcache", "系统网络缓存", "系统", r"%LOCALAPPDATA%\Microsoft\Windows\INetCache", RuleKind::DirContents, "", "WinINet 网络临时文件", false),
    rule("recent-files", "最近使用记录", "系统", r"%APPDATA%\Microsoft\Windows\Recent", RuleKind::DirContents, "", "最近打开文档的快捷方式记录", false),
    rule("jump-lists", "跳转列表（自动）", "系统", r"%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations", RuleKind::DirContents, "", "任务栏右键跳转列表缓存", false),
    rule("jump-custom", "跳转列表（自定义）", "系统", r"%APPDATA%\Microsoft\Windows\Recent\CustomDestinations", RuleKind::DirContents, "", "用户固定的跳转列表项", false),
    rule("windows-logs", "Windows 日志", "系统", r"%WINDIR%\Logs", RuleKind::DirContents, "", "系统组件运行日志", false),
    rule("cbs-logs", "CBS 组件日志", "系统", r"%WINDIR%\Logs\CBS", RuleKind::DirContents, "", "组件服务安装日志，可能较大", false),
    rule("dism-logs", "DISM 日志", "系统", r"%WINDIR%\Logs\DISM", RuleKind::DirContents, "", "系统映像维护日志", false),
    rule("defender-history", "Defender 扫描历史", "系统", r"%PROGRAMDATA%\Microsoft\Windows Defender\Scans\History", RuleKind::DirContents, "", "安全扫描历史记录", true),
    rule("d3d-cache", "DirectX 着色器缓存", "系统", r"%LOCALAPPDATA%\D3DSCache", RuleKind::DirContents, "", "游戏与图形程序的着色器缓存", false),
    rule("panther", "Windows 安装临时文件", "系统", r"%WINDIR%\Panther", RuleKind::DirContents, "", "系统安装与升级日志，删除前请确认无正在进行的升级", true),
    rule("edge-cache", "Edge 网页缓存", "浏览器", r"%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Cache", RuleKind::DirContents, "", "Edge 浏览网页的临时图片与脚本", false),
    rule("edge-code-cache", "Edge 代码缓存", "浏览器", r"%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Code Cache", RuleKind::DirContents, "", "Edge 编译后的 JavaScript 缓存", false),
    rule("chrome-cache", "Chrome 网页缓存", "浏览器", r"%LOCALAPPDATA%\Google\Chrome\User Data\Default\Cache", RuleKind::DirContents, "", "Chrome 浏览网页的临时文件", false),
    rule("chrome-code-cache", "Chrome 代码缓存", "浏览器", r"%LOCALAPPDATA%\Google\Chrome\User Data\Default\Code Cache", RuleKind::DirContents, "", "Chrome 编译后的 JavaScript 缓存", false),
    rule("firefox-cache", "Firefox 网页缓存", "浏览器", r"%LOCALAPPDATA%\Mozilla\Firefox\Profiles", RuleKind::Glob, "**/cache2/**", "Firefox 各配置文件的网页缓存", false),
    rule("npm-cache", "npm 缓存", "开发工具", r"%LOCALAPPDATA%\npm-cache", RuleKind::DirContents, "", "Node.js 包管理器下载缓存", true),
    rule("pip-cache", "pip 缓存", "开发工具", r"%LOCALAPPDATA%\pip\Cache", RuleKind::DirContents, "", "Python 包管理器下载缓存", true),
    rule("cargo-cache", "Cargo 下载缓存", "开发工具", r"%USERPROFILE%\.cargo\registry\cache", RuleKind::DirContents, "", "Rust 依赖包压缩缓存，删除后重新编译时需再次下载", true),
    rule("nuget-cache", "NuGet 包缓存", "开发工具", r"%USERPROFILE%\.nuget\packages", RuleKind::DirContents, "", " .NET 依赖包缓存，删除会影响离线构建", true),
    rule("gradle-cache", "Gradle 缓存", "开发工具", r"%USERPROFILE%\.gradle\caches", RuleKind::DirContents, "", "Java 构建缓存，删除后需重新下载依赖", true),
    rule("vscode-cache", "VS Code 缓存", "应用", r"%APPDATA%\Code\Cache", RuleKind::DirContents, "", "编辑器界面缓存", false),
    rule("vscode-codedata", "VS Code CachedData", "应用", r"%APPDATA%\Code\CachedData", RuleKind::DirContents, "", "编辑器运行时代码缓存", false),
    rule("wechat-cache", "微信缓存", "应用", r"%APPDATA%\Tencent\WeChat", RuleKind::DirContents, "", "微信聊天文件与图片缓存，清理可能丢失本地记录", true),
    rule("qq-cache", "QQ 缓存", "应用", r"%APPDATA%\Tencent\QQ", RuleKind::DirContents, "", "QQ 图片与文件缓存", true),
    rule("netease-cache", "网易云音乐缓存", "应用", r"%LOCALAPPDATA%\NetEase\CloudMusic\Cache", RuleKind::DirContents, "", "音乐播放缓存", false),
    rule("downloads-temp", "下载残留文件", "下载", r"%USERPROFILE%\Downloads", RuleKind::Glob, "*.crdownload;*.part;*.partial;*.download;*.tmp", "未完成或中断的下载临时文件", false),
];

pub fn builtin_rules() -> impl Iterator<Item = CleanRule> + 'static {
    BUILTINS.iter().map(|r| CleanRule {
        id: r.id.to_string(),
        name: r.name.to_string(),
        group: r.group.to_string(),
        path: r.path.to_string(),
        kind: r.kind,
        pattern: r.pattern.to_string(),
        note: r.note.to_string(),
        dangerous: r.dangerous,
    })
}

pub fn builtin_count() -> usize {
    BUILTINS.len()
}

pub fn custom_rule(rule: &CustomCleanRule) -> CleanRule {
    CleanRule {
        id: rule.id.clone(),
        name: rule.name.clone(),
        group: "自定义".into(),
        path: rule.path.clone(),
        kind: match rule.kind {
            CleanKind::Dir => RuleKind::DirContents,
            CleanKind::File => RuleKind::File,
            CleanKind::Glob => RuleKind::Glob,
        },
        pattern: rule.pattern.clone(),
        note: rule.note.clone(),
        dangerous: rule.dangerous,
    }
}

pub fn active_rules(settings: &CleanerSettings) -> Vec<CleanRule> {
    let mut rules: Vec<CleanRule> = builtin_rules()
        .filter(|r| !settings.disabled_builtin.contains(&r.id))
        .collect();
    for custom in settings.custom_rules.iter().filter(|r| r.enabled && !r.path.trim().is_empty()) {
        rules.push(custom_rule(custom));
    }
    rules
}

pub fn expand_env(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '%') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                if !name.is_empty() {
                    if let Some(value) = std::env::var_os(&name) {
                        out.push_str(&value.to_string_lossy());
                        i += end + 2;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn split_patterns(raw: &str) -> Vec<String> {
    raw.split([';', ',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn pattern_matches(patterns: &[String], path: &Path, rel: &str) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let rel = rel.replace('\\', "/").to_lowercase();
    patterns.iter().any(|p| {
        if let Ok(glob) = glob::Pattern::new(p) {
            if glob.matches(&name) || glob.matches(&rel) || glob.matches_path(path) {
                return true;
            }
        }
        if p.contains("**") {
            if let Some(suffix) = p.strip_prefix("**/").and_then(|s| s.strip_suffix("/**")) {
                return format!("/{rel}/").contains(&format!("/{suffix}/"))
                    || rel.split('/').any(|part| part.eq_ignore_ascii_case(suffix));
            }
        }
        false
    })
}

#[derive(Clone, Debug)]
pub struct CleanItem {
    pub rule_id: String,
    pub rule_name: String,
    pub group: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CleanScan {
    pub items: Vec<CleanItem>,
    pub errors: Vec<String>,
    pub total_bytes: u64,
    pub file_count: usize,
    pub dir_count: usize,
    pub scanned_rules: usize,
    pub capped: bool,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub struct CleanProgress {
    pub stage: &'static str,
    pub done: usize,
    pub current: String,
}

pub fn scan(rules: &[CleanRule], cancel: &AtomicBool, progress: &mut dyn FnMut(CleanProgress)) -> CleanScan {
    let mut scan = CleanScan::default();
    let mut seen: HashSet<String> = HashSet::new();
    const MAX_ITEMS: usize = 250_000;

    for rule in rules {
        if cancel.load(Ordering::Relaxed) {
            scan.cancelled = true;
            break;
        }
        scan.scanned_rules += 1;
        let expanded = expand_env(&rule.path);
        let base = PathBuf::from(&expanded);
        if expanded.trim().is_empty() || !base.exists() {
            continue;
        }
        progress(CleanProgress {
            stage: "扫描清理项目",
            done: scan.items.len(),
            current: rule.name.clone(),
        });
        match rule.kind {
            RuleKind::File => {
                if base.is_file() {
                    add_item(&mut scan, &mut seen, rule, base, 0, false);
                }
            }
            RuleKind::DirContents => {
                if !base.is_dir() {
                    continue;
                }
                for entry in WalkDir::new(&base)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                    .skip(1)
                {
                    if cancel.load(Ordering::Relaxed) {
                        scan.cancelled = true;
                        break;
                    }
                    if entry.depth() == 0 {
                        continue;
                    }
                    let path = entry.path().to_path_buf();
                    if util::deletion_blocked(&path) {
                        continue;
                    }
                    if entry.file_type().is_dir() {
                        add_dir(&mut scan, &mut seen, rule, path);
                    } else if entry.file_type().is_file() {
                        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        add_item(&mut scan, &mut seen, rule, path, size, false);
                    }
                    if scan.items.len() >= MAX_ITEMS {
                        scan.capped = true;
                        break;
                    }
                }
            }
            RuleKind::Glob => {
                if !base.is_dir() {
                    continue;
                }
                let patterns = split_patterns(&rule.pattern);
                if patterns.is_empty() {
                    continue;
                }
                for entry in WalkDir::new(&base)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                {
                    if cancel.load(Ordering::Relaxed) {
                        scan.cancelled = true;
                        break;
                    }
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let path = entry.path();
                    let rel = path
                        .strip_prefix(&base)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !pattern_matches(&patterns, path, &rel) {
                        continue;
                    }
                    if util::deletion_blocked(path) {
                        continue;
                    }
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    add_item(&mut scan, &mut seen, rule, path.to_path_buf(), size, false);
                    if scan.items.len() >= MAX_ITEMS {
                        scan.capped = true;
                        break;
                    }
                }
            }
        }
        if scan.capped {
            break;
        }
    }
    scan
}

fn add_item(scan: &mut CleanScan, seen: &mut HashSet<String>, rule: &CleanRule, path: PathBuf, size: u64, is_dir: bool) {
    let key = path.to_string_lossy().to_lowercase();
    if !seen.insert(key) {
        return;
    }
    scan.total_bytes += size;
    if is_dir {
        scan.dir_count += 1;
    } else {
        scan.file_count += 1;
    }
    scan.items.push(CleanItem {
        rule_id: rule.id.clone(),
        rule_name: rule.name.clone(),
        group: rule.group.clone(),
        path,
        size,
        is_dir,
    });
}

fn add_dir(scan: &mut CleanScan, seen: &mut HashSet<String>, rule: &CleanRule, path: PathBuf) {
    add_item(scan, seen, rule, path, 0, true);
}

#[derive(Clone, Debug, Default)]
pub struct CleanReport {
    pub deleted_files: usize,
    pub deleted_dirs: usize,
    pub freed: u64,
    pub failed: usize,
    pub blocked: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

pub fn delete_items(items: &[CleanItem], move_to_trash: bool, cancel: &AtomicBool, progress: &mut dyn FnMut(CleanProgress)) -> CleanReport {
    let mut report = CleanReport::default();
    let mut files: Vec<&CleanItem> = items.iter().filter(|i| !i.is_dir).collect();
    files.sort_by_key(|i| std::cmp::Reverse(i.size));
    let total = items.len();
    for (index, item) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return report;
        }
        progress(CleanProgress { stage: "删除文件", done: index, current: item.path.display().to_string() });
        if util::deletion_blocked(&item.path) {
            report.blocked += 1;
            continue;
        }
        let result = if move_to_trash {
            trash::delete(&item.path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(&item.path).map_err(|e| e.to_string())
        };
        match result {
            Ok(()) => {
                report.deleted_files += 1;
                report.freed += item.size;
            }
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push(format!("{}: {e}", item.path.display()));
                }
            }
        }
    }

    let mut dirs: Vec<&CleanItem> = items.iter().filter(|i| i.is_dir).collect();
    dirs.sort_by_key(|i| std::cmp::Reverse(i.path.components().count()));
    for (index, item) in dirs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        progress(CleanProgress { stage: "删除空目录", done: files.len() + index, current: item.path.display().to_string() });
        if util::deletion_blocked(&item.path) {
            report.blocked += 1;
            continue;
        }
        let empty = std::fs::read_dir(&item.path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            continue;
        }
        let result = if move_to_trash {
            trash::delete(&item.path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_dir(&item.path).map_err(|e| e.to_string())
        };
        match result {
            Ok(()) => report.deleted_dirs += 1,
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push(format!("{}: {e}", item.path.display()));
                }
            }
        }
    }
    let _ = total;
    report
}

pub fn create_restore_point() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = "Checkpoint-Computer -Description 'ToolsNest cleanup' -RestorePointType 'MODIFY_SETTINGS'";
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("无法启动 PowerShell: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(if stderr.is_empty() {
                "创建还原点失败，请确认系统保护已开启并以管理员身份运行".into()
            } else {
                stderr
            })
        }
    }
    #[cfg(not(windows))]
    {
        Err("仅支持 Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fts_clean_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn has_at_least_36_builtin_rules_and_expands_env() {
        assert!(builtin_count() >= 36, "count={}", builtin_count());
        let list: Vec<CleanRule> = builtin_rules().collect();
        assert!(list.iter().all(|r| !r.id.is_empty() && !r.name.is_empty() && !r.path.is_empty()));
        let temp = std::env::var("TEMP").unwrap();
        assert_eq!(expand_env("%TEMP%"), temp);
        assert_eq!(expand_env("prefix-%TEMP%-suffix"), format!("prefix-{temp}-suffix"));
        assert_eq!(expand_env("%UNKNOWN_FTS_VAR%"), "%UNKNOWN_FTS_VAR%");
    }

    #[test]
    fn custom_dir_rule_scans_and_deletes_files() {
        let root = temp("dir");
        fs::create_dir_all(root.join("cache").join("sub")).unwrap();
        fs::write(root.join("cache/a.tmp"), "12345").unwrap();
        fs::write(root.join("cache/sub/b.tmp"), "1234567890").unwrap();
        let rules = vec![CleanRule {
            id: "custom".into(),
            name: "测试缓存".into(),
            group: "自定义".into(),
            path: root.join("cache").display().to_string(),
            kind: RuleKind::DirContents,
            pattern: String::new(),
            note: String::new(),
            dangerous: false,
        }];
        let scan = scan(&rules, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(scan.file_count, 2, "{:?}", scan.errors);
        assert_eq!(scan.total_bytes, 15);
        assert!(!scan.items.iter().any(|i| i.path == root.join("cache")));
        let report = delete_items(&scan.items, false, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(report.deleted_files, 2, "{:?}", report.errors);
        assert_eq!(report.freed, 15);
        assert!(!root.join("cache/a.tmp").exists());
        assert!(root.join("cache").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn glob_rule_matches_patterns() {
        let root = temp("glob");
        fs::create_dir_all(root.join("cache/sub")).unwrap();
        fs::write(root.join("cache/a.tmp"), "a").unwrap();
        fs::write(root.join("cache/keep.txt"), "keep").unwrap();
        fs::write(root.join("cache/sub/b.tmp"), "b").unwrap();
        let rules = vec![CleanRule {
            id: "glob".into(),
            name: "临时后缀".into(),
            group: "自定义".into(),
            path: root.join("cache").display().to_string(),
            kind: RuleKind::Glob,
            pattern: "*.tmp".into(),
            note: String::new(),
            dangerous: false,
        }];
        let scan = scan(&rules, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(scan.file_count, 2, "{:?}", scan.items);
        assert!(scan.items.iter().all(|i| i.path.extension().map(|e| e == "tmp").unwrap_or(false)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_rule_and_missing_paths_are_skipped() {
        let root = temp("file");
        let file = root.join("old.log");
        fs::write(&file, "logs").unwrap();
        let rules = vec![
            CleanRule {
                id: "f".into(),
                name: "旧日志".into(),
                group: "自定义".into(),
                path: file.display().to_string(),
                kind: RuleKind::File,
                pattern: String::new(),
                note: String::new(),
                dangerous: false,
            },
            CleanRule {
                id: "missing".into(),
                name: "不存在".into(),
                group: "自定义".into(),
                path: root.join("nope").display().to_string(),
                kind: RuleKind::DirContents,
                pattern: String::new(),
                note: String::new(),
                dangerous: false,
            },
        ];
        let scan = scan(&rules, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(scan.file_count, 1);
        assert!(scan.errors.is_empty());
        let report = delete_items(&scan.items, false, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(report.deleted_files, 1);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn settings_merge_custom_and_disabled_rules() {
        let settings = CleanerSettings {
            disabled_builtin: vec!["user-temp".into(), "windows-temp".into()],
            custom_rules: vec![CustomCleanRule {
                id: "c1".into(),
                name: "自定义".into(),
                path: "%TEMP%".into(),
                kind: CleanKind::Glob,
                pattern: "*.fts".into(),
                note: String::new(),
                enabled: true,
                dangerous: false,
            }],
            ..Default::default()
        };
        let rules = active_rules(&settings);
        assert_eq!(rules.iter().filter(|r| r.group == "自定义").count(), 1);
        assert!(!rules.iter().any(|r| r.id == "user-temp"));
        assert_eq!(rules.len(), builtin_count() - 2 + 1);
    }
}
