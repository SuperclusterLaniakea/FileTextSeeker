use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use walkdir::WalkDir;

use crate::app::ToastKind;
use crate::config::{AppConfig, CollisionRule, RenameMode};
use crate::rename::{self, CaseMode, DateSource, ExtAction, OpKind, PatternPos, PlanAction, RenameOp, RenamePlan, RenameRequest};
use crate::theme;
use crate::widgets;

use super::{job_cancel, job_start, job_take, job_view, pick_folder, progress_row, section_card, stat_row, toggle_chip, Job, TabEnv};

pub struct RenameTab {
    dir: String,
    recursive: bool,
    ext_filter: String,
    ops: Vec<RenameOp>,
    mode: RenameMode,
    target_dir: String,
    collision: CollisionRule,
    preserve_structure: bool,
    plan: Option<RenamePlan>,
    plan_job: Job<RenamePlan>,
    exec_job: Job<(rename::ExecReport, Option<PathBuf>)>,
    last_report: Option<rename::ExecReport>,
    last_undo: Option<PathBuf>,
    confirm_exec: bool,
    show_all: bool,
    last_request: Option<RenameRequest>,
}

impl RenameTab {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            dir: config.rename.last_dir.clone(),
            recursive: config.rename.add_recursive,
            ext_filter: config.rename.extensions_filter.clone(),
            ops: vec![RenameOp::new(OpKind::Template("{name}".into()))],
            mode: config.rename.mode,
            target_dir: config.rename.target_dir.clone(),
            collision: config.rename.collision,
            preserve_structure: config.rename.preserve_structure,
            plan: None,
            plan_job: super::new_job(),
            exec_job: super::new_job(),
            last_report: None,
            last_undo: None,
            confirm_exec: false,
            show_all: false,
            last_request: None,
        }
    }

    fn sync_config(&mut self, env: &mut TabEnv) {
        let settings = &mut env.config.rename;
        if settings.last_dir != self.dir
            || settings.add_recursive != self.recursive
            || settings.mode != self.mode
            || settings.target_dir != self.target_dir
            || settings.collision != self.collision
            || settings.preserve_structure != self.preserve_structure
            || settings.extensions_filter != self.ext_filter
        {
            settings.last_dir = self.dir.clone();
            settings.add_recursive = self.recursive;
            settings.mode = self.mode;
            settings.target_dir = self.target_dir.clone();
            settings.collision = self.collision;
            settings.preserve_structure = self.preserve_structure;
            settings.extensions_filter = self.ext_filter.clone();
            env.mark_dirty();
        }
    }

    fn build_request(&self) -> Result<RenameRequest, String> {
        let root = self.dir.trim();
        if root.is_empty() {
            return Err("请选择文件所在目录".into());
        }
        let root = PathBuf::from(root);
        if !root.is_dir() {
            return Err(format!("目录不存在: {}", root.display()));
        }
        let extensions: Vec<String> = self
            .ext_filter
            .split([',', ';', ' '])
            .map(|s| s.trim().trim_start_matches('.').to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let mut files: Vec<PathBuf> = Vec::new();
        let walker = if self.recursive {
            WalkDir::new(&root)
        } else {
            WalkDir::new(&root).max_depth(1)
        };
        for entry in walker.follow_links(false).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            if !extensions.is_empty() {
                let ext = entry
                    .path()
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !extensions.contains(&ext) {
                    continue;
                }
            }
            files.push(entry.path().to_path_buf());
        }
        files.sort();
        if files.is_empty() {
            return Err("目录中没有可处理的文件".into());
        }
        Ok(RenameRequest {
            files,
            root,
            ops: self.ops.clone(),
            mode: self.mode,
            target_dir: PathBuf::from(self.target_dir.trim()),
            collision: self.collision,
            preserve_structure: self.preserve_structure,
        })
    }

    fn start_plan(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        self.sync_config(env);
        let request = match self.build_request() {
            Ok(request) => request,
            Err(error) => {
                env.toast(error, ToastKind::Warn);
                return;
            }
        };
        if request.ops.iter().all(|op| !op.enabled) {
            env.toast("请至少启用一个重命名步骤", ToastKind::Warn);
            return;
        }
        self.last_request = Some(request.clone());
        job_start(&self.plan_job, ctx, move |cancel, sink| {
            sink.set("计算新名称", 0, request.files.len() as u64, "");
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("已取消".into());
            }
            let plan = rename::build_plan(&request);
            Ok(plan)
        });
    }

    fn start_exec(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let Some(request) = self.last_request.clone() else {
            env.toast("请先预览重命名计划", ToastKind::Warn);
            return;
        };
        let Some(plan) = self.plan.clone() else {
            return;
        };
        let undo_path = env
            .data_dir
            .join("undo")
            .join(format!("rename-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        let undo_path_for_job = undo_path.clone();
        job_start(&self.exec_job, ctx, move |cancel, sink| {
            sink.set("执行重命名", 0, plan.ready().count() as u64, "");
            let report = rename::execute(&plan, &request, cancel);
            let saved = if report.moves.is_empty() {
                None
            } else {
                rename::save_undo(&undo_path_for_job, request.mode, &report.moves)
                    .ok()
                    .map(|_| undo_path_for_job)
            };
            Ok((report, saved))
        });
    }

    fn run_undo(&mut self, env: &mut TabEnv) {
        let Some(path) = self.last_undo.clone() else {
            return;
        };
        match rename::load_undo(&path) {
            Ok(log) => {
                let report = rename::undo(&log);
                if report.failed == 0 {
                    env.toast(format!("已撤销 {} 项操作", report.done), ToastKind::Success);
                } else {
                    env.toast(
                        format!("撤销完成 {} 项，失败 {} 项", report.done, report.failed),
                        ToastKind::Warn,
                    );
                }
                self.last_undo = None;
                self.last_report = Some(report);
            }
            Err(error) => env.toast(format!("读取撤销记录失败: {error}"), ToastKind::Error),
        }
    }

    fn draw_setup(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        section_card(ui, "文件范围", Some("目录、递归与扩展名过滤决定参与处理的文件"), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("源目录").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 100.0).max(200.0);
                if ui
                    .add_sized(
                        [width, 28.0],
                        egui::TextEdit::singleline(&mut self.dir)
                            .hint_text("选择包含待重命名文件的目录")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "浏览").clicked() {
                    if let Some(path) = pick_folder() {
                        self.dir = path.display().to_string();
                        env.mark_dirty();
                    }
                }
            });
            ui.horizontal(|ui| {
                if widgets::toggle_switch(ui, &mut self.recursive).changed() {
                    env.mark_dirty();
                }
                super::small_muted(ui, "包含子目录");
                ui.add_space(8.0);
                if widgets::toggle_switch(ui, &mut self.preserve_structure).changed() {
                    env.mark_dirty();
                }
                super::small_muted(ui, "保留目录结构");
                ui.add_space(8.0);
                ui.label(RichText::new("扩展名").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 70.0).max(140.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut self.ext_filter)
                            .hint_text("留空表示全部，例如 jpg,png,mp4")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
        });
        ui.add_space(8.0);

        section_card(ui, "处理方式", None, |ui| {
            ui.horizontal(|ui| {
                for mode in [RenameMode::Rename, RenameMode::Copy, RenameMode::Move] {
                    if toggle_chip(ui, self.mode == mode, mode.label()) {
                        self.mode = mode;
                        env.mark_dirty();
                    }
                }
                ui.add_space(8.0);
                super::small_muted(ui, "同名冲突");
                egui::ComboBox::from_id_salt("rename_collision")
                    .selected_text(self.collision.label())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for rule in [CollisionRule::AutoNumber, CollisionRule::Skip, CollisionRule::Overwrite] {
                            if ui.selectable_value(&mut self.collision, rule, rule.label()).changed() {
                                env.mark_dirty();
                            }
                        }
                    });
            });
            if self.mode != RenameMode::Rename {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("目标目录").size(11.5).color(theme::dim(ui)));
                    let width = (ui.available_width() - 100.0).max(200.0);
                    if ui
                        .add_sized(
                            [width, 28.0],
                            egui::TextEdit::singleline(&mut self.target_dir)
                                .hint_text("复制 / 移动的目标目录")
                                .margin(egui::Vec2::new(9.0, 5.0)),
                        )
                        .changed()
                    {
                        env.mark_dirty();
                    }
                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "浏览").clicked() {
                        if let Some(path) = pick_folder() {
                            self.target_dir = path.display().to_string();
                            env.mark_dirty();
                        }
                    }
                });
            }
        });
        ui.add_space(8.0);

        section_card(ui, "规则流水线", Some("按从上到下的顺序依次应用；每个步骤都可单独停用"), |ui| {
            let mut ops = std::mem::take(&mut self.ops);
            let mut removed: Option<usize> = None;
            let mut move_to: Option<(usize, isize)> = None;
            for (index, op) in ops.iter_mut().enumerate() {
                let frame = egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 7));
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        widgets::pill(ui, format!("{}", index + 1), theme::ACCENT);
                        widgets::toggle_switch(ui, &mut op.enabled);
                        ui.label(RichText::new(op.kind.label()).size(12.5).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("删除").clicked() {
                                removed = Some(index);
                            }
                            if ui.small_button("下移").clicked() {
                                move_to = Some((index, 1));
                            }
                            if ui.small_button("上移").clicked() {
                                move_to = Some((index, -1));
                            }
                        });
                    });
                    if op.enabled {
                        ui.add_space(3.0);
                        op_controls(ui, &mut op.kind);
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(index) = removed {
                if index < ops.len() {
                    ops.remove(index);
                }
            } else if let Some((index, delta)) = move_to {
                let other = index as isize + delta;
                if other >= 0 && (other as usize) < ops.len() {
                    ops.swap(index, other as usize);
                }
            }
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                super::small_muted(ui, "添加步骤");
                for kind in default_kinds() {
                    if ui.small_button(kind.label()).clicked() {
                        ops.push(RenameOp::new(kind));
                    }
                }
            });
            self.ops = ops;
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let (running, _, _, _) = job_view(&self.plan_job);
            if running {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消").clicked() {
                    job_cancel(&self.plan_job);
                }
            } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Play), "预览新名称").clicked() {
                self.start_plan(ui.ctx(), env);
            }
            super::small_muted(ui, "预览不会修改任何文件");
        });
        progress_row(ui, &self.plan_job);
    }

    fn draw_plan(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        let ready = plan.ready().count();
        let unchanged = plan.items.iter().filter(|i| i.action == PlanAction::Unchanged).count();
        let skipped = plan.items.iter().filter(|i| i.action == PlanAction::Skipped).count();
        stat_row(
            ui,
            &[
                ("待处理", ready.to_string(), theme::ACCENT),
                ("保持不变", unchanged.to_string(), theme::TEXT_DIM),
                ("跳过", skipped.to_string(), theme::WARN),
                ("冲突", plan.conflicts.to_string(), theme::DANGER),
            ],
        );
        ui.add_space(8.0);
        section_card(ui, "重命名预览", Some("确认无误后执行；原地重命名支持一键撤销"), |ui| {
            ui.horizontal(|ui| {
                let action_label = match self.mode {
                    RenameMode::Rename => "执行重命名",
                    RenameMode::Copy => "执行复制",
                    RenameMode::Move => "执行移动",
                };
                if ready > 0 {
                    if widgets::primary_button(ui, Some(widgets::ToolIcon::Pencil), action_label).clicked() {
                        self.confirm_exec = true;
                    }
                } else {
                    widgets::pill(ui, "没有需要变更的文件", theme::SUCCESS);
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Refresh), "重新预览").clicked() {
                    self.start_plan(ui.ctx(), env);
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "关闭预览").clicked() {
                    self.plan = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if plan.items.len() > 400 && !self.show_all {
                        if ui.small_button("显示全部").clicked() {
                            self.show_all = true;
                        }
                    }
                });
            });
            if !plan.errors.is_empty() {
                ui.add_space(3.0);
                for error in plan.errors.iter().take(4) {
                    ui.label(RichText::new(error).size(11.0).color(theme::DANGER));
                }
            }
            ui.add_space(4.0);
            let limit = if self.show_all { plan.items.len() } else { plan.items.len().min(400) };
            TableBuilder::new(ui)
                .striped(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(170.0).at_least(90.0).clip(true))
                .column(Column::initial(42.0).clip(true))
                .column(Column::remainder().at_least(170.0).clip(true))
                .column(Column::initial(62.0).clip(true))
                .column(Column::initial(120.0).at_least(60.0).clip(true))
                .max_scroll_height(380.0)
                .header(24.0, |mut header| {
                    for title in ["原名称", "", "新名称", "状态", "备注"] {
                        header.col(|ui| {
                            ui.label(RichText::new(title).size(11.0).strong().color(theme::dim(ui)));
                        });
                    }
                })
                .body(|body| {
                    body.rows(24.0, limit, |mut row| {
                        let item = &plan.items[row.index()];
                        row.col(|ui| {
                            let original = ui.add(
                                egui::Label::new(RichText::new(&item.original_name).size(11.0))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text(item.source.display().to_string());
                            super::path_actions(original, &item.source);
                        });
                        row.col(|ui| {
                            ui.label(RichText::new("→").size(11.0).color(theme::dim(ui)));
                        });
                        row.col(|ui| {
                            let color = if item.action == PlanAction::Ready { theme::TEXT } else { theme::TEXT_DIM };
                            ui.label(RichText::new(&item.new_name).size(11.0).color(color))
                                .on_hover_text(item.target.display().to_string());
                        });
                        row.col(|ui| {
                            let (text, color) = match item.action {
                                PlanAction::Ready => ("待处理", theme::SUCCESS),
                                PlanAction::Unchanged => ("不变", theme::TEXT_DIM),
                                PlanAction::Skipped => ("跳过", theme::WARN),
                            };
                            ui.label(RichText::new(text).size(10.5).color(color));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(super::truncate_path(&item.note, 40)).size(10.5).color(theme::dim(ui)));
                        });
                        super::path_actions(row.response(), &item.source);
                    });
                });
            if limit < plan.items.len() {
                super::small_muted(ui, format!("仅显示前 {limit} 项，共 {} 项。", plan.items.len()));
            }
        });
        ui.add_space(6.0);
        if self.confirm_exec {
            let action = match self.mode {
                RenameMode::Rename => "重命名",
                RenameMode::Copy => "复制并重命名",
                RenameMode::Move => "移动并重命名",
            };
            let body = format!(
                "即将{action} {ready} 个文件。\n执行完成后可在“最近一次执行”中撤销本次操作。\n请确认预览结果符合预期。"
            );
            if super::confirm_dialog(
                ui.ctx(),
                &mut self.confirm_exec,
                "确认执行",
                &body,
                "开始执行",
                self.mode != RenameMode::Copy,
            ) {
                self.start_exec(ui.ctx(), env);
            }
        }
    }

    fn draw_last_report(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        let Some(report) = &self.last_report else {
            return;
        };
        let failed = report.failed;
        let done = report.done;
        let cancelled = report.cancelled;
        let errors = report.errors.clone();
        let can_undo = self.last_undo.is_some();
        let mut undo = false;
        section_card(ui, "最近一次执行", None, |ui| {
            ui.horizontal(|ui| {
                widgets::pill(ui, format!("成功 {done}"), theme::SUCCESS);
                if failed > 0 {
                    widgets::pill(ui, format!("失败 {failed}"), theme::DANGER);
                }
                if cancelled {
                    widgets::pill(ui, "已取消", theme::WARN);
                }
                if can_undo && widgets::ghost_button(ui, Some(widgets::ToolIcon::Refresh), "撤销本次操作").clicked() {
                    undo = true;
                }
            });
            for error in errors.iter().take(3) {
                ui.label(RichText::new(error).size(10.5).color(theme::DANGER));
            }
        });
        if undo {
            self.run_undo(env);
        }
    }
}

