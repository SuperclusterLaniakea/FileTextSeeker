/// 文件检索助手 GUI Application - 重构版，适配合并应用
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};
use std::sync::{Arc, RwLock};
use std::path::PathBuf;
use std::time::Instant;

use crate::file_seeker::engine::Engine;
use crate::file_seeker::types::{FileEntry, SearchOptions, SortField, SortOrder, SortSpec, IndexSource, IndexType};
use crate::file_seeker::config::Config;
use crate::file_seeker::gui::results_panel::{self, ResultAction};
use crate::file_seeker::gui::options_panel::{self, OptionsTab};
use crate::file_seeker::autostart;

// 为减少代码量，保持原有 use 语句，但路径调整为 crate::file_seeker::

pub struct EverythingApp {
    pub engine: Arc<Engine>,
    pub config: Arc<RwLock<Config>>,
    pub search_text: String,
    pub results: Vec<FileEntry>,
    pub search_options: SearchOptions,
    pub selected_index: Option<usize>,
    pub status_message: String,
    pub show_options: bool,
    pub initialized: bool,

    pub search_history: Vec<String>,
    pub detail_view: bool,
    pub thumbnail_size: u32,
    pub sort_field: SortField,
    pub sort_order: SortOrder,
    pub files_only: bool,
    pub folders_only: bool,
    pub current_tab: OptionsTab,
    pub index_paths: Vec<String>,
    pub indexing_in_progress: bool,
    pub search_as_you_type: bool,
    last_search: String,
    last_displayed_text: String,
    pub monitor_interval_secs: u64,
    last_auto_index: Instant,
    pub first_run: bool,
    pub show_first_run_dialog: bool,
    indexed_paths_snapshot: Vec<String>,
    pub minimized: bool,
    pub start_minimized: bool,
    pub run_in_background: bool,
    pub auto_start_enabled: bool,
    pub file_watcher: crate::file_seeker::watcher::FileWatcher,
    pub realtime_monitoring: bool,
    index_progress_rx: Option<std::sync::mpsc::Receiver<crate::file_seeker::engine::IndexProgress>>,
    pub run_as_admin: bool,
    pub service_enabled: bool,
    pub exclude_hidden: bool,
    pub exclude_system: bool,
    pub exclude_patterns: String,
    pub exclude_paths: String,
    pub file_lists: Vec<String>,
    pub http_enabled: bool,
    pub http_port: u16,
    pub http_username: String,
    pub http_password: String,
    pub ftp_enabled: bool,
    pub ftp_port: u16,
    pub ftp_username: String,
    pub ftp_password: String,
    pub fast_ascii_search: bool,
    pub auto_match_path: bool,
    pub wildcard_full_match: bool,
    pub index_dates: bool,
    pub index_attributes: bool,
    pub show_full_row: bool,
    pub fixed_column_width: bool,
    pub new_folder_text: String,
    pub new_filelist_text: String,
    pub column_widths: [f32; 6],
    pub active_tab: Tab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    FileSearch,
}

