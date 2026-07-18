/// Options panel with full folder/disk selection and text search settings

use eframe::egui::{self, Color32, RichText};
use crate::file_seeker::gui::app::EverythingApp;
use std::path::PathBuf;
use std::net::ToSocketAddrs;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptionsTab {
    General, Search, Results, QuickShare, Advanced, Help,
}

pub fn render_options(ui: &mut egui::Ui, app: &mut EverythingApp) {
    ui.heading("文件检索助手 - 选项");
    ui.separator();
    egui::SidePanel::left("opt_tabs").resizable(false).min_width(120.0).show_inside(ui, |ui| {
        ui.vertical(|ui| {
            let tabs = [
                ("通用", OptionsTab::General),
                ("搜索", OptionsTab::Search),
                ("结果", OptionsTab::Results),
                ("共享", OptionsTab::QuickShare),
                ("高级", OptionsTab::Advanced),
                ("帮助", OptionsTab::Help),
            ];
            for (name, tab) in &tabs {
                let sel = *tab == app.current_tab;
                let mut btn = egui::Button::new(RichText::new(*name).size(12.0));
                if sel { btn = btn.fill(Color32::from_rgb(200, 210, 240)); }
                if ui.add_sized([115.0, 28.0], btn).clicked() { app.current_tab = *tab; }
            }
            ui.separator();
            if ui.button("关闭").clicked() { app.show_options = false; }
        });
    });
    egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            match app.current_tab {
                OptionsTab::General => render_general(ui, app),
                OptionsTab::Search => render_search(ui, app),
                OptionsTab::Results => render_results_tab(ui, app),
                OptionsTab::QuickShare => render_quickshare(ui, app),
                OptionsTab::Advanced => render_advanced(ui, app),
                OptionsTab::Help => render_help(ui, app),
            }
        });
    });
}

fn render_general(ui: &mut egui::Ui, app: &mut EverythingApp) {
    ui.heading("通用设置");
    ui.separator();
    if ui.checkbox(&mut app.auto_start_enabled, "开机自启动").changed() { app.toggle_auto_start(); }
    ui.checkbox(&mut app.realtime_monitoring, "实时监控文件变化");
    ui.checkbox(&mut app.run_in_background, "关闭窗口后在后台运行");
    ui.checkbox(&mut app.search_as_you_type, "边输入边搜索 (建议关闭)");
    ui.separator();
    ui.label("自动增量索引间隔:");
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(&mut app.monitor_interval_secs, 60..=3600).text("秒"));
        if ui.button("默认").clicked() { app.monitor_interval_secs = 300; }
    });
    ui.separator();
    ui.label("快捷键:");
    for (k, v) in &[("Enter", "打开"), ("F2", "重命名"), ("Del", "删除"), ("Ctrl+C", "复制"), ("↑↓", "导航"), ("Esc", "清空")] {
        ui.label(format!("  {}: {}", k, v));
    }
}

fn render_search(ui: &mut egui::Ui, app: &mut EverythingApp) {
    ui.heading("搜索设置");
    ui.separator();
    ui.checkbox(&mut app.search_options.match_case, "匹配大小写");
    ui.checkbox(&mut app.search_options.match_whole_word, "匹配整个词");
    ui.checkbox(&mut app.search_options.match_path, "匹配完整路径");
    ui.checkbox(&mut app.search_options.regex, "启用正则表达式");
    ui.separator();
    ui.label("最大结果数:");
    ui.add(egui::Slider::new(&mut app.search_options.max_results, 10..=1000000));
}

fn render_results_tab(ui: &mut egui::Ui, app: &mut EverythingApp) {
    ui.heading("结果与视图");
    ui.separator();
    ui.checkbox(&mut app.detail_view, "列表视图");
    ui.checkbox(&mut app.show_full_row, "整行选中");
    ui.checkbox(&mut app.fixed_column_width, "固定列宽(防止自动拉伸)");
    ui.separator();
    ui.label("缩略图大小:");
    ui.add(egui::Slider::new(&mut app.thumbnail_size, 32..=512));
}

