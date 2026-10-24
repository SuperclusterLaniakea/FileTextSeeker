#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod cleaner;
mod config;
mod docs;
mod duplicates;
mod lister;
mod merge;
mod monitor;
mod rename;
mod search;
mod sync;
mod tabs;
mod theme;
mod tray;
mod util;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

fn load_window_icon() -> Option<Arc<eframe::egui::IconData>> {
    let bytes = include_bytes!("../icon.ico");
    let dir = ico::IconDir::read(std::io::Cursor::new(bytes.as_slice())).ok()?;
    let entry = dir
        .entries()
        .iter()
        .max_by_key(|e| e.width() as u64 * e.height() as u64)?;
    let image = entry.decode().ok()?;
    Some(Arc::new(eframe::egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: image.width(),
        height: image.height(),
    }))
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let mut config_dir: Option<PathBuf> = None;
    let mut start_minimized = false;
    let mut screenshot: Option<PathBuf> = None;
    let mut initial_tab: Option<app::Tab> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config-dir" if i + 1 < args.len() => {
                config_dir = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--screenshot" if i + 1 < args.len() => {
                screenshot = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--tab" if i + 1 < args.len() => {
                initial_tab = Some(app::Tab::from_id(&args[i + 1]));
                i += 1;
            }
            "--minimized" => start_minimized = true,
            _ => {}
        }
        i += 1;
    }

    if screenshot.is_none() && !tray::acquire_single_instance() {
        return Ok(());
    }

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("ToolsNest · 全能文件工具箱")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([1000.0, 640.0])
        .with_app_id("ToolsNest");
    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: true,
            ..Default::default()
        },
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "ToolsNest · 全能文件工具箱",
        options,
        Box::new(move |cc| {
            let app = app::App::new(cc, config_dir, start_minimized, screenshot, initial_tab);
            if start_minimized && app.has_tray() {
                cc.egui_ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Visible(false));
            }
            Ok(Box::new(app))
        }),
    )
}