impl EverythingApp {
    pub fn new_with_engine(engine: Arc<Engine>, start_minimized: bool) -> Self {
        let auto_start = autostart::is_auto_start_enabled();

        let mut app = Self {
            engine: engine.clone(),
            config: Arc::new(RwLock::new(Config::default())),
            search_text: String::new(),
            results: Vec::new(),
            search_options: SearchOptions::default(),
            selected_index: None,
            status_message: "准备就绪。输入关键字开始搜索。".to_string(),
            show_options: false,
            initialized: false,
            search_history: Vec::new(),
            detail_view: true,
            thumbnail_size: 256,
            sort_field: SortField::Name,
            sort_order: SortOrder::Ascending,
            files_only: false,
            folders_only: false,
            current_tab: OptionsTab::General,
            index_paths: Vec::new(),
            indexing_in_progress: false,
            search_as_you_type: false,
            last_search: String::new(),
            last_displayed_text: String::new(),
            monitor_interval_secs: 300,
            last_auto_index: Instant::now(),
            first_run: false,
            show_first_run_dialog: false,
            indexed_paths_snapshot: Vec::new(),
            minimized: start_minimized,
            start_minimized,
            run_in_background: true,
            auto_start_enabled: auto_start,
            file_watcher: crate::file_seeker::watcher::FileWatcher::new(),
            realtime_monitoring: false,
            index_progress_rx: None,
            run_as_admin: false,
            service_enabled: false,
            exclude_hidden: true,
            exclude_system: true,
            exclude_patterns: "*.tmp;*.log;*.bak".to_string(),
            exclude_paths: "$Recycle.Bin;System Volume Information".to_string(),
            file_lists: Vec::new(),
            http_enabled: false,
            http_port: 8080,
            http_username: String::new(),
            http_password: String::new(),
            ftp_enabled: false,
            ftp_port: 21,
            ftp_username: String::new(),
            ftp_password: String::new(),
            fast_ascii_search: true,
            auto_match_path: true,
            wildcard_full_match: false,
            index_dates: true,
            index_attributes: false,
            show_full_row: true,
            fixed_column_width: true,
            new_folder_text: String::new(),
            new_filelist_text: String::new(),
            column_widths: [220.0, 280.0, 80.0, 60.0, 140.0, 140.0],
            active_tab: Tab::FileSearch,
        };

        // 默认索引路径
        if cfg!(windows) {
            if let Ok(profile) = std::env::var("USERPROFILE") {
                app.index_paths.push(profile);
            }
        } else if let Ok(home) = std::env::var("HOME") {
            app.index_paths.push(home);
        }

        app
    }

    pub fn build_index(&mut self) {
        self.indexing_in_progress = true;
        self.status_message = "正在后台建立索引... (UI 不会卡死)".to_string();

        {
            let mut sources = self.engine.index_sources.write().unwrap();
            sources.clear();
            for path in &self.index_paths {
                sources.push(IndexSource {
                    index_type: IndexType::Folder,
                    path: PathBuf::from(path),
                    enabled: true,
                    label: Some(path.clone()),
                });
            }
        }

        let rx = self.engine.build_index_async();
        self.index_progress_rx = Some(rx);
        self.status_message = "索引任务已启动，可在后台进行...".to_string();
    }

    pub fn check_index_progress(&mut self) {
        use crate::file_seeker::engine::IndexProgress;
        if let Some(rx) = &self.index_progress_rx {
            while let Ok(progress) = rx.try_recv() {
                match progress {
                    IndexProgress::Started(msg) => {
                        self.status_message = msg;
                    }
                    IndexProgress::Progress(msg, _pct) => {
                        self.status_message = msg;
                    }
                    IndexProgress::FileCount(_count) => {}
                    IndexProgress::Complete(files, folders) => {
                        self.indexing_in_progress = false;
                        self.status_message = format!("索引完成。{} 个文件 {} 个文件夹", files, folders);
                    }
                    IndexProgress::Error(e) => {
                        self.indexing_in_progress = false;
                        self.status_message = format!("索引错误: {}", e);
                    }
                }
            }
        }
    }

    pub fn execute_search(&mut self) {
        let query = self.search_text.trim().to_string();
        if query == self.last_search && !self.results.is_empty() {
            return;
        }
        self.last_search = query.clone();

        if query.is_empty() {
            self.results = Vec::new();
            self.status_message = "输入关键字开始搜索".to_string();
            return;
        }

        self.search_options.files_only = self.files_only;
        self.search_options.folders_only = self.folders_only;

        if !query.is_empty() && !self.search_history.contains(&query) {
            self.search_history.insert(0, query.clone());
            if self.search_history.len() > 100 {
                self.search_history.pop();
            }
        }

        match self.engine.search(&query, &self.search_options) {
            Ok(mut results) => {
                crate::file_seeker::engine::sorter::sort_entries(&mut results, &SortSpec {
                    field: self.sort_field,
                    order: self.sort_order,
                });
                let total = results.len();
                self.results = results;
                let files = self.results.iter().filter(|r| !r.is_directory).count();
                let folders = self.results.iter().filter(|r| r.is_directory).count();
                self.status_message = format!("✓ {} 个结果 {} 文件, {} 文件夹", total, files, folders);
            }
            Err(e) => self.status_message = format!("✓ 搜索错误: {}", e),
        }
    }

