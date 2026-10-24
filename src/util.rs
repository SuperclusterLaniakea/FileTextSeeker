use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn format_time(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn format_time_short(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%m-%d %H:%M").to_string()
}

pub fn system_time_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_secs() -> i64 {
    system_time_secs(SystemTime::now())
}

pub fn duration_short(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs} 秒")
    } else if secs < 3600 {
        format!("{} 分 {} 秒", secs / 60, secs % 60)
    } else {
        format!("{} 小时 {} 分", secs / 3600, (secs % 3600) / 60)
    }
}

pub fn open_path(path: &Path) {
    let _ = open::that_detached(path);
}

pub fn reveal_path(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new("explorer.exe");
        if path.is_dir() {
            cmd.arg(path);
        } else {
            cmd.arg(format!("/select,{}", path.display()));
        }
        let _ = cmd.creation_flags(CREATE_NO_WINDOW).spawn();
    }
    #[cfg(not(windows))]
    {
        if let Some(parent) = path.parent() {
            let _ = open::that_detached(parent);
        }
    }
}

pub fn copy_text(ctx: &eframe::egui::Context, text: &str) {
    ctx.copy_text(text.to_owned());
    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
}

pub fn canonical_or_absolute(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    })
}

pub fn valid_filename(name: &str) -> Result<(), String> {    if name.trim().is_empty() {
        return Err("文件名不能为空".into());
    }
    if name == "." || name == ".." {
        return Err("文件名无效".into());
    }
    for ch in name.chars() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (ch as u32) < 32 {
            return Err(format!("文件名包含非法字符: {ch}"));
        }
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("文件名不能以点或空格结尾".into());
    }
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("{stem} 是系统保留名称"));
    }
    Ok(())
}

pub fn split_name_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(pos) if pos > 0 && pos + 1 < name.len() => {
            (name[..pos].to_string(), name[pos + 1..].to_string())
        }
        _ => (name.to_string(), String::new()),
    }
}

pub fn decode_text(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

pub fn is_hidden_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        if let Ok(meta) = std::fs::metadata(path) {
            let attr = meta.file_attributes();
            return attr & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0;
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        use winapi::um::handleapi::CloseHandle;
        use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
        use winapi::um::securitybaseapi::GetTokenInformation;
        use winapi::um::winnt::{TokenElevation, HANDLE, TOKEN_ELEVATION, TOKEN_QUERY};
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ok != 0 && elevation.TokenIsElevated != 0
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn relaunch_elevated() -> bool {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use winapi::um::shellapi::ShellExecuteW;
        use winapi::um::winuser::SW_SHOWNORMAL;
        let exe = std::env::current_exe().unwrap_or_default();
        let wide = |s: &OsStr| -> Vec<u16> {
            s.encode_wide().chain(std::iter::once(0)).collect()
        };
        let file = wide(exe.as_os_str());
        let verb = wide(OsStr::new("runas"));
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        (result as isize) > 32
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn unique_target(target: &Path) -> PathBuf {
    if !target.exists() {
        return target.to_path_buf();
    }
    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = target
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..10_000 {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    target.to_path_buf()
}

pub fn trim_working_set() {
    #[cfg(windows)]
    unsafe {
        use winapi::um::processthreadsapi::GetCurrentProcess;
        use winapi::um::psapi::EmptyWorkingSet;
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}

pub fn process_memory_mb() -> f32 {
    #[cfg(windows)]
    {
        use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use winapi::um::processthreadsapi::GetCurrentProcess;
        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut counters,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ) != 0
            {
                return counters.WorkingSetSize as f32 / (1024.0 * 1024.0);
            }
        }
    }
    0.0
}

fn normalized(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
    text.strip_prefix(r"\\?\").unwrap_or(&text).trim_end_matches('\\').to_string()
}

/// Paths that must never be removed, even if a scan mistakenly selects them.
/// User documents can still be deleted deliberately; system roots and core
/// binary directories are always protected, with cache subfolders exempted.
pub fn deletion_blocked(path: &Path) -> bool {
    let target = normalized(path);
    if target.is_empty() {
        return true;
    }
    let components: Vec<_> = path.components().collect();
    if components.len() <= 1 {
        return true;
    }

    let mut strict: Vec<String> = Vec::new();
    if let Some(v) = std::env::var_os("WINDIR") {
        strict.push(normalized(Path::new(&v)));
    } else {
        strict.push(r"c:\windows".into());
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
        if let Some(v) = std::env::var_os(key) {
            strict.push(normalized(Path::new(&v)));
        }
    }
    strict.push(r"c:\$recycle.bin".into());
    strict.push(r"c:\system volume information".into());

    let mut exact: Vec<String> = Vec::new();
    for dir in [dirs::home_dir(), dirs::config_dir(), dirs::data_dir(), dirs::data_local_dir()] {
        if let Some(v) = dir {
            exact.push(normalized(&v));
        }
    }

    let cache_ok = [
        r"\temp\",
        r"\tmp\",
        r"\cache\",
        r"\caches\",
        r"\softwaredistribution\",
        r"\deliveryoptimization\",
        r"\prefetch\",
        r"\logs\",
        r"\crashdumps\",
        r"\minidump\",
        r"\webcache\",
    ]
    .iter()
    .any(|needle| format!("{target}\\").contains(needle));

    for root in strict {
        if root.is_empty() {
            continue;
        }
        if target == root {
            return true;
        }
        if target.starts_with(&format!("{root}\\")) && !cache_ok {
            return true;
        }
        if root.starts_with(&format!("{target}\\")) {
            return true;
        }
    }
    for root in exact {
        if !root.is_empty() && (target == root || root.starts_with(&format!("{target}\\"))) {
            return true;
        }
    }
    false
}