impl RenameTab {
    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        self.sync_config(env);
        if let Some(plan) = job_take(&self.plan_job) {
            let ready = plan.ready().count();
            if plan.errors.is_empty() {
                env.toast(format!("预览完成 · {ready} 个文件待处理"), ToastKind::Success);
                env.set_status(format!("批量重命名 · {ready} 个文件待处理"));
            } else {
                env.toast(plan.errors.join("；"), ToastKind::Warn);
            }
            let has_ready = ready > 0;
            self.plan = Some(plan);
            if !has_ready {
                self.confirm_exec = false;
            }
        }
        if let Some((report, undo)) = job_take(&self.exec_job) {
            let message = if report.failed == 0 {
                format!("重命名完成 · 成功处理 {} 个文件", report.done)
            } else {
                format!("重命名完成 · 成功 {} / 失败 {}", report.done, report.failed)
            };
            env.toast(message.clone(), if report.failed == 0 { ToastKind::Success } else { ToastKind::Warn });
            env.set_status(format!("批量重命名 · {message}"));
            self.last_report = Some(report);
            self.last_undo = undo;
            self.plan = None;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let enabled_ops = self.ops.iter().filter(|op| op.enabled).count();
                stat_row(
                    ui,
                    &[
                        ("启用的步骤", enabled_ops.to_string(), theme::ACCENT),
                        ("处理模式", self.mode.label().into(), theme::BLUE),
                        ("冲突策略", self.collision.label().into(), theme::WARN),
                        ("可撤销", if self.last_undo.is_some() { "是" } else { "—" }.into(), theme::SUCCESS),
                    ],
                );
                ui.add_space(10.0);
                self.draw_setup(ui, env);
                if self.plan.is_some() {
                    ui.add_space(8.0);
                    self.draw_plan(ui, env);
                } else {
                    ui.add_space(8.0);
                    section_card(ui, "使用提示", None, |ui| {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Pencil,
                            "规则流水线已准备",
                            "选择目录并添加步骤，先预览再执行；原地重命名会自动保存撤销记录。",
                        );
                    });
                }
                if self.last_report.is_some() {
                    ui.add_space(8.0);
                    self.draw_last_report(ui, env);
                }
            });
    }
}

