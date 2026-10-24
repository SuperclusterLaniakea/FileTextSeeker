use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, CornerRadius, Layout, RichText, Stroke, Vec2};

use crate::config::{self, AppConfig, ConfigStore, ThemeChoice};
use crate::theme;
use crate::widgets::{self, ToolIcon};
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Search,
    Docs,
    Duplicates,
    Rename,
    Sync,
    Merge,
    Lister,
    Cleaner,
}

impl Tab {
    pub fn id(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Docs => "docs",
            Self::Duplicates => "duplicates",
            Self::Rename => "rename",
            Self::Sync => "sync",
            Self::Merge => "merge",
            Self::Lister => "lister",
            Self::Cleaner => "cleaner",
        }
    }
    pub fn from_id(id: &str) -> Self {
        match id {
            // The former "快速索引" tab was merged into 文件搜索.
            "swift" => Self::Search,
            "docs" => Self::Docs,
            "duplicates" => Self::Duplicates,
            "rename" => Self::Rename,
            "sync" => Self::Sync,
            "merge" => Self::Merge,
            "lister" => Self::Lister,
            "cleaner" => Self::Cleaner,
            _ => Self::Search,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Search => "文件搜索",
            Self::Docs => "文档搜索",
            Self::Duplicates => "重复查找",
            Self::Rename => "批量重命名",
            Self::Sync => "文件同步",
            Self::Merge => "代码合并",
            Self::Lister => "文件清单",
            Self::Cleaner => "垃圾清理",
        }
    }
    pub fn caption(self) -> &'static str {
        match self {
            Self::Search => "持久索引 + 精确匹配，按名称、路径、大小与日期瞬间定位",
            Self::Docs => "全文检索 PDF、Word、Excel、PPT 与文本",
            Self::Duplicates => "重复文件、相似图片与空间占用分析",
            Self::Rename => "规则流水线、标签模板与批量改名预览",
            Self::Sync => "镜像 / 更新 / 双向同步，高速并行复制",
            Self::Merge => "把整个代码目录汇成一份可读文档",
            Self::Lister => "目录盘点并导出 CSV / Excel 清单",
            Self::Cleaner => "安全扫描系统与应用缓存，一键释放空间",
        }
    }
    pub fn icon(self) -> ToolIcon {
        match self {
            Self::Search => ToolIcon::Search,
            Self::Docs => ToolIcon::Document,
            Self::Duplicates => ToolIcon::Copies,
            Self::Rename => ToolIcon::Pencil,
            Self::Sync => ToolIcon::Sync,
            Self::Merge => ToolIcon::Layers,
            Self::Lister => ToolIcon::Grid,
            Self::Cleaner => ToolIcon::Broom,
        }
    }
    pub fn group(self) -> &'static str {
        match self {
            Self::Search | Self::Docs => "智能检索",
            Self::Duplicates | Self::Rename | Self::Merge | Self::Lister => "文件工作台",
            Self::Sync | Self::Cleaner => "系统工具",
        }
    }
}

pub const ALL_TABS: [Tab; 8] = [
    Tab::Search,
    Tab::Docs,
    Tab::Duplicates,
    Tab::Rename,
    Tab::Merge,
    Tab::Lister,
    Tab::Sync,
    Tab::Cleaner,
];

#[derive(Clone, Copy)]
pub enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

struct Toast {
    text: String,
    kind: ToastKind,
    born: Instant,
}