fn render_quickshare(ui: &mut egui::Ui, app: &mut EverythingApp) {
    ui.heading("快速共享");
    ui.separator();
    ui.label("将索引文件夹通过 HTTP/FTP 共享到局域网");
    ui.label(RichText::new("共享内容: 索引路径管理中添加的文件夹/磁盘").size(11.0).color(Color32::GRAY));
    ui.separator();

    // HTTP Server
    ui.heading("🌐 HTTP 服务器");
    ui.horizontal(|ui| {
        ui.label(if app.http_enabled { "🟢 运行中" } else { "🔴 已停止" });
        if ui.button(if app.http_enabled { "⏹ 停止" } else { "▶ 启动" }).clicked() {
            app.http_enabled = !app.http_enabled;
            if app.http_enabled {
                app.status_message = "HTTP 服务器启动中...".to_string();
                let engine = app.engine.clone();
                let port = app.http_port;
                std::thread::spawn(move || {
                    let mut server = crate::file_seeker::http_server::HttpServer::new(engine).with_port(port);
                    let _ = server.start();
                });
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("端口:"); ui.add(egui::Slider::new(&mut app.http_port, 1024..=65535).text(""));
        if app.http_enabled {
            let local_ip = get_local_ip();
            let url = format!("http://{}:{}", local_ip, app.http_port);
            let url2 = format!("http://localhost:{}", app.http_port);
            let u1 = url.clone();
            let u2 = url2.clone();
            if ui.button(url).clicked() { ui.ctx().copy_text(u1); }
            if ui.button(url2).clicked() { ui.ctx().copy_text(u2); }
        }
    });
    ui.horizontal(|ui| {
        ui.label("用户名:");
        ui.text_edit_singleline(&mut app.http_username);
    });
    ui.horizontal(|ui| {
        ui.label("密码:");
        ui.text_edit_singleline(&mut app.http_password);
    });
    ui.separator();

    // FTP Server
    ui.heading("FTP 服务器");
    ui.horizontal(|ui| {
        ui.label(if app.ftp_enabled { "[运行中]" } else { "[已停止]" });
        if ui.button(if app.ftp_enabled { "⏹ 停止" } else { "▶ 启动" }).clicked() {
            app.ftp_enabled = !app.ftp_enabled;
            if app.ftp_enabled {
                let port = app.ftp_port;
                app.status_message = format!("FTP 服务器启动中（端口 {}）...", port);
                let u = app.ftp_username.clone();
                let p = app.ftp_password.clone();
                let dirs: Vec<PathBuf> = app.index_paths.iter().map(PathBuf::from).collect();
                std::thread::spawn(move || {
                    let mut server = crate::file_seeker::ftp::FtpServer::new()
                        .with_port(port)
                        .with_auth(u, p)
                        .with_root(dirs);
                    let _ = server.start();
                });
            } else {
                app.status_message = "FTP 服务器已停止".to_string();
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("端口:"); ui.add(egui::Slider::new(&mut app.ftp_port, 1024..=65535).text(""));
        if app.ftp_enabled {
            let local_ip = get_local_ip();
            let url = format!("ftp://{}:{}", local_ip, app.ftp_port);
            let u1 = url.clone();
            if ui.button(url).clicked() { ui.ctx().copy_text(u1); }
        }
    });
    ui.horizontal(|ui| {
        ui.label("用户名:");
        ui.text_edit_singleline(&mut app.ftp_username);
    });
    ui.horizontal(|ui| {
        ui.label("密码:");
        ui.text_edit_singleline(&mut app.ftp_password);
    });
}

fn render_advanced(ui: &mut egui::Ui, app: &mut EverythingApp) {
    ui.heading("索引路径管理");
    ui.separator();
    ui.label(RichText::new("添加需要建立索引的文件夹或磁盘").size(11.0).color(Color32::GRAY));
    let paths = app.index_paths.clone();
    for p in &paths {
        ui.horizontal(|ui| {
            let icon = if p.len() <= 3 && p.contains(':') { "💾" } else { "📁" };
            ui.label(format!("{} {}", icon, p));
            if ui.small_button("删除").clicked() { app.index_paths.retain(|x| x != p); }
        });
    }
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut app.new_folder_text);
        if ui.button("手动添加").clicked() && !app.new_folder_text.is_empty() {
            let p = app.new_folder_text.trim().to_string();
            if !app.index_paths.contains(&p) { app.index_paths.push(p); }
            app.new_folder_text.clear();
        }
        if ui.button("📁 选择文件夹").clicked() {
            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                let s = p.to_string_lossy().to_string();
                if !app.index_paths.contains(&s) { app.index_paths.push(s); }
            }
        }
        if ui.button("💾 选择磁盘").clicked() {
            #[cfg(windows)] {
                for c in 'A'..='Z' {
                    let d = format!("{}:\\", c);
                    if std::path::Path::new(&d).exists() && !app.index_paths.contains(&d) {
                        app.index_paths.push(d);
                    }
                }
            }
        }
        if ui.button("🔄 重建索引").clicked() { app.build_index(); }
    });
    ui.separator();

    ui.heading("索引状态");
    ui.label(format!("  文件: {}  文件夹: {}  总计: {}", app.engine.total_file_count(), app.engine.total_folder_count(), app.engine.total_entries()));
    ui.separator();

    ui.heading("排除设置");
    ui.checkbox(&mut app.exclude_hidden, "排除隐藏文件");
    ui.checkbox(&mut app.exclude_system, "排除系统文件");
    ui.label("排除模式:");
    ui.text_edit_multiline(&mut app.exclude_patterns);
    ui.separator();

    ui.heading("搜索历史");
    ui.label(format!("  {} 条", app.search_history.len()));
    if !app.search_history.is_empty() && ui.button("清空历史").clicked() { app.search_history.clear(); }
}

