use eframe::egui::{self, CentralPanel, TopBottomPanel};
use crate::file_seeker::gui::app::EverythingApp;
use crate::file_seeker::engine::Engine;
use crate::doc_searcher::app::DocSearcherApp;
use crate::file_lister_gui::FileListerApp;
use crate::code_merger::CodeMergerApp;
use std::sync::Arc;

#[derive(PartialEq)]
enum ActiveTab {
    FileSearch,
    DocSearch,
    FileListGenerate,
    CodeMerge,
}

pub struct MergedApp {
    file_seeker: EverythingApp,
    doc_searcher: DocSearcherApp,
    file_lister: FileListerApp,
    code_merger: CodeMergerApp,
    active_tab: ActiveTab,
}

impl MergedApp {
    pub fn new(cc: &eframe::CreationContext<'_>, start_minimized: bool) -> Self {
        // 加载中文字体
        let mut fonts = egui::FontDefinitions::default();
        for path in &["C:\\Windows\\Fonts\\msyh.ttc", "C:\\Windows\\Fonts\\simsun.ttc"] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert("chinese".to_owned(), egui::FontData::from_owned(bytes.into()));
                fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "chinese".to_owned());
                fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "chinese".to_owned());
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);

        // 文件检索助手初始化
        let engine = Arc::new(Engine::new());
        let config_path = crate::file_seeker::config::Config::get_config_path();
        if let Ok(cfg) = crate::file_seeker::config::Config::load(&config_path.to_string_lossy()) {
            *engine.config.write().unwrap() = cfg;
        }
        {
            let mut sources = engine.index_sources.write().unwrap();
            if let Ok(profile) = std::env::var("USERPROFILE") {
                sources.push(crate::file_seeker::types::IndexSource {
                    index_type: crate::file_seeker::types::IndexType::Folder,
                    path: std::path::PathBuf::from(&profile),
                    enabled: true,
                    label: Some(profile),
                });
            }
        }
        let file_seeker = EverythingApp::new_with_engine(engine, start_minimized);
        let doc_searcher = DocSearcherApp::new(cc);
        let file_lister = FileListerApp::default();
        let code_merger = CodeMergerApp::default();

        Self {
            file_seeker,
            doc_searcher,
            file_lister,
            code_merger,
            active_tab: ActiveTab::FileSearch,
        }
    }
}

impl eframe::App for MergedApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.file_seeker.save_state();
            self.file_seeker.file_watcher.stop_watching();
            #[cfg(windows)] { crate::file_seeker::tray::kill_tray_helper(); }
            std::process::exit(0);
        }

        TopBottomPanel::top("main_tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 左侧：标签页切换
                ui.selectable_value(&mut self.active_tab, ActiveTab::FileSearch, "📁 文件搜索");
                ui.selectable_value(&mut self.active_tab, ActiveTab::DocSearch, "📄 文档检索");
                ui.selectable_value(&mut self.active_tab, ActiveTab::FileListGenerate, "📋 文件清单生成器");
                ui.selectable_value(&mut self.active_tab, ActiveTab::CodeMerge, "🔧 代码合并工具");

                // 右侧：最小化到托盘按钮
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("最小化到托盘").clicked() {
                        #[cfg(windows)] {
                            crate::file_seeker::tray::hide_main_window();
                            crate::file_seeker::tray::ensure_tray_helper();
                        }
                        self.file_seeker.minimized = true;
                    }
                });
            });
        });

        CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                ActiveTab::FileSearch => self.file_seeker.render(ui),
                ActiveTab::DocSearch => self.doc_searcher.render(ui),
                ActiveTab::FileListGenerate => self.file_lister.render(ui),
                ActiveTab::CodeMerge => self.code_merger.render(ui),
            }
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}