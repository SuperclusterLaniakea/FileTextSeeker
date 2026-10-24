pub mod cleaner;
pub mod docs;
pub mod duplicates;
pub mod lister;
pub mod merge;
pub mod rename;
pub mod search;
pub mod sync;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};

use crate::app::{Tab, ToastKind};
use crate::config::AppConfig;
use crate::theme;
use crate::util;
use crate::widgets;

pub struct TabEnv<'a> {
    pub config: &'a mut AppConfig,
    pub data_dir: &'a Path,
    pub status: &'a mut String,
    pub toasts: Vec<(String, ToastKind)>,
    pub dirty: bool,
}

impl TabEnv<'_> {
    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push((text.into(), kind));
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        *self.status = text.into();
    }
}

pub struct Tabs {
    pub search: search::SearchTab,
    pub docs: docs::DocsTab,
    pub duplicates: duplicates::DuplicatesTab,
    pub rename: rename::RenameTab,
    pub sync: sync::SyncTab,
    pub merge: merge::MergeTab,
    pub lister: lister::ListerTab,
    pub cleaner: cleaner::CleanerTab,
}

impl Tabs {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            search: search::SearchTab::new(config),
            docs: docs::DocsTab::new(config),
            duplicates: duplicates::DuplicatesTab::new(config),
            rename: rename::RenameTab::new(config),
            sync: sync::SyncTab::new(config),
            merge: merge::MergeTab::new(config),
            lister: lister::ListerTab::new(config),
            cleaner: cleaner::CleanerTab::new(config),
        }
    }

    pub fn ui(&mut self, tab: Tab, ui: &mut egui::Ui, env: &mut TabEnv) {
        match tab {
            Tab::Search => self.search.ui(ui, env),
            Tab::Docs => self.docs.ui(ui, env),
            Tab::Duplicates => self.duplicates.ui(ui, env),
            Tab::Rename => self.rename.ui(ui, env),
            Tab::Sync => self.sync.ui(ui, env),
            Tab::Merge => self.merge.ui(ui, env),
            Tab::Lister => self.lister.ui(ui, env),
            Tab::Cleaner => self.cleaner.ui(ui, env),
        }
    }

    pub fn monitoring_paths(&self, config: &AppConfig, data_dir: &Path) -> Vec<PathBuf> {
        if !config.monitoring {
            return Vec::new();
        }
        let mut paths = self.search.watch_roots();
        paths.extend(docs::DocsTab::watch_roots(config, data_dir));
        paths
    }

    pub fn apply_fs_events(
        &mut self,
        paths: &[PathBuf],
        ctx: &egui::Context,
        env: &mut TabEnv,
    ) -> usize {
        if paths.is_empty() {
            return 0;
        }
        let mut affected = false;
        affected |= self.search.apply_fs_events(paths, ctx, env);
        affected |= self.docs.apply_fs_events(paths, env);
        if affected {
            paths.len()
        } else {
            0
        }
    }

    pub fn tick(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        self.docs.tick(ctx, env);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub stage: String,
    pub done: u64,
    pub total: u64,
    pub message: String,
}

pub struct JobState<T> {
    pub running: bool,
    pub cancel_requested: bool,
    cancel: Arc<AtomicBool>,
    pub progress: Progress,
    pub result: Option<T>,
    pub error: Option<String>,
}

pub type Job<T> = Arc<Mutex<JobState<T>>>;

pub fn new_job<T>() -> Job<T> {
    Arc::new(Mutex::new(JobState {
        running: false,
        cancel_requested: false,
        cancel: Arc::new(AtomicBool::new(false)),
        progress: Progress::default(),
        result: None,
        error: None,
    }))
}

#[derive(Clone)]
pub struct ProgressSink {
    update: Arc<dyn Fn(Progress) + Send + Sync>,
}

impl ProgressSink {
    pub fn set(&self, stage: impl Into<String>, done: u64, total: u64, message: impl Into<String>) {
        (self.update)(Progress {
            stage: stage.into(),
            done,
            total,
            message: message.into(),
        });
    }
}

fn lock<T>(job: &Job<T>) -> std::sync::MutexGuard<'_, JobState<T>> {
    job.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn job_start<T, F>(job: &Job<T>, ctx: &egui::Context, work: F)