    pub fn open_result(&mut self, index: usize) {
        if let Some(entry) = self.results.get(index) {
            let path_str = entry.full_path.to_string_lossy().to_string();
            #[cfg(target_os = "windows")]
            {
                if entry.is_directory {
                    let _ = std::process::Command::new("explorer").arg(&path_str).spawn();
                } else {
                    let _ = std::process::Command::new("cmd").args(&["/c", "start", "", &path_str]).spawn();
                }
            }
            let _ = self.engine.increment_run_count(&path_str);
        }
    }

    pub fn open_path(&mut self, index: usize) {
        if let Some(entry) = self.results.get(index) {
            let path_str = entry.parent_path.to_string_lossy().to_string();
            #[cfg(target_os = "windows")]
            { let _ = std::process::Command::new("explorer").arg(&path_str).spawn(); }
        }
    }

    pub fn open_properties(&self, index: usize) {
        if let Some(entry) = self.results.get(index) {
            let path = entry.full_path.to_string_lossy().to_string();
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                use winapi::um::shellapi::ShellExecuteW;
                let wide_path: Vec<u16> = std::ffi::OsStr::new(&path)
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                unsafe {
                    ShellExecuteW(
                        std::ptr::null_mut(),
                        "properties\0".as_ptr() as *const u16,
                        wide_path.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        1,
                    );
                }
            }
        }
    }

    pub fn copy_path(&self, index: usize, ctx: &egui::Context) {
        if let Some(entry) = self.results.get(index) {
            let path = entry.full_path.to_string_lossy().to_string();
            ctx.copy_text(path);
        }
    }

    pub fn copy_name(&self, index: usize, ctx: &egui::Context) {
        if let Some(entry) = self.results.get(index) {
            ctx.copy_text(entry.file_name.clone());
        }
    }

    pub fn toggle_auto_start(&mut self) {
        if self.auto_start_enabled {
            let _ = autostart::enable_auto_start();
        } else {
            let _ = autostart::disable_auto_start();
        }
    }

    pub fn save_state(&self) {
        #[derive(serde::Serialize)]
        struct AppState {
            index_paths: Vec<String>,
            search_history: Vec<String>,
            exclude_hidden: bool,
            exclude_system: bool,
            exclude_patterns: String,
            exclude_paths: String,
            file_lists: Vec<String>,
            http_enabled: bool,
            http_port: u16,
            http_username: String,
            http_password: String,
            ftp_enabled: bool,
            ftp_port: u16,
            ftp_username: String,
            ftp_password: String,
            monitor_interval_secs: u64,
            realtime_monitoring: bool,
            search_options_match_case: bool,
            search_options_whole_word: bool,
            search_options_match_path: bool,
            search_options_regex: bool,
            thumbnail_size: u32,
            detail_view: bool,
        }

        let state = AppState {
            index_paths: self.index_paths.clone(),
            search_history: self.search_history.clone(),
            exclude_hidden: self.exclude_hidden,
            exclude_system: self.exclude_system,
            exclude_patterns: self.exclude_patterns.clone(),
            exclude_paths: self.exclude_paths.clone(),
            file_lists: self.file_lists.clone(),
            http_enabled: self.http_enabled,
            http_port: self.http_port,
            http_username: self.http_username.clone(),
            http_password: self.http_password.clone(),
            ftp_enabled: self.ftp_enabled,
            ftp_port: self.ftp_port,
            ftp_username: self.ftp_username.clone(),
            ftp_password: self.ftp_password.clone(),
            monitor_interval_secs: self.monitor_interval_secs,
            realtime_monitoring: self.realtime_monitoring,
            search_options_match_case: self.search_options.match_case,
            search_options_whole_word: self.search_options.match_whole_word,
            search_options_match_path: self.search_options.match_path,
            search_options_regex: self.search_options.regex,
            thumbnail_size: self.thumbnail_size,
            detail_view: self.detail_view,
        };

        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let path = get_state_file_path();
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn load_state(&mut self) {
        #[derive(serde::Deserialize)]
        struct AppState {
            index_paths: Vec<String>,
            search_history: Vec<String>,
            exclude_hidden: bool,
            exclude_system: bool,
            exclude_patterns: String,
            exclude_paths: String,
            file_lists: Vec<String>,
            http_enabled: bool,
            http_port: u16,
            http_username: String,
            http_password: String,
            ftp_enabled: bool,
            ftp_port: u16,
            ftp_username: String,
            ftp_password: String,
            monitor_interval_secs: u64,
            realtime_monitoring: bool,
            search_options_match_case: bool,
            search_options_whole_word: bool,
            search_options_match_path: bool,
            search_options_regex: bool,
            thumbnail_size: u32,
            detail_view: bool,
        }

        let path = get_state_file_path();
        if !path.exists() {
            self.show_first_run_dialog = true;
            return;
        }

        if let Ok(json) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<AppState>(&json) {
                self.index_paths = state.index_paths;
                self.search_history = state.search_history;
                self.exclude_hidden = state.exclude_hidden;
                self.exclude_system = state.exclude_system;
                self.exclude_patterns = state.exclude_patterns;
                self.exclude_paths = state.exclude_paths;
                self.file_lists = state.file_lists;
                self.http_enabled = state.http_enabled;
                self.http_port = state.http_port;
                self.ftp_enabled = state.ftp_enabled;
                self.ftp_port = state.ftp_port;
                self.monitor_interval_secs = state.monitor_interval_secs;
                self.realtime_monitoring = state.realtime_monitoring;
                self.search_options.match_case = state.search_options_match_case;
                self.search_options.match_whole_word = state.search_options_whole_word;
                self.search_options.match_path = state.search_options_match_path;
                self.search_options.regex = state.search_options_regex;
                self.thumbnail_size = state.thumbnail_size;
                self.detail_view = state.detail_view;
            }
        }
    }

    // ========== 重构后的渲染入口，替代原 update 方法 ==========
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // 窗口最小化状态处理
        let is_minimized = ctx.input(|i| i.viewport().minimized);
        if is_minimized == Some(false) || is_minimized.is_none() {
            self.minimized = false;
        }

        // ---------- 首次初始化 ----------
        if !self.initialized {
            self.initialized = true;

            // 字体
            let mut fonts = FontDefinitions::default();
            let mut loaded_font = None;
            if cfg!(windows) {
                let font_paths = [
                    "C:\\Windows\\Fonts\\msyh.ttf",
                    "C:\\Windows\\Fonts\\msyh.ttc",
                    "C:\\Windows\\Fonts\\simsun.ttc",
                ];
                for path in &font_paths {
                    if std::path::Path::new(path).exists() {
                        if let Ok(data) = std::fs::read(path) {
                            let name = format!("cn_font_{}", std::path::Path::new(path)
                                .file_stem().unwrap().to_string_lossy());
                            fonts.font_data.insert(name.clone(), FontData::from_owned(data));
                            loaded_font = Some(name);
                            break;
                        }
                    }
                }
            }
            if let Some(font) = loaded_font {
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.insert(0, font.clone());
                }
                if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                    family.insert(0, font);
                }
            }
            ctx.set_fonts(fonts);

            // 样式 —— 跟随系统明暗主题
            let mut style = (*ctx.style()).clone();
            let is_dark = style.visuals.dark_mode;
            let text_color = if is_dark { Color32::from_rgb(220, 220, 220) } else { Color32::from_rgb(35, 35, 35) };
            let bg_color = if is_dark { Color32::from_rgb(30, 30, 30) } else { Color32::from_rgb(245, 245, 248) };
            let faint_bg = if is_dark { Color32::from_rgb(40, 40, 42) } else { Color32::from_rgb(248, 248, 252) };
            let inactive_bg = if is_dark { Color32::from_rgb(45, 45, 48) } else { Color32::from_rgb(240, 240, 245) };
            let border_color = if is_dark { Color32::from_rgb(60, 60, 65) } else { Color32::from_rgb(210, 210, 215) };
            let hyperlink = if is_dark { Color32::from_rgb(80, 160, 255) } else { Color32::from_rgb(0, 100, 180) };
            let selection_bg = Color32::from_rgb(0, 120, 212);
            style.visuals.window_rounding = 6.0.into();
            style.visuals.window_shadow = egui::epaint::Shadow {
                offset: [2.0, 4.0].into(),
                blur: 12.0,
                spread: 0.0,
                color: Color32::from_black_alpha(40),
            };
            style.visuals.selection.bg_fill = selection_bg;
            style.visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(0, 90, 180));
            style.visuals.widgets.noninteractive.bg_fill = bg_color;
            style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border_color);
            style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text_color);
            style.visuals.widgets.inactive.bg_fill = inactive_bg;
            style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.5, text_color);
            style.visuals.widgets.active.bg_fill = selection_bg;
            style.visuals.widgets.active.fg_stroke = egui::Stroke::new(2.0, Color32::WHITE);
            style.visuals.widgets.hovered.bg_fill = selection_bg;
            style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, Color32::WHITE);
            style.visuals.hyperlink_color = hyperlink;
            style.visuals.faint_bg_color = faint_bg;
            style.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
            style.spacing.button_padding = egui::Vec2::new(8.0, 4.0);
            ctx.set_style(style);

            self.load_state();

            if !self.index_paths.is_empty() && self.engine.total_entries() == 0 {
                self.build_index();
                self.status_message = "正在自动建立索引...".to_string();
            } else {
                self.status_message = "输入关键字开始搜索".to_string();
            }
        }

        // ---------- 首次运行对话框 ----------
        if self.show_first_run_dialog {
            egui::Window::new("👋 欢迎使用 文件检索助手")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.label(RichText::new("欢迎使用 文件检索助手 文件搜索工具！").size(16.0).strong());
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label("开始使用前，请先添加需要建立索引的文件夹：");
                    ui.add_space(4.0);
                    ui.label("• 点击「选择文件夹」选择需要搜索的目录");
                    ui.label("• 或点击「💾 选择磁盘」添加整个磁盘");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("选择文件夹").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                let path_str = path.to_string_lossy().to_string();
                                if !self.index_paths.contains(&path_str) {
                                    self.index_paths.push(path_str);
                                }
                                self.show_first_run_dialog = false;
                            }
                        }
                        if ui.button("💾 选择磁盘").clicked() {
                            #[cfg(windows)]
                            {
                                let drives = get_windows_drives();
                                for drive in drives {
                                    if !self.index_paths.contains(&drive) {
                                        self.index_paths.push(drive);
                                    }
                                }
                                self.show_first_run_dialog = false;
                            }
                        }
                        if ui.button("稍后再说").clicked() {
                            self.show_first_run_dialog = false;
                        }
                    });
                    ui.add_space(4.0);
                    if !self.index_paths.is_empty() {
                        if ui.button("✓ 开始索引").clicked() {
                            self.build_index();
                            self.show_first_run_dialog = false;
                        }
                    }
                });
        }

        // 索引进度
        self.check_index_progress();

        // 索引路径一致性检查
        if !self.indexed_paths_snapshot.is_empty() && self.engine.total_entries() > 0 {
            let current_paths: Vec<String> = self.index_paths.clone();
            if current_paths != self.indexed_paths_snapshot {
                self.status_message = "检测到索引路径变化，正在重建索引...".to_string();
                self.build_index();
                self.indexed_paths_snapshot = current_paths;
            }
        } else if self.engine.total_entries() > 0 {
            self.indexed_paths_snapshot = self.index_paths.clone();
        }

        // 实时监控
        if self.realtime_monitoring && !self.index_paths.is_empty() {
            if !self.file_watcher.is_running() {
                let paths: Vec<PathBuf> = self.index_paths.iter().map(|p| p.into()).collect();
                if let Err(e) = self.file_watcher.start_watching(paths) {
                    self.status_message = format!("文件监控启动失败: {}", e);
                    self.realtime_monitoring = false;
                } else {
                    self.status_message = "文件监控已启动(增量更新)".to_string();
                }
            }

            while let Some(event) = self.file_watcher.poll_event() {
                match event {
                    crate::file_seeker::watcher::WatcherEvent::FileAdded(path) => {
                        if let Some(entry) = crate::file_seeker::engine::indexer::index_folder_fast(&path)
                            .ok().and_then(|mut v| v.pop()) {
                            if let Ok(mut entries) = self.engine.entries.write() {
                                entries.push(entry);
                            }
                        }
                    }
                    crate::file_seeker::watcher::WatcherEvent::FileRemoved(path) => {
                        if let Ok(mut entries) = self.engine.entries.write() {
                            entries.retain(|e| e.full_path != path);
                        }
                    }
                    crate::file_seeker::watcher::WatcherEvent::FileModified(path) => {
                        if let Ok(mut entries) = self.engine.entries.write() {
                            entries.retain(|e| e.full_path != path);
                        }
                        if let Some(entry) = crate::file_seeker::engine::indexer::index_folder_fast(&path)
                            .ok().and_then(|mut v| v.pop()) {
                            if let Ok(mut entries) = self.engine.entries.write() {
                                entries.push(entry);
                            }
                        }
                    }
                    _ => {}
                }
            }

            let elapsed = self.last_auto_index.elapsed();
            let full_interval = std::time::Duration::from_secs(self.monitor_interval_secs * 6);
            if elapsed >= full_interval {
                self.status_message = "执行定期全量索引校验...".to_string();
                self.build_index();
                self.last_auto_index = Instant::now();
            }
        }

        // ==================== 搜索栏区域 (原 TopBottomPanel) ====================
        ui.horizontal(|ui| {
            if ui.button("设置").clicked() {
                self.show_options = !self.show_options;
            }

            let search_resp = ui.add_sized(
                [ui.available_width() - 200.0, 24.0],
                egui::TextEdit::singleline(&mut self.search_text)
                    .hint_text("搜索文件或文件夹... 支持: * ? | ! size: dm: ext: audio: pic:")
            );

            if search_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.last_search.clear();
                self.execute_search();
            }

            if ui.button("搜索").clicked() {
                self.last_search.clear();
                self.execute_search();
            }
            if ui.button("清空").clicked() {
                self.search_text.clear();
                self.last_search.clear();
                self.results.clear();
                self.status_message = "输入关键字开始搜索".to_string();
            }

            if !self.search_history.is_empty() {
                ui.menu_button("🕒", |ui| {
                    let items: Vec<String> = self.search_history.iter().take(25).cloned().collect();
                    for entry in &items {
                        let e = entry.clone();
                        if ui.button(&e).clicked() {
                            self.search_text = e;
                            self.last_search.clear();
                            self.execute_search();
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("清空历史").clicked() {
                        self.search_history.clear();
                    }
                });
            }
        });

        // 过滤选项行
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.files_only, "仅文件");
            if self.files_only { self.folders_only = false; }
            ui.checkbox(&mut self.folders_only, "仅文件夹");
            if self.folders_only { self.files_only = false; }

            ui.separator();

            ui.checkbox(&mut self.search_options.match_case, "大小写");
            ui.checkbox(&mut self.search_options.match_whole_word, "整词");
            ui.checkbox(&mut self.search_options.match_path, "匹配路径");
            ui.checkbox(&mut self.search_options.regex, "正则");

            ui.separator();

            ui.menu_button("快速过滤", |ui| {
                let macros = [
                    ("音频", "audio:"),
                    ("文档", "doc:"),
                    ("图片", "pic:"),
                    ("视频", "video:"),
                    ("压缩包", "zip:"),
                    ("可执行", "exe:"),
                    ("今天修改", "dm:today"),
                    ("昨天修改", "dm:yesterday"),
                    ("本周修改", "dm:thisweek"),
                    ("大文件(>100MB)", "size:>100mb"),
                    ("超大文件 (>1GB)", "size:>1gb"),
                ];
                for (label, q) in &macros {
                    if ui.button(*label).clicked() {
                        self.search_text = q.to_string();
                        self.last_search.clear();
                        self.last_displayed_text.clear();
                        self.execute_search();
                        ui.close_menu();
                    }
                }
            });

            if ui.button("重建索引").clicked() {
                self.last_search.clear();
                self.build_index();
            }

            if !self.results.is_empty() && ui.button("导出结果").clicked() {
                let text: String = self.results.iter()
                    .take(50)
                    .map(|e| e.full_path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                ctx.copy_text(text);
                self.status_message = format!("✓ 已复制 {} 个结果到剪贴板", self.results.len().min(50));
            }
        });

        ui.separator();

        // ==================== 中央内容 (原 CentralPanel) ====================
        if self.show_options {
            options_panel::render_options(ui, self);
        } else {
            ui.horizontal(|ui| {
                let fs_btn = egui::Button::new(if self.active_tab == Tab::FileSearch { "文件搜索" } else { "文件搜索" });
                if ui.add(fs_btn).clicked() { self.active_tab = Tab::FileSearch; }
            });
            ui.separator();

            match self.active_tab {
                Tab::FileSearch => {
                    if let Some(action) = results_panel::render_results(ui, self) {
                        match action {
                            ResultAction::Open(idx) => {
                                self.selected_index = Some(idx);
                                self.open_result(idx);
                            }
                            ResultAction::OpenPath(idx) => {
                                self.open_path(idx);
                            }
                            ResultAction::Properties(idx) => {
                                self.open_properties(idx);
                            }
                            ResultAction::Sort(field) => {
                                if self.sort_field == field {
                                    self.sort_order = match self.sort_order {
                                        SortOrder::Ascending => SortOrder::Descending,
                                        SortOrder::Descending => SortOrder::Ascending,
                                    };
                                } else {
                                    self.sort_field = field;
                                    self.sort_order = SortOrder::Ascending;
                                }
                                self.last_search.clear();
                                self.execute_search();
                            }
                            ResultAction::CopyPath(idx) => {
                                self.copy_path(idx, &ctx);
                                self.status_message = "✓ 路径已复制到剪贴板".to_string();
                            }
                            ResultAction::CopyName(idx) => {
                                self.copy_name(idx, &ctx);
                                self.status_message = "✓ 文件名已复制到剪贴板".to_string();
                            }
                        }
                    }
                }
            }
        }

        ui.separator();

        // ==================== 底部状态栏 (原 BottomPanel) ====================
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            ui.label(RichText::new(&self.status_message).size(11.0));

            let total = self.engine.total_entries();
            if total > 0 {
                ui.separator();
                ui.label(RichText::new(format!("索引 {} 条", total)).size(11.0).color(Color32::DARK_GREEN));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.results.is_empty() {
                    let sel = self.selected_index.map(|i| i + 1).unwrap_or(0);
                    ui.label(RichText::new(format!("选中: {}/{}", sel, self.results.len())).size(11.0));
                    ui.separator();
                }

                let sort_name = match self.sort_field {
                    SortField::Name => "名称",
                    SortField::Path => "路径",
                    SortField::Size => "大小",
                    SortField::Extension => "扩展名",
                    SortField::DateModified => "修改日期",
                    SortField::DateCreated => "创建日期",
                    SortField::DateAccessed => "访问日期",
                    SortField::RunCount => "运行次数",
                    _ => "名称",
                };
                let arrow = match self.sort_order {
                    SortOrder::Ascending => "▲",
                    SortOrder::Descending => "▼",
                };
                ui.label(RichText::new(format!("{} {}", sort_name, arrow)).size(11.0));
                ui.separator();

                if self.run_in_background {
                    ui.label(RichText::new("后台运行").size(11.0).color(Color32::DARK_BLUE));
                }

                if self.indexing_in_progress {
                    ui.label(RichText::new("⏳ 索引中...").size(11.0).color(Color32::GOLD));
                }
            });
        });
    }
}

// 公共辅助函数
#[cfg(windows)]
pub fn get_windows_drives() -> Vec<String> {
    let mut drives = Vec::new();
    for letter in 'A'..='Z' {
        let root = format!("{}:\\", letter);
        if std::path::Path::new(&root).exists() {
            drives.push(format!("{}:\\", letter));
        }
    }
    drives
}

fn get_state_file_path() -> PathBuf {
    let mut path = std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    path.push("文件检索助手_state.json");
    path
}

// Drop 实现
impl Drop for EverythingApp {
    fn drop(&mut self) {
        self.file_watcher.stop_watching();
        self.save_state();
    }
}