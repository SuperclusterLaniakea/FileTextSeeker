use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{Command, Child};
use std::ptr::null_mut;
use std::sync::Mutex;

use winapi::shared::windef::HWND;
use winapi::um::winuser::{FindWindowW, ShowWindow, SW_HIDE, SW_RESTORE};

// ─── tray-helper.exe 生命周期管理 ──────────────────────────────────────────

/// 全局 tray-helper 子进程句柄。
static TRAY_HELPER: Mutex<Option<Child>> = Mutex::new(None);

/// 确保 tray-helper.exe 正在运行（首次最小化时调用）。
pub fn ensure_tray_helper() {
    let mut guard = TRAY_HELPER.lock().unwrap();
    if guard.is_some() {
        return;
    }
    let path = get_tray_helper_path();
    if path.exists() {
        match Command::new(&path).spawn() {
            Ok(child) => {
                *guard = Some(child);
            }
            Err(e) => {
                eprintln!("tray::ensure_tray_helper: failed to launch: {}", e);
            }
        }
    } else {
        eprintln!("tray::ensure_tray_helper: {} not found", path.display());
    }
}

/// 终止 tray-helper.exe（主进程退出时调用）。
pub fn kill_tray_helper() {
    if let Some(mut child) = TRAY_HELPER.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// 通过原生 Windows API 隐藏主窗口（与 tray-helper 的恢复操作一致）。
pub fn hide_main_window() {
    unsafe {
        let title: Vec<u16> = OsStr::new("超级检索工具")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let hwnd_main: HWND = FindWindowW(null_mut(), title.as_ptr());
        if !hwnd_main.is_null() {
            ShowWindow(hwnd_main, SW_HIDE);
        }
    }
}

fn get_tray_helper_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tray-helper.exe")
}