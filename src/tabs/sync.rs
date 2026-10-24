use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::app::ToastKind;
use crate::config::{AppConfig, ConflictChoice, SyncModeChoice, SyncSettings};
use crate::sync::{self, ActionKind, SyncJob, SyncOptions, SyncPlan, SyncReport};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{job_cancel, job_start, job_take, job_view, progress_row, section_card, small_muted, stat_row, toggle_chip, Job, TabEnv};

pub struct SyncTab {
    plan: Option<SyncPlan>,
    plan_job: Job<SyncPlan>,
    exec_job: Job<SyncReport>,
    report: Option<SyncReport>,
    confirm_exec: bool,
    advanced: bool,
    show_all: bool,
    last_opts: Option<SyncOptions>,
}

impl SyncTab {
    pub fn new(_config: &AppConfig) -> Self {
        Self {
            plan: None,
            plan_job: super::new_job(),
            exec_job: super::new_job(),
            report: None,
            confirm_exec: false,
            advanced: false,
            show_all: false,
            last_opts: None,
        }
    }

    fn start_plan(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let opts = options_from(&env.config.sync);
        if opts.left.as_os_str().is_empty() || opts.right.as_os_str().is_empty() {
            env.toast("请选择左侧和右侧目录", ToastKind::Warn);
            return;
        }
        if !opts.left.is_dir() || !opts.right.is_dir() {
            env.toast("同步目录必须已经存在（可先手动创建右侧目录）", ToastKind::Warn);
            return;
        }
        self.last_opts = Some(opts.clone());
        job_start(&self.plan_job, ctx, move |cancel, sink| {
            let sink_for_progress = sink.clone();
            let plan = sync::build_plan(&opts, cancel, &mut |progress| {
                sink_for_progress.set(
                    progress.stage,
                    progress.done,
                    progress.total,
                    progress.message.clone(),
                );
            });
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("已取消扫描".into());
            }
            Ok(plan)
        });
    }

    fn start_exec(&mut self, ctx: &egui::Context, _env: &mut TabEnv) {
        let (Some(plan), Some(opts)) = (self.plan.clone(), self.last_opts.clone()) else {
            return;
        };
        let total = plan.actions.len() as u64;
        job_start(&self.exec_job, ctx, move |cancel, sink| {
            sink.set("执行同步", 0, total, "");
            let sink_for_progress = sink.clone();
            let report = sync::execute(&plan, &opts, cancel, move |progress| {
                sink_for_progress.set(
                    progress.stage,
                    progress.done,
                    progress.total,
                    progress.message.clone(),
                );
            });
            Ok(report)
        });
    }

    fn save_job(&mut self, env: &mut TabEnv) {
        let Some(path) = super::save_file("sync-job.fts.toml", "TOML 任务", &["toml"]) else {
            return;
        };
        let options = options_from(&env.config.sync);
        let job = SyncJob::from_options("我的同步任务", &options);
        match sync::save_job(&path, &job) {
            Ok(()) => env.toast("同步任务已保存，下次可一键载入", ToastKind::Success),
            Err(error) => env.toast(format!("保存任务失败: {error}"), ToastKind::Error),
        }
    }

    fn load_job(&mut self, env: &mut TabEnv) {
        let Some(path) = super::pick_file("TOML 任务", &["toml"]) else {
            return;
        };
        match sync::load_job(&path) {
            Ok(job) => {
                apply_options(&mut env.config.sync, job.to_options());
                env.mark_dirty();
                self.plan = None;
                env.toast(format!("已载入同步任务「{}」", job.name), ToastKind::Success);
            }
            Err(error) => env.toast(format!("载入任务失败: {error}"), ToastKind::Error),
        }
    }

    fn draw_dirs(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        section_card(ui, "同步目录", Some("左侧为源，右侧为目标；镜像模式会让两侧完全一致"), |ui| {
            for (label, is_left) in [("左侧", true), ("右侧", false)] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(11.5).color(theme::dim(ui)));
                    let width = (ui.available_width() - 100.0).max(180.0);
                    let value = if is_left { &mut env.config.sync.left } else { &mut env.config.sync.right };
                    if ui
                        .add_sized(
                            [width, 28.0],
                            egui::TextEdit::singleline(value)
                                .hint_text(if is_left { "源目录" } else { "目标目录" })
                                .margin(egui::Vec2::new(9.0, 5.0)),
                        )
                        .changed()
                    {
                        env.mark_dirty();
                    }
                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "浏览").clicked() {
                        if let Some(path) = super::pick_folder() {
                            if is_left {
                                env.config.sync.left = path.display().to_string();
                            } else {
                                env.config.sync.right = path.display().to_string();
                            }
                            env.mark_dirty();
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Sync), "交换左右").clicked() {
                    std::mem::swap(&mut env.config.sync.left, &mut env.config.sync.right);
                    env.mark_dirty();
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Export), "保存任务").clicked() {
                    self.save_job(env);
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "载入任务").clicked() {
                    self.load_job(env);
                }
            });
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                for mode in [SyncModeChoice::Mirror, SyncModeChoice::Update, SyncModeChoice::Twoway, SyncModeChoice::Custom] {
                    if toggle_chip(ui, env.config.sync.mode == mode, mode.label()) {
                        env.config.sync.mode = mode;
                        env.mark_dirty();
                        self.plan = None;
                    }
                }
            });
            let hint = match env.config.sync.mode {
                SyncModeChoice::Mirror => "镜像：复制新增/变更文件，并删除右侧多余文件。",
                SyncModeChoice::Update => "更新：只复制新增或较新的文件，不删除任何内容。",
                SyncModeChoice::Twoway => "双向：两侧缺失的文件互相补齐，冲突按下方规则处理。",
                SyncModeChoice::Custom => "自定义：双向同步并自动采用较新版本，跳过无法判断的冲突。",
            };
            small_muted(ui, hint);
            if matches!(env.config.sync.mode, SyncModeChoice::Twoway | SyncModeChoice::Custom) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("冲突处理").size(11.5).color(theme::dim(ui)));
                    egui::ComboBox::from_id_salt("sync_conflict")
                        .selected_text(env.config.sync.conflict.label())
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for conflict in [ConflictChoice::Newer, ConflictChoice::Left, ConflictChoice::Right, ConflictChoice::Skip] {
                                if ui
                                    .selectable_value(&mut env.config.sync.conflict, conflict, conflict.label())
                                    .changed()
                                {
                                    env.mark_dirty();
                                }
                            }
                        });
                });
            }
        });
        ui.add_space(8.0);
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        section_card(ui, "过滤与传输", Some("默认配置适合大多数场景；高级选项可精确控制"), |ui| {
            ui.horizontal_wrapped(|ui| {
                if toggle_chip(ui, env.config.sync.dry_run, "预演（不写入）") {
                    env.config.sync.dry_run = !env.config.sync.dry_run;
                    env.mark_dirty();
                }
                if toggle_chip(ui, env.config.sync.verify, "BLAKE3 校验") {
                    env.config.sync.verify = !env.config.sync.verify;
                    env.mark_dirty();
                }
                if toggle_chip(ui, env.config.sync.preserve_timestamps, "保留时间戳") {
                    env.config.sync.preserve_timestamps = !env.config.sync.preserve_timestamps;
                    env.mark_dirty();
                }
                if toggle_chip(ui, env.config.sync.exclude_hidden, "排除隐藏文件") {
                    env.config.sync.exclude_hidden = !env.config.sync.exclude_hidden;
                    env.mark_dirty();
                }
                if toggle_chip(ui, env.config.sync.follow_symlinks, "跟随符号链接") {
                    env.config.sync.follow_symlinks = !env.config.sync.follow_symlinks;
                    env.mark_dirty();
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::ChevronDown), if self.advanced { "收起高级" } else { "高级选项" }).clicked() {
                    self.advanced = !self.advanced;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("包含规则").size(11.0).color(theme::dim(ui)));
                let width = (ui.available_width() - 90.0).max(160.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut env.config.sync.include_patterns)
                            .hint_text("留空表示全部，例如 *.jpg;**/*.png")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("排除规则").size(11.0).color(theme::dim(ui)));
                let width = (ui.available_width() - 90.0).max(160.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut env.config.sync.exclude_patterns)
                            .hint_text("target/**, node_modules/**, *.tmp")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
            if self.advanced {
                ui.add_space(2.0);
                egui::Grid::new("sync_advanced")
                    .num_columns(4)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("排除正则").size(11.0).color(theme::dim(ui)));
                        ui.add_sized(
                            [200.0, 24.0],
                            egui::TextEdit::singleline(&mut env.config.sync.regex_exclude)
                                .hint_text("\\.tmp$")
                                .margin(egui::Vec2::new(7.0, 3.0)),
                        );
                        ui.label(RichText::new("最小体积 KB").size(11.0).color(theme::dim(ui)));
                        ui.add(egui::DragValue::new(&mut env.config.sync.min_size_kb).range(0..=1_000_000));
                        ui.end_row();
                        ui.label(RichText::new("最大体积 MB").size(11.0).color(theme::dim(ui)));
                        ui.add(egui::DragValue::new(&mut env.config.sync.max_size_mb).range(0..=100_000));
                        ui.label(RichText::new("并发线程").size(11.0).color(theme::dim(ui)));
                        ui.add(egui::DragValue::new(&mut env.config.sync.concurrent).range(1..=16));
                        ui.end_row();
                        ui.label(RichText::new("缓冲大小 MB").size(11.0).color(theme::dim(ui)));
                        ui.add(egui::DragValue::new(&mut env.config.sync.buffer_mb).range(0..=64));
                        ui.label(RichText::new("自动重试").size(11.0).color(theme::dim(ui)));
                        ui.add(egui::DragValue::new(&mut env.config.sync.auto_retry).range(0..=10));
                        ui.end_row();
                        ui.label(RichText::new("无缓冲复制").size(11.0).color(theme::dim(ui)));
                        ui.checkbox(&mut env.config.sync.unbuffered, "");
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    });
                small_muted(ui, "范围值为 0 表示不限制；校验会读取两侧内容计算 BLAKE3，速度略慢但更可靠。");
            }
        });
        ui.add_space(8.0);
    }

    fn draw_plan(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        let copies = plan.copy_actions().count();
        let deletes = plan.delete_actions().count();
        let conflicts = plan.conflicts;
        let dry_run = env.config.sync.dry_run;
        stat_row(
            ui,
            &[
                ("左侧文件", plan.left_files.to_string(), theme::ACCENT),
                ("右侧文件", plan.right_files.to_string(), theme::BLUE),
                ("复制", format!("{} · {}", copies, util::format_size(plan.copy_bytes())), theme::SUCCESS),
                ("删除", format!("{} · {}", deletes, util::format_size(plan.delete_bytes())), if deletes > 0 { theme::DANGER } else { theme::TEXT_DIM }),
            ],
        );
        ui.add_space(8.0);
        section_card(ui, "同步计划", Some("执行前请核对复制与删除动作"), |ui| {
            ui.horizontal(|ui| {
                let label = if dry_run { "预演计划" } else { "执行同步" };
                if !plan.actions.is_empty() || plan.left_files + plan.right_files > 0 {
                    if widgets::primary_button(ui, Some(widgets::ToolIcon::Sync), label).clicked() {
                        self.confirm_exec = true;
                    }
                } else {
                    widgets::pill(ui, "两侧已经一致", theme::SUCCESS);
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Refresh), "重新扫描").clicked() {
                    self.start_plan(ui.ctx(), env);
                }
                if dry_run {
                    widgets::pill(ui, "预演模式不会写入文件", theme::WARN);
                }
                if conflicts > 0 {
                    widgets::pill(ui, format!("冲突 {conflicts}"), theme::WARN);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if plan.actions.len() > 600 && !self.show_all {
                        if ui.small_button("显示全部").clicked() {
                            self.show_all = true;
                        }
                    }
                });
            });
            if !plan.errors.is_empty() {
                for error in plan.errors.iter().take(4) {
                    ui.label(RichText::new(error).size(11.0).color(theme::DANGER));
                }
            }
            if !plan.warnings.is_empty() {
                for warning in plan.warnings.iter().take(3) {
                    ui.label(RichText::new(warning).size(10.5).color(theme::WARN));
                }
            }
            ui.add_space(4.0);
            let limit = if self.show_all { plan.actions.len() } else { plan.actions.len().min(600) };
            TableBuilder::new(ui)
                .striped(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(132.0).at_least(96.0).clip(true))
                .column(Column::remainder().at_least(180.0).clip(true))
                .column(Column::initial(80.0).at_least(60.0).clip(true))
                .column(Column::initial(170.0).at_least(90.0).clip(true))
                .max_scroll_height(360.0)
                .header(24.0, |mut header| {
                    for title in ["操作", "相对路径", "大小", "说明"] {
                        header.col(|ui| {
                            ui.label(RichText::new(title).size(11.0).strong().color(theme::dim(ui)));
                        });
                    }
                })
                .body(|body| {
                    body.rows(24.0, limit, |mut row| {
                        let action = &plan.actions[row.index()];
                        row.col(|ui| {
                            let color = match action.kind {
                                ActionKind::CopyLeftToRight | ActionKind::CopyRightToLeft => theme::SUCCESS,
                                ActionKind::DeleteLeft | ActionKind::DeleteRight => theme::DANGER,
                                ActionKind::Conflict => theme::WARN,
                            };
                            widgets::pill(ui, action.kind.label(), color);
                        });
                        let mut path_hit = None;
                        row.col(|ui| {
                            path_hit = Some(
                                ui.add(
                                    egui::Label::new(RichText::new(&action.rel).size(11.0))
                                        .sense(egui::Sense::click()),
                                )
                                .on_hover_text(action.target.display().to_string()),
                            );
                        });
                        row.col(|ui| {
                            let text = if action.bytes > 0 { util::format_size(action.bytes) } else { "—".into() };
                            ui.label(RichText::new(text).size(10.5).color(theme::dim(ui)));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(super::truncate_path(&action.reason, 42)).size(10.5).color(theme::dim(ui)));
                        });
                        let path = action
                            .source
                            .as_deref()
                            .filter(|path| path.exists())
                            .unwrap_or(&action.target);
                        if let Some(hit) = path_hit {
                            super::path_actions(hit, path);
                        }
                        super::path_actions(row.response(), path);
                    });
                });
            if limit < plan.actions.len() {
                small_muted(ui, format!("仅显示前 {limit} 项，共 {} 项。", plan.actions.len()));
            }
        });
        ui.add_space(6.0);
        if self.confirm_exec {
            let body = format!(
                "即将处理 {} 个动作：复制 {} 个（{}），删除 {} 个。\n{}",
                plan.actions.len(),
                copies,
                util::format_size(plan.copy_bytes()),
                deletes,
                if dry_run { "当前为预演模式，不会实际写入或删除。" } else { "删除动作仅作用于目标侧多余文件，请确认目录选择正确。" }
            );
            if super::confirm_dialog(ui.ctx(), &mut self.confirm_exec, "确认执行同步", &body, "开始同步", deletes > 0 && !dry_run) {
                self.start_exec(ui.ctx(), env);
            }
        }
        if let Some(report) = &self.report {
            let copied = report.copied;
            let updated = report.updated;
            let deleted = report.deleted;
            let skipped = report.skipped;
            let failed = report.failed;
            let bytes = report.bytes;
            let dry_run = report.dry_run;
            let cancelled = report.cancelled;
            let errors = report.errors.clone();
            section_card(ui, "执行结果", None, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if dry_run {
                        widgets::pill(ui, "预演", theme::WARN);
                    }
                    widgets::pill(ui, format!("复制 {copied}"), theme::SUCCESS);
                    widgets::pill(ui, format!("更新 {updated}"), theme::BLUE);
                    if deleted > 0 {
                        widgets::pill(ui, format!("删除 {deleted}"), theme::DANGER);
                    }
                    widgets::pill(ui, format!("跳过 {skipped}"), theme::TEXT_DIM);
                    if failed > 0 {
                        widgets::pill(ui, format!("失败 {failed}"), theme::DANGER);
                    }
                    if cancelled {
                        widgets::pill(ui, "已取消", theme::WARN);
                    }
                    widgets::pill(ui, util::format_size(bytes), theme::ACCENT);
                });
                for error in errors.iter().take(3) {
                    ui.label(RichText::new(error).size(10.5).color(theme::DANGER));
                }
            });
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        if let Some(plan) = job_take(&self.plan_job) {
            let copies = plan.copy_actions().count();
            let deletes = plan.delete_actions().count();
            if plan.errors.is_empty() {
                env.toast(format!("扫描完成 · 复制 {copies} · 删除 {deletes}"), ToastKind::Success);
                env.set_status(format!("文件同步 · 计划复制 {copies} 项，删除 {deletes} 项"));
            } else {
                env.toast(plan.errors.iter().take(2).cloned().collect::<Vec<_>>().join("；"), ToastKind::Warn);
            }
            self.plan = Some(plan);
            self.report = None;
        }
        if let Some(report) = job_take(&self.exec_job) {
            let message = format!(
                "同步完成 · 复制 {} · 更新 {} · 删除 {} · 失败 {}",
                report.copied, report.updated, report.deleted, report.failed
            );
            env.toast(message.clone(), if report.failed == 0 { ToastKind::Success } else { ToastKind::Warn });
            env.set_status(message);
            self.report = Some(report);
            self.plan = None;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                stat_row(
                    ui,
                    &[
                        ("同步模式", env.config.sync.mode.label().into(), theme::ACCENT),
                        ("并发线程", env.config.sync.concurrent.to_string(), theme::BLUE),
                        ("传输校验", if env.config.sync.verify { "BLAKE3" } else { "关闭" }.into(), theme::SUCCESS),
                        ("预演", if env.config.sync.dry_run { "开启" } else { "关闭" }.into(), if env.config.sync.dry_run { theme::WARN } else { theme::TEXT_DIM }),
                    ],
                );
                ui.add_space(10.0);
                self.draw_dirs(ui, env);
                self.draw_options(ui, env);
                ui.horizontal(|ui| {
                    let (running, _, _, _) = job_view(&self.plan_job);
                    if running {
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消扫描").clicked() {
                            job_cancel(&self.plan_job);
                        }
                    } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Search), "扫描并生成计划").clicked() {
                        self.start_plan(ui.ctx(), env);
                    }
                    super::small_muted(ui, "先生成计划再执行，镜像模式的删除动作会明确列出");
                });
                progress_row(ui, &self.plan_job);
                progress_row(ui, &self.exec_job);
                if self.plan.is_some() {
                    ui.add_space(8.0);
                    self.draw_plan(ui, env);
                } else if self.report.is_none() {
                    ui.add_space(8.0);
                    section_card(ui, "同步建议", None, |ui| {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Sync,
                            "没有待执行的计划",
                            "选择左右目录并扫描。首次同步大目录建议开启“预演”确认动作列表。",
                        );
                    });
                }
            });
    }
}