where
    T: Send + 'static,
    F: FnOnce(&AtomicBool, ProgressSink) -> Result<T, String> + Send + 'static,
{
    let cancel = {
        let mut state = lock(job);
        if state.running {
            return;
        }
        state.running = true;
        state.cancel_requested = false;
        state.error = None;
        state.result = None;
        state.progress = Progress { stage: "准备中".into(), ..Default::default() };
        state.cancel.store(false, Ordering::Relaxed);
        Arc::clone(&state.cancel)
    };
    let job_for_sink = Arc::clone(job);
    let ctx_for_repaint = ctx.clone();
    let sink = ProgressSink {
        update: Arc::new(move |progress| {
            if let Ok(mut state) = job_for_sink.lock() {
                if !state.running {
                    return;
                }
                state.progress = progress;
            }
            ctx_for_repaint.request_repaint();
        }),
    };
    let job_for_worker = Arc::clone(job);
    let ctx_for_worker = ctx.clone();
    std::thread::spawn(move || {
        let result = work(&cancel, sink);
        let mut state = lock(&job_for_worker);
        state.running = false;
        match result {
            Ok(value) => state.result = Some(value),
            Err(error) => {
                if !state.cancel_requested {
                    state.error = Some(error);
                }
            }
        }
        drop(state);
        ctx_for_worker.request_repaint();
    });
}

pub fn job_cancel<T>(job: &Job<T>) {
    let mut state = lock(job);
    state.cancel_requested = true;
    state.cancel.store(true, Ordering::Relaxed);
    state.progress.stage = "正在取消…".into();
}

pub fn job_take<T>(job: &Job<T>) -> Option<T> {
    lock(job).result.take()
}

pub fn job_running<T>(job: &Job<T>) -> bool {
    lock(job).running
}

pub fn job_view<T>(job: &Job<T>) -> (bool, bool, Progress, Option<String>) {
    let state = lock(job);
    (
        state.running,
        state.cancel_requested,
        state.progress.clone(),
        state.error.clone(),
    )
}

pub fn progress_row<T>(ui: &mut egui::Ui, job: &Job<T>) -> bool {
    let (running, cancelling, progress, error) = job_view(job);
    let mut cancelled = false;
    if running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(&progress.stage).size(12.0).strong());
            if progress.total > 0 {
                ui.label(
                    RichText::new(format!("{}/{}", progress.done, progress.total))
                        .size(11.5)
                        .color(theme::dim(ui)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if cancelling {
                    ui.label(RichText::new("正在取消…").size(11.5).color(theme::WARN));
                } else if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "取消").clicked() {
                    cancelled = true;
                }
            });
        });
        if !progress.message.is_empty() {
            ui.label(
                RichText::new(truncate_path(&progress.message, 120))
                    .size(10.5)
                    .color(theme::dim(ui)),
            );
        }
        if progress.total > 0 {
            let ratio = (progress.done as f32 / progress.total as f32).clamp(0.0, 1.0);
            ui.add(egui::ProgressBar::new(ratio).desired_height(6.0).show_percentage());
        } else {
            ui.add(egui::ProgressBar::new(0.0).desired_height(6.0).animate(true));
        }
    }
    if let Some(error) = error {
        ui.label(RichText::new(error).size(11.5).color(theme::DANGER));
    }
    if cancelled {
        job_cancel(job);
    }
    cancelled
}

pub fn section_card<R>(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: Option<&str>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    widgets::card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            widgets::section_label(ui, title);
            if let Some(subtitle) = subtitle {
                ui.label(RichText::new(subtitle).size(10.5).color(theme::dim(ui).gamma_multiply(0.85)));
            }
        });
        ui.add_space(6.0);
        add(ui)
    })
}

pub fn stat_row(ui: &mut egui::Ui, entries: &[(&str, String, Color32)]) {
    ui.horizontal_wrapped(|ui| {
        for (label, value, color) in entries {
            widgets::stat_card(ui, label, value, *color);
        }
    });
}

pub fn truncate_path(path: &str, max: usize) -> String {
    if path.chars().count() <= max {
        return path.to_string();
    }
    let tail: String = path.chars().rev().take(max.saturating_sub(3)).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

/// Attach double-click-to-open and a path context menu to an existing response.
pub fn path_actions(response: egui::Response, path: &Path) -> egui::Response {
    let response = if response.sense.senses_click() {
        response
    } else {
        response.interact(egui::Sense::click())
    };
    if response.double_clicked() {
        if path.exists() {
            util::open_path(path);
        } else if let Some(parent) = path.parent() {
            util::open_path(parent);
        }
    }
    response.context_menu(|ui| {
        let exists = path.exists();
        if exists {
            if ui.button("打开").clicked() {
                util::open_path(path);
                ui.close();
            }
            if ui.button("在资源管理器中显示").clicked() {
                util::reveal_path(path);
                ui.close();
            }
        }
        if ui.button("复制完整路径").clicked() {
            ui.ctx().copy_text(path.display().to_string());
            ui.close();
        }
        if !exists {
            ui.label(RichText::new("路径不存在").size(11.0).color(theme::WARN));
        }
    });
    response
}

pub fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().pick_folder()
}

