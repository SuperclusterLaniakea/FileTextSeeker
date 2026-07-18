/// Auto-start management - Windows registry Run key
///
/// Uses HKCU\Software\Microsoft\Windows\CurrentVersion\Run

use winreg::enums::*;
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "文件检索助手";  // ← 修复：补上结束引号，修正中文

/// Check if auto-start is enabled
pub fn is_auto_start_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run_key) = hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_READ) {
        run_key.get_value::<String, _>(APP_NAME).is_ok()
    } else {
        false
    }
}

/// Enable auto-start (add to registry Run key with --minimized flag)
pub fn enable_auto_start() -> Result<(), String> {
    let exe_path = get_exe_path()?;
    // Add --minimized flag to start minimized to tray
    let cmd_line = format!("\"{}\" --minimized", exe_path);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = hkcu
        .open_subkey_with_flags(RUN_KEY_PATH, KEY_SET_VALUE)
        .map_err(|e| format!("Cannot open registry Run key: {}", e))?;
    run_key
        .set_value(APP_NAME, &cmd_line)
        .map_err(|e| format!("Cannot set registry value: {}", e))?;
    Ok(())
}

/// Disable auto-start (remove from registry Run key)
pub fn disable_auto_start() -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = hkcu
        .open_subkey_with_flags(RUN_KEY_PATH, KEY_SET_VALUE)
        .map_err(|e| format!("Cannot open registry Run key: {}", e))?;
    run_key
        .delete_value(APP_NAME)
        .map_err(|e| format!("Cannot delete registry value: {}", e))?;
    Ok(())
}

/// Toggle auto-start
pub fn toggle_auto_start() -> Result<bool, String> {
    if is_auto_start_enabled() {
        disable_auto_start()?;
        Ok(false)
    } else {
        enable_auto_start()?;
        Ok(true)
    }
}

fn get_exe_path() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Cannot get executable path: {}", e))
}