fn render_help(ui: &mut egui::Ui, _app: &mut EverythingApp) {
    ui.heading("[帮助] 文件检索助手 使用说明");
    ui.separator();
    ui.label("[文件搜索] - 搜索文件名和路径");
    ui.label("  - 输入关键字，按 Enter 搜索");
    ui.label("  - 支持通配符 * (任意字符), ? (单个字符)");
    ui.label("  - 支持运算符 AND(空格), OR(|), NOT(!)");
    ui.label("  - 支持函数: size: dm: dc: ext: attrib: runcount:");
    ui.label("  - 例如: size:>1mb dm:today *.rs");
    ui.separator();
    ui.label("[结果列表]");
    ui.label("  - 双击: 打开文件");
    ui.label("  - 右键: 打开/复制/重命名/属性菜单");
    ui.label("  - 拖动列边界: 调整列宽");
    ui.label("  - 点击列表头: 排序");
    ui.separator();
    ui.label("[快速共享]");
    ui.label("  - HTTP: 通过浏览器访问文件 (默认端口8080)");
    ui.label("  - FTP: 快速文件传输 (默认端口21)");
    ui.label("  - 默认用户名/密码: user/user");
    ui.label("  - 共享内容: 高级中添加的所有索引路径");
    ui.separator();
    ui.label("[快捷键]");
    ui.label("  Enter=打开  F2=重命名  Del=删除  Ctrl+C=复制");
    ui.separator();
    ui.label("版本: 1.0.0 | 作者: file-seeker");
}

fn get_local_ip() -> String {
    // Simple approach: get hostname and resolve
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    if let Ok(addrs) = (hostname.as_str(), 0u16).to_socket_addrs() {
        for addr in addrs {
            let ip = addr.ip().to_string();
            if !ip.starts_with("127.") && !ip.starts_with("169.") {
                return ip;
            }
        }
    }
    "127.0.0.1".to_string()
}