fn default_kinds() -> Vec<OpKind> {
    vec![
        OpKind::Replace { find: String::new(), replace: String::new(), regex: false, case_sensitive: false },
        OpKind::Delete { find: String::new(), regex: false, case_sensitive: false },
        OpKind::Insert { text: String::new(), at: 0, from_end: false },
        OpKind::Prefix(String::new()),
        OpKind::Suffix(String::new()),
        OpKind::Case(CaseMode::default()),
        OpKind::Trim { collapse_spaces: true },
        OpKind::Numbering {
            start: 1,
            step: 1,
            pad: 3,
            separator: "-".into(),
            position: PatternPos::Start,
            reset_per_dir: false,
        },
        OpKind::Date {
            source: DateSource::Modified,
            format: "%Y%m%d".into(),
            separator: "-".into(),
            position: PatternPos::End,
        },
        OpKind::Extension { action: ExtAction::Set, value: String::new() },
        OpKind::Template("{name}".into()),
    ]
}

fn op_controls(ui: &mut egui::Ui, kind: &mut OpKind) {
    match kind {
        OpKind::Replace { find, replace, regex, case_sensitive } => {
            ui.horizontal(|ui| {
                text_input(ui, "查找", find, 120.0, "");
                text_input(ui, "替换为", replace, 120.0, "");
                ui.checkbox(regex, "正则");
                ui.checkbox(case_sensitive, "区分大小写");
            });
        }
        OpKind::Delete { find, regex, case_sensitive } => {
            ui.horizontal(|ui| {
                text_input(ui, "删除内容", find, 200.0, "");
                ui.checkbox(regex, "正则");
                ui.checkbox(case_sensitive, "区分大小写");
            });
        }
        OpKind::Insert { text, at, from_end } => {
            ui.horizontal(|ui| {
                text_input(ui, "插入文本", text, 180.0, "");
                ui.label(RichText::new("位置").size(11.0).color(theme::dim(ui)));
                ui.add(egui::DragValue::new(at).range(0..=4096));
                ui.checkbox(from_end, "从末尾计数");
            });
        }
        OpKind::Prefix(text) => {
            ui.horizontal(|ui| {
                text_input(ui, "前缀", text, 220.0, "例如 2024-");
            });
        }
        OpKind::Suffix(text) => {
            ui.horizontal(|ui| {
                text_input(ui, "后缀", text, 220.0, "例如 -备份");
            });
        }
        OpKind::Case(mode) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("大小写风格").size(11.0).color(theme::dim(ui)));
                egui::ComboBox::from_id_salt("case_mode")
                    .selected_text(mode.label())
                    .show_ui(ui, |ui| {
                        for option in CaseMode::ALL {
                            ui.selectable_value(mode, option, option.label());
                        }
                    });
            });
        }
        OpKind::Trim { collapse_spaces } => {
            ui.horizontal(|ui| {
                ui.checkbox(collapse_spaces, "把连续空格合并为一个");
                super::small_muted(ui, "始终会去除首尾空白");
            });
        }
        OpKind::Numbering { start, step, pad, separator, position, reset_per_dir } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("起始").size(11.0).color(theme::dim(ui)));
                ui.add(egui::DragValue::new(start).range(0..=1_000_000));
                ui.label(RichText::new("步长").size(11.0).color(theme::dim(ui)));
                ui.add(egui::DragValue::new(step).range(1..=10_000));
                ui.label(RichText::new("补零").size(11.0).color(theme::dim(ui)));
                ui.add(egui::DragValue::new(pad).range(0..=10));
                text_input(ui, "分隔符", separator, 60.0, "-");
                position_combo(ui, "numbering_pos", position);
                ui.checkbox(reset_per_dir, "每个目录重新计数");
            });
        }
        OpKind::Date { source, format, separator, position } => {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("date_source")
                    .selected_text(source.label())
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        for option in [DateSource::Modified, DateSource::Created, DateSource::Accessed] {
                            ui.selectable_value(source, option, option.label());
                        }
                    });
                text_input(ui, "格式", format, 110.0, "%Y%m%d");
                text_input(ui, "分隔符", separator, 60.0, "-");
                position_combo(ui, "date_pos", position);
            });
        }
        OpKind::Extension { action, value } => {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("ext_action")
                    .selected_text(action.label())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for option in [ExtAction::Set, ExtAction::Remove, ExtAction::Lower, ExtAction::Upper] {
                            ui.selectable_value(action, option, option.label());
                        }
                    });
                if *action == ExtAction::Set {
                    text_input(ui, "新扩展名", value, 100.0, "例如 txt");
                }
            });
        }
        OpKind::Template(template) => {
            ui.horizontal(|ui| {
                text_input(ui, "模板", template, 300.0, "{name}-{date}");
                super::small_muted(ui, "可用：{name} {original} {ext} {parent} {index} {date} {time}");
            });
        }
    }
}

fn position_combo(ui: &mut egui::Ui, id: &str, position: &mut PatternPos) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(position.label())
        .width(84.0)
        .show_ui(ui, |ui| {
            for option in [PatternPos::Start, PatternPos::End] {
                ui.selectable_value(position, option, option.label());
            }
        });
}

fn text_input(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32, hint: &str) -> bool {
    ui.label(RichText::new(label).size(11.0).color(theme::dim(ui)));
    let width = width.min((ui.available_width() - 70.0).max(70.0));
    ui.add_sized(
        [width, 24.0],
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .margin(egui::Vec2::new(7.0, 3.0)),
    )
    .changed()
}