fn options_from(settings: &SyncSettings) -> SyncOptions {
    SyncOptions {
        left: PathBuf::from(settings.left.trim()),
        right: PathBuf::from(settings.right.trim()),
        mode: settings.mode,
        conflict: settings.conflict,
        include_patterns: settings.include_patterns.clone(),
        exclude_patterns: settings.exclude_patterns.clone(),
        regex_exclude: settings.regex_exclude.clone(),
        min_size_kb: settings.min_size_kb,
        max_size_mb: settings.max_size_mb,
        exclude_hidden: settings.exclude_hidden,
        buffer_mb: settings.buffer_mb,
        concurrent: settings.concurrent,
        unbuffered: settings.unbuffered,
        verify: settings.verify,
        follow_symlinks: settings.follow_symlinks,
        preserve_timestamps: settings.preserve_timestamps,
        auto_retry: settings.auto_retry,
        dry_run: settings.dry_run,
    }
}

fn apply_options(settings: &mut SyncSettings, options: SyncOptions) {
    settings.left = options.left.display().to_string();
    settings.right = options.right.display().to_string();
    settings.mode = options.mode;
    settings.conflict = options.conflict;
    settings.include_patterns = options.include_patterns;
    settings.exclude_patterns = options.exclude_patterns;
    settings.regex_exclude = options.regex_exclude;
    settings.min_size_kb = options.min_size_kb;
    settings.max_size_mb = options.max_size_mb;
    settings.exclude_hidden = options.exclude_hidden;
    settings.buffer_mb = options.buffer_mb;
    settings.concurrent = options.concurrent;
    settings.unbuffered = options.unbuffered;
    settings.verify = options.verify;
    settings.follow_symlinks = options.follow_symlinks;
    settings.preserve_timestamps = options.preserve_timestamps;
    settings.auto_retry = options.auto_retry;
    settings.dry_run = options.dry_run;
}
