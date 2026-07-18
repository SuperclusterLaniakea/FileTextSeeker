#![windows_subsystem = "windows"]

/// Standalone Windows tray helper — 不依赖任何第三方托盘库
///
/// 使用 WinAPI 直接操作（winapi crate）。
/// 在后台运行一个托盘图标，点击「显示」时查找主窗口并显示/启动，
/// 点击「隐藏」时隐藏主窗口，点击「退出」时结束自身进程。

use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::process::Command;

use winapi::shared::minwindef::{DWORD, LPARAM, LRESULT, UINT, WPARAM, FALSE};
use winapi::shared::windef::{HICON, HWND, POINT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::shellapi::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use winapi::um::winuser::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW,
    LoadIconW, LoadImageW, PostMessageW, PostQuitMessage, RegisterClassExW,
    SetForegroundWindow, ShowWindow, TrackPopupMenu, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDI_APPLICATION, IMAGE_ICON,
    LR_LOADFROMFILE, MF_STRING, MSG, SW_HIDE, SW_RESTORE, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NULL,
    WM_RBUTTONUP, WM_USER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

// ── Constants ─────────────────────────────────────────────────────────────

const WM_TRAYICON: UINT = WM_USER + 1;
const TRAY_ICON_ID: UINT = 1001;
const CMD_SHOW: UINT = 1002;
const CMD_HIDE: UINT = 1003;
const CMD_EXIT: UINT = 1004;

// ── String helpers ────────────────────────────────────────────────────────

fn to_wstring(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

unsafe fn copy_wstr_into(src: &[u16], dst: *mut u16, max_len: usize) {
    let mut i = 0usize;
    while i < max_len - 1 && i < src.len() && *src.as_ptr().add(i) != 0 {
        *dst.add(i) = *src.as_ptr().add(i);
        i += 1;
    }
    *dst.add(i) = 0;
}

// ── Icon loading ──────────────────────────────────────────────────────────

/// 尝试从 icon.ico 文件加载图标，失败时使用系统默认图标。
unsafe fn load_tray_icon() -> HICON {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let paths = &[
        "icon.ico",           // 当前目录 / exe 同目录
        "../icon.ico",        // 上一层
        "../../icon.ico",     // 上两层（调试时可能在 target/debug/）
    ];

    for &name in paths {
        // 尝试相对于 exe 所在目录
        let full = exe_dir.join(name);
        if full.exists() {
            let wfull = to_wstring(&full.to_string_lossy());
            let handle = LoadImageW(
                null_mut(),
                wfull.as_ptr(),
                IMAGE_ICON,
                32, 32,
                LR_LOADFROMFILE,
            );
            if !handle.is_null() {
                return handle as HICON;
            }
        }

        // 尝试相对于当前工作目录
        let wpath = to_wstring(name);
        let handle = LoadImageW(
            null_mut(),
            wpath.as_ptr(),
            IMAGE_ICON,
            32, 32,
            LR_LOADFROMFILE,
        );
        if !handle.is_null() {
            return handle as HICON;
        }
    }

    LoadIconW(null_mut(), IDI_APPLICATION)
}

// ── Tray icon operations ──────────────────────────────────────────────────

unsafe fn add_tray_icon(hwnd: HWND) -> bool {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ICON_ID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;

    let tip = to_wstring("超级检索工具 - 后台运行");
    copy_wstr_into(&tip, nid.szTip.as_mut_ptr(), 128);

    nid.hIcon = load_tray_icon();

    Shell_NotifyIconW(NIM_ADD, &mut nid) != FALSE
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ICON_ID;
    Shell_NotifyIconW(NIM_DELETE, &mut nid);
}

// ── Context menu ──────────────────────────────────────────────────────────

unsafe fn show_context_menu(hwnd: HWND) {
    let hmenu = CreatePopupMenu();
    if hmenu.is_null() {
        return;
    }

    AppendMenuW(
        hmenu,
        MF_STRING,
        CMD_SHOW as usize,
        to_wstring("显示 超级检索工具").as_ptr(),
    );
    AppendMenuW(
        hmenu,
        MF_STRING,
        CMD_HIDE as usize,
        to_wstring("隐藏到托盘").as_ptr(),
    );
    AppendMenuW(
        hmenu,
        MF_STRING,
        CMD_EXIT as usize,
        to_wstring("退出托盘助手").as_ptr(),
    );

    let mut pos = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pos);

    SetForegroundWindow(hwnd);

    TrackPopupMenu(
        hmenu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pos.x,
        pos.y,
        0,
        hwnd,
        null_mut(),
    );

    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(hmenu);
}

// ── Main app window management ───────────────────────────────────────────

/// 查找主窗口（标题为 "超级检索工具"），如果找到则恢复并前置，否则启动新实例。
unsafe fn show_or_launch_main_app() {
    let title = to_wstring("超级检索工具");
    let hwnd_main = FindWindowW(null_mut(), title.as_ptr());
    if !hwnd_main.is_null() {
        ShowWindow(hwnd_main, SW_RESTORE);
        SetForegroundWindow(hwnd_main);
    } else {
        let main_exe = get_main_exe_path();
        let _ = Command::new(&main_exe).spawn();
    }
}

/// 查找主窗口并隐藏到托盘。
unsafe fn hide_main_window() {
    let title = to_wstring("超级检索工具");
    let hwnd_main = FindWindowW(null_mut(), title.as_ptr());
    if !hwnd_main.is_null() {
        ShowWindow(hwnd_main, SW_HIDE);
    }
}

fn get_main_exe_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("super-searcher.exe")
}

// ── Window procedure ──────────────────────────────────────────────────────

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            if !add_tray_icon(hwnd) {
                return -1;
            }
            0
        }

        WM_TRAYICON => {
            let mouse_msg = lparam & 0xFFFF;
            match mouse_msg as UINT {
                WM_RBUTTONUP => show_context_menu(hwnd),
                WM_LBUTTONDBLCLK => {
                    show_or_launch_main_app();
                }
                _ => {}
            }
            0
        }

        WM_COMMAND => {
            match wparam as UINT {
                CMD_SHOW => {
                    show_or_launch_main_app();
                }
                CMD_HIDE => {
                    hide_main_window();
                }
                CMD_EXIT => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }

        WM_DESTROY => {
            remove_tray_icon(hwnd);
            PostQuitMessage(0);
            0
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────

fn main() {
    unsafe {
        // ── 防止多个实例 ──
        let existing = FindWindowW(
            to_wstring("FileSeekerTrayHelper").as_ptr(),
            null_mut(),
        );
        if !existing.is_null() {
            return;
        }

        let hinst = GetModuleHandleW(null_mut());
        let class_name = to_wstring("FileSeekerTrayHelper");

        let wc = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as UINT,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };

        if RegisterClassExW(&wc) == 0 {
            return;
        }

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            to_wstring("超级检索工具 - 托盘助手").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            400,
            300,
            null_mut(),
            null_mut(),
            hinst,
            null_mut(),
        );

        if hwnd.is_null() {
            return;
        }

        ShowWindow(hwnd, SW_HIDE);

        let mut msg: MSG = mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}