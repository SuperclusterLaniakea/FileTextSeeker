#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod file_seeker;
mod doc_searcher;
mod file_lister_gui;
mod code_merger;
mod merged_app;

use merged_app::MergedApp;

/// 从 .ico 文件加载图标数据
fn load_icon_from_ico(path: &str) -> Result<egui::IconData, Box<dyn std::error::Error>> {
    let ico_data = std::fs::read(path)?;
    let icon_dir = ico::IconDir::read(std::io::Cursor::new(&ico_data))?;
    let entry = icon_dir.entries().iter()
        .min_by_key(|e| (e.width() as i32 - 32).abs() + (e.height() as i32 - 32).abs())
        .ok_or("No icon entries")?;
    let icon_img = entry.decode()?;
    let rgba = icon_img.rgba_data().to_vec();
    Ok(egui::IconData {
        rgba,
        width: icon_img.width(),
        height: icon_img.height(),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 800.0])
        .with_title("超级检索工具");

    // 加载程序图标
    let mut icon_loaded = false;
    let icon_paths = ["icon.ico", "../icon.ico", "../../icon.ico"];
    for p in &icon_paths {
        if let Ok(icon_data) = load_icon_from_ico(p) {
            viewport = viewport.with_icon(icon_data);
            icon_loaded = true;
            break;
        }
    }
    // 尝试相对于 exe 所在目录
    if !icon_loaded {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                for p in &["icon.ico", "../icon.ico", "../../icon.ico"] {
                    let full = exe_dir.join(p);
                    if let Ok(icon_data) = load_icon_from_ico(&full.to_string_lossy()) {
                        viewport = viewport.with_icon(icon_data);
                        break;
                    }
                }
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "SuperSearcher",
        options,
        Box::new(|cc| {
            let start_minimized = std::env::args().any(|a| a == "--minimized");
            Box::new(MergedApp::new(cc, start_minimized))
        }),
    )?;

    Ok(())
}
