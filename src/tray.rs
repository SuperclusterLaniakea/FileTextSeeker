pub enum TrayEvent {
    Show,
    ToggleMonitoring,
    Exit,
}

#[cfg(windows)]
mod platform {
    use super::TrayEvent;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
    use winapi::shared::windef::HWND;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::shellapi::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use winapi::um::synchapi::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject};
    use winapi::um::winbase::WAIT_OBJECT_0;
    use winapi::um::winnt::{EVENT_MODIFY_STATE, SYNCHRONIZE};
    use winapi::um::winuser::*;

    const WM_TRAY: UINT = WM_APP + 1;
    const ID_SHOW: usize = 101;
    const ID_MONITOR: usize = 102;
    const ID_EXIT: usize = 103;
    const CLASS_NAME: &str = "ToolsNestTrayWindow";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct TrayShared {
        sender: mpsc::Sender<TrayEvent>,
        monitoring: Arc<AtomicBool>,
        hwnd: HWND,
        icon_added: bool,
        taskbar_created: UINT,
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayShared;
        if !ptr.is_null() {
            if msg == WM_TRAY {
                match lparam as u32 {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                        let _ = (*ptr).sender.send(TrayEvent::Show);
                    }
                    WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(ptr),
                    _ => {}
                }
                return 0;
            }
            if msg == WM_COMMAND {
                handle_command(ptr, wparam & 0xffff);
                return 0;
            }
            if msg == WM_CLOSE {
                DestroyWindow(hwnd);
                return 0;
            }
            if msg == WM_DESTROY {
                remove_icon(ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                PostQuitMessage(0);
                return 0;
            }
            if msg != 0 && msg == (*ptr).taskbar_created {
                add_icon(ptr);
                return 0;
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    unsafe fn handle_command(ptr: *mut TrayShared, command: usize) {
        let event = match command {
            ID_SHOW => Some(TrayEvent::Show),
            ID_MONITOR => Some(TrayEvent::ToggleMonitoring),
            ID_EXIT => Some(TrayEvent::Exit),
            _ => None,
        };
        if let Some(event) = event {
            let _ = (*ptr).sender.send(event);
        }
    }

    unsafe fn show_menu(ptr: *mut TrayShared) {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let show = wide("打开主界面");
        let monitor = if (*ptr).monitoring.load(Ordering::Relaxed) {
            wide("关闭实时监控")
        } else {
            wide("开启实时监控")
        };
        let exit = wide("退出 ToolsNest");
        AppendMenuW(menu, MF_STRING, ID_SHOW, show.as_ptr());
        AppendMenuW(menu, MF_STRING, ID_MONITOR, monitor.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_EXIT, exit.as_ptr());
        let mut point = std::mem::zeroed();
        GetCursorPos(&mut point);
        SetForegroundWindow((*ptr).hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            0,
            (*ptr).hwnd,
            std::ptr::null_mut(),
        );
        DestroyMenu(menu);
        PostMessageW((*ptr).hwnd, WM_NULL, 0, 0);
        if command != 0 {
            handle_command(ptr, command as usize);
        }
    }

    unsafe fn remove_icon(ptr: *mut TrayShared) {
        if (*ptr).icon_added {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = (*ptr).hwnd;
            data.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &mut data);
            (*ptr).icon_added = false;
        }
    }

    unsafe fn add_icon(ptr: *mut TrayShared) {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = (*ptr).hwnd;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        let instance = GetModuleHandleW(std::ptr::null());
        let mut icon = LoadIconW(instance, MAKEINTRESOURCEW(1));
        if icon.is_null() {
            icon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        }
        data.hIcon = icon;
        let tip: Vec<u16> = "ToolsNest · 全能文件工具箱".encode_utf16().take(127).collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        (*ptr).icon_added = Shell_NotifyIconW(NIM_ADD, &mut data) != 0;
    }

    pub struct Tray {
        events: mpsc::Receiver<TrayEvent>,
        monitoring: Arc<AtomicBool>,
        hwnd: usize,
        done: Option<mpsc::Receiver<()>>,
    }

    impl Tray {
        pub fn spawn(monitoring: bool) -> Option<Self> {
            let (event_tx, event_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel::<usize>();
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let monitoring = Arc::new(AtomicBool::new(monitoring));
            let monitoring_for_thread = Arc::clone(&monitoring);
            std::thread::Builder::new()
                .name("fts-tray".into())
                .spawn(move || {
                    unsafe { run(event_tx, monitoring_for_thread, ready_tx) };
                    let _ = done_tx.send(());
                })
                .ok()?;
            let hwnd = ready_rx.recv_timeout(Duration::from_secs(5)).ok()?;
            if hwnd == 0 {
                return None;
            }
            Some(Self { events: event_rx, monitoring, hwnd, done: Some(done_rx) })
        }

        pub fn set_monitoring(&self, on: bool) {
            self.monitoring.store(on, Ordering::Relaxed);
        }

        pub fn drain(&self) -> Vec<TrayEvent> {
            let mut events = Vec::new();
            while let Ok(event) = self.events.try_recv() {
                events.push(event);
            }
            events
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
            }
            if let Some(done) = self.done.take() {
                let _ = done.recv_timeout(Duration::from_secs(2));
            }
        }
    }

    unsafe fn run(
        sender: mpsc::Sender<TrayEvent>,
        monitoring: Arc<AtomicBool>,
        ready: mpsc::Sender<usize>,
    ) {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = wide(CLASS_NAME);
        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(wndproc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        class.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        let atom = RegisterClassW(&class);
        if atom == 0 {
            let _ = ready.send(0);
            return;
        }
        let title = wide("ToolsNest");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            let _ = ready.send(0);
            UnregisterClassW(class_name.as_ptr(), instance);
            return;
        }
        let taskbar_name = wide("TaskbarCreated");
        let taskbar_created = RegisterWindowMessageW(taskbar_name.as_ptr());
        let boxed = Box::into_raw(Box::new(TrayShared {
            sender,
            monitoring,
            hwnd,
            icon_added: false,
            taskbar_created,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, boxed as isize);
        add_icon(boxed);
        let _ = ready.send(hwnd as usize);

        let mut message: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
            if result <= 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        remove_icon(boxed);
        drop(Box::from_raw(boxed));
        UnregisterClassW(class_name.as_ptr(), instance);
    }

    const MUTEX_NAME: &str = "Local\\ToolsNest.Instance.v1";
    const SHOW_EVENT_NAME: &str = "Local\\ToolsNest.Show.v1";
    const ERROR_ALREADY_EXISTS: u32 = 183;

    /// Returns false when another instance is already running; asks it to show its window.
    pub fn acquire_single_instance() -> bool {
        unsafe {
            let name = wide(MUTEX_NAME);
            let handle = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
            if handle.is_null() {
                return true;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let event_name = wide(SHOW_EVENT_NAME);
                let event = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
                if !event.is_null() {
                    SetEvent(event);
                    CloseHandle(event);
                }
                return false;
            }
            let event_name = wide(SHOW_EVENT_NAME);
            let _event = CreateEventW(std::ptr::null_mut(), 0, 0, event_name.as_ptr());
            // Both handles intentionally live for the process lifetime.
            true
        }
    }

    pub fn show_requested() -> bool {
        unsafe {
            let event_name = wide(SHOW_EVENT_NAME);
            let event = OpenEventW(SYNCHRONIZE, 0, event_name.as_ptr());
            if event.is_null() {
                return false;
            }
            let result = WaitForSingleObject(event, 0) == WAIT_OBJECT_0;
            CloseHandle(event);
            result
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::TrayEvent;

    pub struct Tray {
        monitoring: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Tray {
        pub fn spawn(monitoring: bool) -> Option<Self> {
            Some(Self { monitoring: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(monitoring)) })
        }
        pub fn set_monitoring(&self, on: bool) {
            self.monitoring.store(on, std::sync::atomic::Ordering::Relaxed);
        }
        pub fn drain(&self) -> Vec<TrayEvent> {
            Vec::new()
        }
    }

    pub fn acquire_single_instance() -> bool {
        true
    }

    pub fn show_requested() -> bool {
        false
    }
}

pub use platform::Tray;

pub fn acquire_single_instance() -> bool {
    platform::acquire_single_instance()
}

pub fn show_requested() -> bool {
    platform::show_requested()
}