pub fn pick_folders() -> Vec<PathBuf> {
    rfd::FileDialog::new().pick_folders().unwrap_or_default()
}

pub fn pick_file(filter_name: &str, extensions: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter(filter_name, extensions).pick_file()
}

pub fn save_file(default_name: &str, filter_name: &str, extensions: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_file_name(default_name)
        .add_filter(filter_name, extensions)
        .save_file()
}

pub fn path_chips(
    ui: &mut egui::Ui,
    paths: &mut Vec<String>,
    empty_hint: &str,
) -> bool {
    let mut changed = false;
    if paths.is_empty() {
        ui.label(RichText::new(empty_hint).size(11.5).color(theme::dim(ui)));
    }
    let mut remove: Option<usize> = None;
    ui.horizontal_wrapped(|ui| {
        for (index, path) in paths.iter().enumerate() {
            egui::Frame::new()
                .fill(theme::ACCENT.gamma_multiply(0.08))
                .stroke(Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.2)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin { left: 8, right: 4, top: 3, bottom: 3 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(truncate_path(path, 52)).size(11.0).color(theme::text(ui)));
                        if ui
                            .add(egui::Button::new(RichText::new("×").size(12.0)).frame(false))
                            .on_hover_text("移除")
                            .clicked()
                        {
                            remove = Some(index);
                        }
                    });
                });
        }
    });
    if let Some(index) = remove {
        paths.remove(index);
        changed = true;
    }
    changed
}

pub fn add_folders_button(ui: &mut egui::Ui, paths: &mut Vec<String>, label: &str) -> bool {
    let mut changed = false;
    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), label).clicked() {
        for path in pick_folders() {
            let text = path.display().to_string();
            if !paths.iter().any(|p| p.eq_ignore_ascii_case(&text)) {
                paths.push(text);
                changed = true;
            }
        }
    }
    changed
}

pub fn add_folder_button(ui: &mut egui::Ui, paths: &mut Vec<String>, label: &str) -> bool {
    let mut changed = false;
    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), label).clicked() {
        if let Some(path) = pick_folder() {
            let text = path.display().to_string();
            if !paths.iter().any(|p| p.eq_ignore_ascii_case(&text)) {
                paths.push(text);
                changed = true;
            }
        }
    }
    changed
}

pub fn toggle_chip(ui: &mut egui::Ui, active: bool, text: &str) -> bool {
    let fill = if active {
        theme::ACCENT.gamma_multiply(0.18)
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };
    let stroke = if active {
        Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.5))
    } else {
        Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color)
    };
    let text_color = if active { theme::ACCENT } else { theme::dim(ui) };
    ui.add(
        egui::Button::new(RichText::new(text).size(11.5).color(text_color))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(0.0, 24.0)),
    )
    .clicked()
}

pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    widgets::pill(ui, text, color);
}

pub fn small_muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).size(10.5).color(theme::dim(ui)));
}

pub fn confirm_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    title: &str,
    body: &str,
    confirm_label: &str,
    danger: bool,
) -> bool {
    let mut confirmed = false;
    if !*open {
        return false;
    }
    let visuals = ctx.style_of(ctx.theme()).visuals.clone();
    let mut window_open = *open;
    egui::Window::new(title)
        .id(egui::Id::new(("confirm", title)))
        .collapsible(false)
        .resizable(false)
        .open(&mut window_open)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            egui::Frame::new()
                .fill(visuals.window_fill)
                .stroke(Stroke::new(1.0, if danger { theme::DANGER.gamma_multiply(0.4) } else { theme::BORDER }))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(18.0)
                .shadow(visuals.window_shadow),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(body).size(12.5));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if danger {
                    if widgets::danger_button(ui, Some(widgets::ToolIcon::Check), confirm_label).clicked() {
                        confirmed = true;
                    }
                } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Check), confirm_label).clicked() {
                    confirmed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::ghost_button(ui, None, "取消").clicked() {
                        *open = false;
                    }
                });
            });
        });
    if confirmed || !window_open {
        *open = false;
    }
    confirmed
}

pub fn fixed_pos_grid(ui: &mut egui::Ui, id: &str, columns: usize, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(columns)
        .spacing([12.0, 6.0])
        .show(ui, add);
}