pub struct App {
    pub config_store: ConfigStore,
    pub config: AppConfig,
    pub tabs: crate::tabs::Tabs,
    pub tab: Tab,
    pub status: String,
    pub dirty: bool,
    toasts: Vec<Toast>,
    settings_open: bool,
    about_open: bool,
    hidden_to_tray: bool,
    exit_requested: bool,
    last_autosave: Instant,
    start_minimized: bool,
    elevated: bool,
    pub monitoring_events: u64,
    pub quick_focus: bool,
    screenshot_path: Option<PathBuf>,
    screenshot_requested: bool,
    frames: u64,
    tray: Option<crate::tray::Tray>,
    monitor: crate::monitor::MonitorHub,
    scheduled_clean: Option<mpsc::Receiver<(String, Option<String>)>>,
    last_schedule_check: Option<Instant>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config_dir: Option<PathBuf>,
        start_minimized: bool,
        screenshot_path: Option<PathBuf>,
        initial_tab: Option<Tab>,
    ) -> Self {
        let store = ConfigStore::new(config_dir);
        let mut cfg = store.load();
        cfg.autostart = config::autostart_enabled();
        let tab = initial_tab.unwrap_or_else(|| Tab::from_id(&cfg.active_tab));
        theme::setup(&cc.egui_ctx, cfg.theme == ThemeChoice::Dark, cfg.font_scale);

        let tabs = crate::tabs::Tabs::new(&cfg);
        let tray = if screenshot_path.is_none() {
            crate::tray::Tray::spawn(cfg.monitoring)
        } else {
            None
        };
        let hidden_to_tray = start_minimized && tray.is_some();
        let monitor = crate::monitor::MonitorHub::new(cc.egui_ctx.clone());
        let mut app = Self {
            config_store: store,
            config: cfg,
            tabs,
            tab,
            status: "就绪".into(),
            dirty: false,
            toasts: Vec::new(),
            settings_open: false,
            about_open: false,
            hidden_to_tray,
            exit_requested: false,
            last_autosave: Instant::now(),
            start_minimized,
            elevated: util::is_elevated(),
            monitoring_events: 0,
            quick_focus: false,
            screenshot_path,
            screenshot_requested: false,
            frames: 0,
            tray,
            monitor,
            scheduled_clean: None,
            last_schedule_check: None,
        };
        app.status = "ToolsNest 已就绪，所有工具数据仅在本机处理".into();
        if app.hidden_to_tray && app.config.low_memory {
            util::trim_working_set();
        }
        app
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
        while self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn save(&mut self) {
        self.config.active_tab = self.tab.id().to_string();
        self.config_store.save(&self.config);
        self.dirty = false;
    }

    pub fn has_tray(&self) -> bool {
        self.tray.is_some()
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden_to_tray = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        let events = self.tray.as_ref().map(|tray| tray.drain()).unwrap_or_default();
        for event in events {
            match event {
                crate::tray::TrayEvent::Show => self.show_window(ctx),
                crate::tray::TrayEvent::ToggleMonitoring => {
                    self.config.monitoring = !self.config.monitoring;
                    self.dirty = true;
                    let message = if self.config.monitoring {
                        "实时监控已开启，索引会随文件变更自动更新"
                    } else {
                        "实时监控已关闭"
                    };
                    self.status = message.into();
                    self.toast(message, ToastKind::Info);
                }
                crate::tray::TrayEvent::Exit => {
                    self.exit_requested = true;
                    self.save();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some(tray) = &self.tray {
            tray.set_monitoring(self.config.monitoring);
        }
        if crate::tray::show_requested() {
            self.show_window(ctx);
        }
    }

    fn process_monitoring(&mut self, ctx: &egui::Context) {
        let paths = self.tabs.monitoring_paths(&self.config, &self.config_store.dir);
        self.monitor.sync(&paths, self.config.monitoring);
        let batch = self.monitor.drain();
        let mut env = crate::tabs::TabEnv {
            config: &mut self.config,
            data_dir: self.config_store.dir.as_path(),
            status: &mut self.status,
            toasts: Vec::new(),
            dirty: false,
        };
        let mut affected = 0;
        if !batch.is_empty() {
            affected = self.tabs.apply_fs_events(&batch, ctx, &mut env);
            self.monitoring_events += batch.len() as u64;
        }
        self.tabs.tick(ctx, &mut env);
        let toasts = std::mem::take(&mut env.toasts);
        let dirty = env.dirty;
        drop(env);
        for (text, kind) in toasts {
            self.toast(text, kind);
        }
        if dirty {
            self.dirty = true;
        }
        if affected > 0 {
            self.status = format!("实时监控 · 已同步 {affected} 项文件变更");
        }
    }

    fn poll_scheduled_clean(&mut self) {
        if let Some(receiver) = &self.scheduled_clean {
            if let Ok((message, history)) = receiver.try_recv() {
                self.scheduled_clean = None;
                self.config.cleaner.last_run = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                if let Some(entry) = history {
                    self.config.cleaner.history.insert(0, entry);
                    self.config.cleaner.history.truncate(20);
                }
                self.status = message.clone();
                self.dirty = true;
                self.toast(message, ToastKind::Success);
                self.save();
            }
            return;
        }
        if !self.config.cleaner.schedule_enabled {
            return;
        }
        let due_check = self
            .last_schedule_check
            .map(|last| last.elapsed() >= Duration::from_secs(30))
            .unwrap_or(true);
        if !due_check {
            return;
        }
        self.last_schedule_check = Some(Instant::now());
        let Ok(target) = chrono::NaiveTime::parse_from_str(self.config.cleaner.schedule_time.trim(), "%H:%M") else {
            return;
        };
        let now = chrono::Local::now();
        if now.time() < target {
            return;
        }
        let today = now.format("%Y-%m-%d").to_string();
        if self.config.cleaner.last_run.starts_with(&today) {
            return;
        }
        let rules: Vec<crate::cleaner::CleanRule> = crate::cleaner::active_rules(&self.config.cleaner)
            .into_iter()
            .filter(|rule| !rule.dangerous)
            .collect();
        if rules.is_empty() {
            return;
        }
        let recycle = self.config.cleaner.schedule_recycle;
        let (sender, receiver) = mpsc::channel();
        self.scheduled_clean = Some(receiver);
        self.status = "定时清理正在后台运行…".into();
        std::thread::Builder::new()
            .name("fts-scheduled-clean".into())
            .spawn(move || {
                let cancel = AtomicBool::new(false);
                let scan = crate::cleaner::scan(&rules, &cancel, &mut |_| {});
                let report = crate::cleaner::delete_items(&scan.items, recycle, &cancel, &mut |_| {});
                let message = if scan.items.is_empty() {
                    "定时清理完成 · 没有可清理的项目".to_string()
                } else if report.failed > 0 {
                    format!(
                        "定时清理完成 · 释放 {} · {} 项未能删除",
                        util::format_size(report.freed),
                        report.failed
                    )
                } else {
                    format!(
                        "定时清理完成 · 删除 {} 个文件 · 释放 {}",
                        report.deleted_files,
                        util::format_size(report.freed)
                    )
                };
                let history = if scan.items.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} · 释放 {} · {} 项",
                        chrono::Local::now().format("%Y-%m-%d %H:%M"),
                        util::format_size(report.freed),
                        report.deleted_files
                    ))
                };
                let _ = sender.send((message, history));
            })
            .ok();
    }

    pub fn apply_theme(&self, ctx: &egui::Context) {
        theme::setup(ctx, self.config.theme == ThemeChoice::Dark, self.config.font_scale);
    }

    fn nav_item(ui: &mut egui::Ui, tab: Tab, active: bool) -> bool {
        let height = 42.0;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), egui::Sense::click());
        let hovered = response.hovered();
        if active {
            ui.painter()
                .rect_filled(rect.shrink2(Vec2::new(0.0, 2.0)), CornerRadius::same(9), theme::ACCENT.gamma_multiply(0.13));
            let bar = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 2.0, rect.center().y - 10.0),
                Vec2::new(3.0, 20.0),
            );
            ui.painter().rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
        } else if hovered {
            ui.painter().rect_filled(
                rect.shrink2(Vec2::new(0.0, 2.0)),
                CornerRadius::same(9),
                ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.55),
            );
        }
        let color = if active {
            theme::ACCENT
        } else if hovered {
            ui.visuals().text_color()
        } else {
            theme::dim(ui)
        };
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            Vec2::splat(18.0),
        );
        widgets::paint_icon(ui, icon_rect, tab.icon(), color);
        ui.painter().text(
            egui::pos2(rect.left() + 44.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            tab.label(),
            egui::FontId::proportional(13.5),
            if active { ui.visuals().text_color() } else { color },
        );
        response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        let side = egui::Panel::left("sidebar")
            .exact_size(224.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(if self.config.theme == ThemeChoice::Dark { theme::SIDEBAR } else { theme::LIGHT_SIDEBAR })
                    .inner_margin(egui::Margin { left: 12, right: 12, top: 14, bottom: 12 }),
            );
        side.show(ui, |ui| {
            let footer_height = 132.0;
            let nav_height = (ui.available_height() - footer_height).max(240.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), nav_height),
                Layout::top_down(Align::LEFT),
                |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(11), theme::ACCENT.gamma_multiply(0.16));
                widgets::paint_icon(ui, rect.shrink(9.0), ToolIcon::Bolt, theme::ACCENT);
                ui.add_space(3.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("ToolsNest").size(15.0).strong());
                    ui.label(RichText::new("全能文件工具箱").size(10.5).color(theme::dim(ui)));
                });
            });
            ui.add_space(18.0);

            let mut last_group = "";
            for tab in ALL_TABS {
                if tab.group() != last_group {
                    if !last_group.is_empty() {
                        ui.add_space(10.0);
                    }
                    last_group = tab.group();
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(tab.group())
                                .size(10.5)
                                .color(theme::dim(ui).gamma_multiply(0.8)),
                        );
                    });
                    ui.add_space(1.0);
                }
                let active = self.tab == tab;
                if Self::nav_item(ui, tab, active) {
                    self.tab = tab;
                    self.dirty = true;
                    self.status = format!("{} · {}", tab.label(), tab.caption());
                }
            }
                        });
                },
            );
            ui.add_space(6.0);

            ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    widgets::draw_dot(ui, self.config.monitoring, theme::SUCCESS);
                    ui.label(
                        RichText::new(if self.config.monitoring { "实时监控运行中" } else { "实时监控已关闭" })
                            .size(11.5)
                            .color(theme::dim(ui)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{:.0} MB", util::process_memory_mb())).size(10.5).color(theme::dim(ui)));
                    });
                });
                ui.add_space(4.0);
                let frame = egui::Frame::new()
                    .fill(theme::ACCENT.gamma_multiply(0.06))
                    .stroke(Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.16)))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(11));
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        widgets::paint_icon(
                            ui,
                            egui::Rect::from_min_size(ui.cursor().min, Vec2::splat(15.0)),
                            ToolIcon::Monitor,
                            theme::ACCENT,
                        );
                        ui.add_space(19.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("后台常驻 + 实时监控").size(11.5).strong());
                            ui.label(
                                RichText::new("关闭窗口后继续在托盘运行")
                                    .size(10.0)
                                    .color(theme::dim(ui)),
                            );
                        });
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::IconButton::new(ToolIcon::Settings, "设置").show(ui).clicked() {
                            self.settings_open = true;
                        }
                    });
                });
            });
        });
    }

    fn draw_header(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(if self.config.theme == ThemeChoice::Dark {
                        theme::BG
                    } else {
                        theme::LIGHT_BG
                    })
                    .inner_margin(egui::Margin { left: 22, right: 22, top: 14, bottom: 10 }),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.tab.group())
                                    .size(11.0)
                                    .color(theme::ACCENT.gamma_multiply(0.9)),
                            );
                            ui.label(RichText::new("·").size(11.0).color(theme::dim(ui)));
                            ui.label(
                                RichText::new(self.tab.label())
                                    .size(11.0)
                                    .color(theme::dim(ui)),
                            );
                        });
                        ui.add_space(3.0);
                        ui.label(RichText::new(self.tab.label()).size(24.0).strong());
                        ui.label(
                            RichText::new(self.tab.caption())
                                .size(12.0)
                                .color(theme::dim(ui)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::IconButton::new(ToolIcon::Gear, "设置").show(ui).clicked() {
                            self.settings_open = true;
                        }
                        if widgets::IconButton::new(ToolIcon::Minus, "最小化到系统托盘").show(ui).clicked() {
                            self.hide_to_tray(&ctx);
                        }
                        let theme_icon = if self.config.theme == ThemeChoice::Dark { ToolIcon::Bolt } else { ToolIcon::Monitor };
                        if widgets::IconButton::new(theme_icon, "切换主题").show(ui).clicked() {
                            self.config.theme = if self.config.theme == ThemeChoice::Dark {
                                ThemeChoice::Light
                            } else {
                                ThemeChoice::Dark
                            };
                            self.apply_theme(&ctx);
                            self.dirty = true;
                        }
                        ui.add_space(4.0);
                        let mut monitoring = self.config.monitoring;
                        let response = ui
                            .horizontal(|ui| {
                                widgets::draw_dot(ui, monitoring, theme::SUCCESS);
                                ui.label(
                                    RichText::new("实时监控")
                                        .size(12.0)
                                        .color(if monitoring { theme::text(ui) } else { theme::dim(ui) }),
                                );
                                let toggle = widgets::toggle_switch(ui, &mut monitoring);
                                toggle
                            })
                            .inner;
                        if response.changed() {
                            self.config.monitoring = monitoring;
                            self.dirty = true;
                            self.toast(
                                if monitoring {
                                    "实时监控已开启，变更将自动同步到索引"
                                } else {
                                    "实时监控已关闭"
                                },
                                ToastKind::Info,
                            );
                        }
                        if self.elevated {
                            ui.add_space(2.0);
                            widgets::pill(ui, "管理员", theme::SUCCESS);
                        }
                    });
                });
                ui.add_space(6.0);
                // Hairline divider with subtle accent gradient.
                let rect = ui.available_rect_before_wrap();
                let y = rect.top();
                let p = ui.painter();
                p.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                );
                p.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.left() + 140.0, y)],
                    Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.55)),
                );
            });
    }

    fn draw_footer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("footer")
            .frame(
                egui::Frame::new()
                    .fill(if self.config.theme == ThemeChoice::Dark {
                        theme::SIDEBAR
                    } else {
                        theme::LIGHT_SIDEBAR
                    })
                    .inner_margin(egui::Margin { left: 20, right: 20, top: 6, bottom: 6 }),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::draw_dot(ui, self.config.monitoring, theme::SUCCESS);
                    ui.label(RichText::new(&self.status).size(11.0).color(theme::dim(ui)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(chrono::Local::now().format("%Y-%m-%d %H:%M").to_string())
                                .size(10.5)
                                .color(theme::dim(ui).gamma_multiply(0.8)),
                        );
                        if self.config.show_memory {
                            ui.label(
                                RichText::new(format!("内存 {:.1} MB", util::process_memory_mb()))
                                    .size(10.5)
                                    .color(theme::dim(ui).gamma_multiply(0.9)),
                            );
                            ui.label(RichText::new("·").size(10.5).color(theme::dim(ui)));
                        }
                        ui.label(
                            RichText::new(format!("监控事件 {}", self.monitoring_events))
                                .size(10.5)
                                .color(theme::dim(ui).gamma_multiply(0.9)),
                        );
                    });
                });
            });
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        self.save();
        if self.tray.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            return;
        }
        self.hidden_to_tray = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        if self.config.low_memory {
            util::trim_working_set();
        }
        self.toast("已隐藏到系统托盘，搜索与监控继续在后台运行", ToastKind::Info);
    }

    fn draw_placeholder(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::empty_state(
                ui,
                self.tab.icon(),
                self.tab.label(),
                "该工作台正在装配中。",
            );
        });
    }

    fn draw_content(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 22, right: 22, top: 0, bottom: 8 }))
            .show(ui, |ui| {
                let mut env = crate::tabs::TabEnv {
                    config: &mut self.config,
                    data_dir: self.config_store.dir.as_path(),
                    status: &mut self.status,
                    toasts: Vec::new(),
                    dirty: false,
                };
                self.tabs.ui(self.tab, ui, &mut env);
                let toasts = std::mem::take(&mut env.toasts);
                let dirty = env.dirty;
                drop(env);
                for (text, kind) in toasts {
                    self.toast(text, kind);
                }
                if dirty {
                    self.dirty = true;
                }
            });
    }

    fn draw_settings(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let dark = self.config.theme == ThemeChoice::Dark;
        let visuals = ctx
            .style_of(if dark { egui::Theme::Dark } else { egui::Theme::Light })
            .visuals
            .clone();
        egui::Window::new("设置")
            .id(egui::Id::new("settings_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                egui::Frame::new()
                    .fill(visuals.window_fill)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(18.0)
                    .shadow(visuals.window_shadow),
            )
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(RichText::new("外观与体验").size(12.0).strong().color(theme::dim(ui)));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("主题");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut dark_mode = self.config.theme == ThemeChoice::Dark;
                        if ui.selectable_label(!dark_mode, "浅色").clicked() {
                            dark_mode = false;
                        }
                        if ui.selectable_label(dark_mode, "深色").clicked() {
                            dark_mode = true;
                        }
                        if (dark_mode) != (self.config.theme == ThemeChoice::Dark) {
                            self.config.theme = if dark_mode { ThemeChoice::Dark } else { ThemeChoice::Light };
                            self.apply_theme(ctx);
                            self.dirty = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("字体缩放");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("{:.0}%", self.config.font_scale * 100.0));
                        let mut scale = self.config.font_scale;
                        if ui.add(egui::Slider::new(&mut scale, 0.85..=1.35).show_value(false)).changed() {
                            self.config.font_scale = scale;
                            self.apply_theme(ctx);
                            self.dirty = true;
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new("后台与监控").size(12.0).strong().color(theme::dim(ui)));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("关闭窗口时隐藏到托盘");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::toggle_switch(ui, &mut self.config.close_to_tray).changed() {
                            self.dirty = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("开机自启动（最小化到托盘）");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::toggle_switch(ui, &mut self.config.autostart).changed() {
                            if let Err(e) = config::set_autostart(self.config.autostart) {
                                self.toast(format!("设置开机启动失败: {e}"), ToastKind::Error);
                            } else {
                                self.toast("开机启动设置已更新", ToastKind::Success);
                            }
                            self.dirty = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("启用全局实时监控");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut mon = self.config.monitoring;
                        if widgets::toggle_switch(ui, &mut mon).changed() {
                            self.config.monitoring = mon;
                            self.dirty = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("低内存模式（空闲 1 FPS，隐藏时释放工作集）");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::toggle_switch(ui, &mut self.config.low_memory).changed() {
                            self.dirty = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("状态栏显示内存占用");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::toggle_switch(ui, &mut self.config.show_memory).changed() {
                            self.dirty = true;
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if widgets::ghost_button(ui, Some(ToolIcon::Folder), "打开配置目录").clicked() {
                        util::open_path(&self.config_store.dir);
                    }
                    if widgets::ghost_button(ui, Some(ToolIcon::Refresh), "立即保存").clicked() {
                        self.save();
                        self.toast("设置已保存", ToastKind::Success);
                    }
                    if !self.elevated && widgets::ghost_button(ui, Some(ToolIcon::Shield), "以管理员重启").clicked() {
                        if util::relaunch_elevated() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("完成").clicked() {
                            self.save();
                            self.settings_open = false;
                        }
                    });
                });
                let _ = dark;
            });
        if !open {
            self.settings_open = false;
        }
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        let now = Instant::now();
        self.toasts.retain(|t| now.duration_since(t.born) < Duration::from_secs(4));
        let dark = self.config.theme == ThemeChoice::Dark;
        let visuals = ctx
            .style_of(if dark { egui::Theme::Dark } else { egui::Theme::Light })
            .visuals
            .clone();
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-22.0, 108.0))
            .interactable(false)
            .show(ctx, |ui| {
                for toast in self.toasts.iter().rev() {
                    let (color, icon) = match toast.kind {
                        ToastKind::Info => (theme::BLUE, ToolIcon::Monitor),
                        ToastKind::Success => (theme::SUCCESS, ToolIcon::Check),
                        ToastKind::Warn => (theme::WARN, ToolIcon::Shield),
                        ToastKind::Error => (theme::DANGER, ToolIcon::Close),
                    };
                    egui::Frame::new()
                        .fill(visuals.window_fill)
                        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin { left: 12, right: 14, top: 9, bottom: 9 })
                        .shadow(visuals.window_shadow)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let icon_rect = egui::Rect::from_min_size(ui.cursor().min + Vec2::new(0.0, 4.0), Vec2::splat(15.0));
                                widgets::paint_icon(ui, icon_rect, icon, color);
                                ui.add_space(20.0);
                                ui.label(RichText::new(&toast.text).size(12.5));
                            });
                        });
                    ui.add_space(6.0);
                }
            });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_tray(ctx);
        self.process_monitoring(ctx);
        self.poll_scheduled_clean();

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.config.close_to_tray
                && self.tray.is_some()
                && !self.exit_requested
                && !self.hidden_to_tray
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
            } else {
                self.save();
            }
        }
        if self.dirty && self.last_autosave.elapsed() > Duration::from_secs(5) {
            self.save();
            self.last_autosave = Instant::now();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::paint_backdrop(&ctx, self.config.theme == ThemeChoice::Dark, ui.max_rect());
        self.draw_sidebar(ui);
        self.draw_header(ui);
        self.draw_footer(ui);
        self.draw_content(ui);
        self.draw_settings(&ctx);
        self.draw_toasts(&ctx);

        self.frames += 1;
        if let Some(path) = self.screenshot_path.clone() {
            if !self.screenshot_requested && self.frames >= 4 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.screenshot_requested = true;
            }
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let [width, height] = image.size;
                if let Err(err) = image::save_buffer(
                    &path,
                    image.as_raw(),
                    width as u32,
                    height as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("screenshot save failed: {err}");
                }
                self.screenshot_path = None;
                self.exit_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }

        let idle_ms = if self.config.low_memory { 1000 } else { 250 };
        ctx.request_repaint_after(Duration::from_millis(idle_ms));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save();
    }
}
