use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::app::ToastKind;
use crate::cleaner::{self, CleanItem, CleanReport, CleanScan, CleanRule};
use crate::config::{AppConfig, CleanKind, CustomCleanRule};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{job_cancel, job_start, job_take, job_view, progress_row, section_card, small_muted, stat_row, Job, TabEnv};

struct GroupView {
    id: String,
    name: String,
    group: String,
    note: String,
    dangerous: bool,
    bytes: u64,
    file_count: usize,
    dir_count: usize,
    samples: Vec<PathBuf>,
}

pub struct CleanerTab {
    scan_job: Job<CleanScan>,
    delete_job: Job<(CleanReport, Option<String>)>,
    scan: Option<CleanScan>,
    groups: Vec<GroupView>,
    selected_rules: HashSet<String>,
    expanded_rule: Option<String>,
    confirm_delete: bool,
    rules_open: bool,
    last_report: Option<CleanReport>,
    show_all_groups: bool,
}

impl CleanerTab {
    pub fn new(_config: &AppConfig) -> Self {
        Self {
            scan_job: super::new_job(),
            delete_job: super::new_job(),
            scan: None,
            groups: Vec::new(),
            selected_rules: HashSet::new(),
            expanded_rule: None,
            confirm_delete: false,
            rules_open: false,
            last_report: None,
            show_all_groups: false,
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let rules = cleaner::active_rules(&env.config.cleaner);
        if rules.is_empty() {
            env.toast("没有启用的清理规则，请在“清理规则”中启用", ToastKind::Warn);
            return;
        }
        job_start(&self.scan_job, ctx, move |cancel, sink| {
            sink.set("扫描清理项目", 0, 0, "");
            let scan = cleaner::scan(&rules, cancel, &mut |progress| {
                sink.set(progress.stage, progress.done as u64, 0, progress.current);
            });
            Ok(scan)
        });
    }

    fn start_delete(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let Some(scan) = &self.scan else {
            return;
        };
        let items: Vec<CleanItem> = scan
            .items
            .iter()
            .filter(|item| self.selected_rules.contains(&item.rule_id))
            .cloned()
            .collect();
        if items.is_empty() {
            return;
        }
        let move_to_trash = env.config.cleaner.move_to_trash;
        let make_restore_point = env.config.cleaner.create_restore_point;
        job_start(&self.delete_job, ctx, move |cancel, sink| {
            let total = items.len() as u64;
            let restore_error = if make_restore_point {
                sink.set("创建系统还原点", 0, total, "需要管理员权限，可能需要几十秒");
                match cleaner::create_restore_point() {
                    Ok(()) => None,
                    Err(error) => Some(format!("系统还原点创建失败: {error}")),
                }
            } else {
                None
            };
            let report = cleaner::delete_items(&items, move_to_trash, cancel, &mut |progress| {
                sink.set(progress.stage, progress.done as u64, total, progress.current);
            });
            Ok((report, restore_error))
        });
    }

    fn rebuild_groups(&mut self, env: &TabEnv) {
        let Some(scan) = &self.scan else {
            self.groups.clear();
            return;
        };
        let rules: HashMap<String, CleanRule> = cleaner::active_rules(&env.config.cleaner)
            .into_iter()
            .map(|rule| (rule.id.clone(), rule))
            .collect();
        let mut groups: HashMap<String, GroupView> = HashMap::new();
        for item in &scan.items {
            let entry = groups.entry(item.rule_id.clone()).or_insert_with(|| {
                let meta = rules.get(&item.rule_id);
                GroupView {
                    id: item.rule_id.clone(),
                    name: item.rule_name.clone(),
                    group: item.group.clone(),
                    note: meta.map(|r| r.note.clone()).unwrap_or_default(),
                    dangerous: meta.map(|r| r.dangerous).unwrap_or(false),
                    bytes: 0,
                    file_count: 0,
                    dir_count: 0,
                    samples: Vec::new(),
                }
            });
            entry.bytes += item.size;
            if item.is_dir {
                entry.dir_count += 1;
            } else {
                entry.file_count += 1;
            }
            if entry.samples.len() < 12 {
                entry.samples.push(item.path.clone());
            }
        }
        let mut groups: Vec<GroupView> = groups.into_values().collect();
        groups.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
        self.groups = groups;
        self.selected_rules.clear();
        for group in &self.groups {
            if !group.dangerous {
                self.selected_rules.insert(group.id.clone());
            }
        }
    }

    fn selected(&self) -> (usize, usize, u64) {
        let mut bytes = 0;
        let mut files = 0;
        let mut dirs = 0;
        for group in &self.groups {
            if self.selected_rules.contains(&group.id) {
                bytes += group.bytes;
                files += group.file_count;
                dirs += group.dir_count;
            }
        }
        (files, dirs, bytes)
    }

    fn draw_summary(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        let Some(scan) = &self.scan else {
            return;
        };
        let (files, dirs, bytes) = self.selected();
        stat_row(
            ui,
            &[
                ("可释放空间", util::format_size(scan.total_bytes), theme::SUCCESS),
                ("扫描项目", format!("{} 文件 / {} 目录", scan.file_count, scan.dir_count), theme::ACCENT),
                ("已选择", format!("{files} 文件 / {dirs} 目录"), theme::BLUE),
                ("将释放", util::format_size(bytes), theme::WARN),
            ],
        );
        if scan.capped {
            ui.label(RichText::new("项目数量达到 250,000 上限，清理后建议重新扫描。").size(11.0).color(theme::WARN));
        }
        if scan.cancelled {
            ui.label(RichText::new("扫描已取消，结果可能不完整。").size(11.0).color(theme::WARN));
        }
        if !scan.errors.is_empty() {
            ui.label(
                RichText::new(format!("{} 个路径读取失败：{}", scan.errors.len(), scan.errors[0]))
                    .size(10.5)
                    .color(theme::dim(ui)),
            );
        }
        ui.add_space(8.0);
        section_card(ui, "扫描结果", Some("默认选中所有非敏感规则；敏感项目需手动勾选"), |ui| {
            ui.horizontal_wrapped(|ui| {
                if widgets::primary_button(ui, Some(widgets::ToolIcon::Broom), &format!("清理选中 · {}", util::format_size(bytes))).clicked() {
                    self.confirm_delete = true;
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Check), "全选安全项").clicked() {
                    for group in &self.groups {
                        if !group.dangerous {
                            self.selected_rules.insert(group.id.clone());
                        }
                    }
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空选择").clicked() {
                    self.selected_rules.clear();
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Refresh), "重新扫描").clicked() {
                    self.start_scan(ui.ctx(), env);
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Settings), "清理规则").clicked() {
                    self.rules_open = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.groups.len() > 40 && !self.show_all_groups {
                        if ui.small_button("显示全部").clicked() {
                            self.show_all_groups = true;
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if self.groups.is_empty() {
                widgets::empty_state(ui, widgets::ToolIcon::Broom, "没有发现可清理的项目", "缓存目录可能已经很干净；也可以检查规则是否启用。");
                return;
            }
            let limit = if self.show_all_groups { self.groups.len() } else { self.groups.len().min(40) };
            for group in self.groups.iter().take(limit) {
                let mut checked = self.selected_rules.contains(&group.id);
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .stroke(egui::Stroke::new(
                        1.0,
                        if group.dangerous { theme::WARN.gamma_multiply(0.4) } else { ui.visuals().widgets.noninteractive.bg_stroke.color },
                    ))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut checked, "").changed() {
                                if checked {
                                    self.selected_rules.insert(group.id.clone());
                                } else {
                                    self.selected_rules.remove(&group.id);
                                }
                            }
                            ui.label(RichText::new(&group.name).size(12.0).strong());
                            widgets::pill(ui, &group.group, theme::ACCENT);
                            if group.dangerous {
                                widgets::pill(ui, "敏感", theme::WARN);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(util::format_size(group.bytes)).size(11.0).color(theme::SUCCESS));
                                small_muted(ui, format!("{} 文件 · {} 目录", group.file_count, group.dir_count));
                                let expanded = self.expanded_rule.as_deref() == Some(group.id.as_str());
                                if ui.small_button(if expanded { "收起" } else { "查看" }).clicked() {
                                    self.expanded_rule = if expanded { None } else { Some(group.id.clone()) };
                                }
                            });
                        });
                        if !group.note.is_empty() {
                            small_muted(ui, &group.note);
                        }
                        if self.expanded_rule.as_deref() == Some(group.id.as_str()) {
                            for path in &group.samples {
                                let response = ui.label(RichText::new(super::truncate_path(&path.display().to_string(), 110)).size(10.0).color(theme::dim(ui)))
                                    .on_hover_text(path.display().to_string());
                                super::path_actions(response, path);
                            }
                            if group.file_count + group.dir_count > group.samples.len() {
                                small_muted(ui, format!("… 其余 {} 项未列出", group.file_count + group.dir_count - group.samples.len()));
                            }
                        }
                    });
                ui.add_space(3.0);
            }
            if limit < self.groups.len() {
                small_muted(ui, format!("仅显示前 {limit} 组，共 {} 组。", self.groups.len()));
            }
        });
        ui.add_space(6.0);
        if self.confirm_delete {
            let count = files + dirs;
            let action = if env.config.cleaner.move_to_trash { "移入回收站" } else { "永久删除" };
            let restore = if env.config.cleaner.create_restore_point { "\n将先尝试创建系统还原点（需要管理员权限）。" } else { "" };
            let body = format!(
                "即将{action} {count} 个缓存项目，预计释放 {}。\n不会删除个人文档；受保护的系统路径会自动跳过。{restore}",
                util::format_size(bytes)
            );
            if super::confirm_dialog(
                ui.ctx(),
                &mut self.confirm_delete,
                "确认清理",
                &body,
                action,
                !env.config.cleaner.move_to_trash,
            ) {
                self.start_delete(ui.ctx(), env);
            }
        }
        if let Some(report) = &self.last_report {
            let report = report.clone();
            section_card(ui, "上次清理", None, |ui| {
                ui.horizontal_wrapped(|ui| {
                    widgets::pill(ui, format!("删除文件 {}", report.deleted_files), theme::SUCCESS);
                    if report.deleted_dirs > 0 {
                        widgets::pill(ui, format!("删除目录 {}", report.deleted_dirs), theme::SUCCESS);
                    }
                    widgets::pill(ui, format!("释放 {}", util::format_size(report.freed)), theme::ACCENT);
                    if report.failed > 0 {
                        widgets::pill(ui, format!("失败 {}", report.failed), theme::DANGER);
                    }
                    if report.blocked > 0 {
                        widgets::pill(ui, format!("保护跳过 {}", report.blocked), theme::WARN);
                    }
                });
                for error in report.errors.iter().take(3) {
                    ui.label(RichText::new(error).size(10.5).color(theme::DANGER));
                }
            });
        }
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        section_card(ui, "清理设置", Some("默认使用回收站，可在误删后恢复"), |ui| {
            ui.horizontal_wrapped(|ui| {
                if widgets::toggle_switch(ui, &mut env.config.cleaner.move_to_trash).changed() {
                    env.mark_dirty();
                }
                small_muted(ui, "移入回收站");
                ui.add_space(10.0);
                if widgets::toggle_switch(ui, &mut env.config.cleaner.create_restore_point).changed() {
                    env.mark_dirty();
                }
                small_muted(ui, "清理前创建系统还原点");
            });
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                if widgets::toggle_switch(ui, &mut env.config.cleaner.schedule_enabled).changed() {
                    env.mark_dirty();
                }
                small_muted(ui, "每日自动清理安全缓存");
                ui.add_space(6.0);
                ui.label(RichText::new("时间").size(11.0).color(theme::dim(ui)));
                if ui
                    .add_sized(
                        [64.0, 24.0],
                        egui::TextEdit::singleline(&mut env.config.cleaner.schedule_time)
                            .hint_text("03:00")
                            .margin(egui::Vec2::new(6.0, 3.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
                if widgets::toggle_switch(ui, &mut env.config.cleaner.schedule_recycle).changed() {
                    env.mark_dirty();
                }
                small_muted(ui, "使用回收站");
                if !env.config.cleaner.last_run.is_empty() {
                    widgets::pill(ui, format!("上次 {}", env.config.cleaner.last_run), theme::TEXT_DIM);
                }
            });
            ui.horizontal(|ui| {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Settings), "管理清理规则").clicked() {
                    self.rules_open = true;
                }
                small_muted(ui, format!("当前启用 {} 条规则 · 内置规则共 {} 条", cleaner::active_rules(&env.config.cleaner).len(), cleaner::builtin_count()));
                if !env.config.cleaner.history.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(last) = env.config.cleaner.history.last() {
                            small_muted(ui, format!("最近：{last}"));
                        }
                    });
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let (running, _, _, _) = job_view(&self.scan_job);
            if running {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消扫描").clicked() {
                    job_cancel(&self.scan_job);
                }
            } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Broom), "扫描系统缓存").clicked() {
                self.start_scan(ui.ctx(), env);
            }
            super::small_muted(ui, "扫描只读取文件信息，不会改动任何数据");
        });
        progress_row(ui, &self.scan_job);
        progress_row(ui, &self.delete_job);
    }

    fn draw_rules_window(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        if !self.rules_open {
            return;
        }
        let mut open = true;
        egui::Window::new("清理规则")
            .id(egui::Id::new("cleaner_rules"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .max_size(egui::vec2(640.0, 640.0))
            .show(ctx, |ui| {
                ui.set_width(600.0);
                ui.label(RichText::new("内置规则").size(13.0).strong());
                small_muted(ui, "取消勾选即可跳过对应类别；标记为“敏感”的规则不会自动选中。");
                ui.add_space(4.0);
                let mut disabled = env.config.cleaner.disabled_builtin.clone();
                egui::ScrollArea::vertical()
                    .id_salt("builtin_rules")
                    .max_height(240.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut last_group = String::new();
                        let rules: Vec<CleanRule> = cleaner::builtin_rules().collect();
                        for rule in &rules {
                            if rule.group != last_group {
                                ui.add_space(4.0);
                                widgets::section_label(ui, &rule.group);
                                last_group = rule.group.clone();
                            }
                            ui.horizontal(|ui| {
                                let mut enabled = !disabled.contains(&rule.id);
                                if ui.checkbox(&mut enabled, "").changed() {
                                    if enabled {
                                        disabled.retain(|id| id != &rule.id);
                                    } else {
                                        disabled.push(rule.id.clone());
                                    }
                                }
                                ui.label(RichText::new(&rule.name).size(11.5));
                                if rule.dangerous {
                                    widgets::pill(ui, "敏感", theme::WARN);
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    small_muted(ui, &*rule.note);
                                });
                            });
                        }
                    });
                if disabled != env.config.cleaner.disabled_builtin {
                    env.config.cleaner.disabled_builtin = disabled;
                    env.mark_dirty();
                }
                ui.horizontal(|ui| {
                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Refresh), "恢复默认规则").clicked() {
                        env.config.cleaner.disabled_builtin.clear();
                        env.mark_dirty();
                    }
                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Plus), "添加自定义规则").clicked() {
                        let id = format!("custom-{}", chrono::Local::now().format("%Y%m%d%H%M%S"));
                        env.config.cleaner.custom_rules.push(CustomCleanRule {
                            id,
                            name: "我的清理规则".into(),
                            path: String::new(),
                            kind: CleanKind::Dir,
                            pattern: String::new(),
                            note: String::new(),
                            enabled: true,
                            dangerous: false,
                        });
                        env.mark_dirty();
                    }
                });
                ui.separator();
                ui.label(RichText::new("自定义规则").size(13.0).strong());
                let before_rules = env.config.cleaner.custom_rules.clone();
                let mut rules = std::mem::take(&mut env.config.cleaner.custom_rules);
                let mut remove: Option<usize> = None;
                if rules.is_empty() {
                    small_muted(ui, "尚未添加自定义规则。");
                }
                for (index, rule) in rules.iter_mut().enumerate() {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut rule.enabled, "");
                                ui.add_sized([150.0, 24.0], egui::TextEdit::singleline(&mut rule.name).hint_text("规则名称"));
                                egui::ComboBox::from_id_salt(("custom_kind", index))
                                    .selected_text(rule.kind.label())
                                    .width(96.0)
                                    .show_ui(ui, |ui| {
                                        for kind in [CleanKind::Dir, CleanKind::File, CleanKind::Glob] {
                                            ui.selectable_value(&mut rule.kind, kind, kind.label());
                                        }
                                    });
                                ui.checkbox(&mut rule.dangerous, "敏感");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("删除规则").clicked() {
                                        remove = Some(index);
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("路径").size(11.0).color(theme::dim(ui)));
                                let width = (ui.available_width() - 80.0).max(150.0);
                                ui.add_sized(
                                    [width, 24.0],
                                    egui::TextEdit::singleline(&mut rule.path)
                                        .hint_text("%LOCALAPPDATA%\\SomeApp\\Cache")
                                        .margin(egui::Vec2::new(7.0, 3.0)),
                                );
                                if ui.small_button("浏览").clicked() {
                                    if let Some(path) = super::pick_folder() {
                                        rule.path = path.display().to_string();
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("匹配").size(11.0).color(theme::dim(ui)));
                                ui.add_sized(
                                    [180.0, 24.0],
                                    egui::TextEdit::singleline(&mut rule.pattern)
                                        .hint_text(if rule.kind == CleanKind::Glob { "*.tmp;*.log" } else { "可留空" })
                                        .margin(egui::Vec2::new(7.0, 3.0)),
                                );
                                ui.label(RichText::new("说明").size(11.0).color(theme::dim(ui)));
                                ui.add_sized(
                                    [(ui.available_width() - 10.0).max(120.0), 24.0],
                                    egui::TextEdit::singleline(&mut rule.note).hint_text("清理说明"),
                                );
                            });
                        });
                    ui.add_space(3.0);
                }
                if let Some(index) = remove {
                    rules.remove(index);
                }
                let changed = rules != before_rules;
                env.config.cleaner.custom_rules = rules;
                if changed {
                    env.mark_dirty();
                }
            });
        self.rules_open = open;
    }
}

impl CleanerTab {
    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        if let Some(scan) = job_take(&self.scan_job) {
            let bytes = scan.total_bytes;
            env.toast(
                format!("扫描完成 · 发现 {} 个可清理项目 · {}", scan.items.len(), util::format_size(bytes)),
                ToastKind::Success,
            );
            env.set_status(format!("垃圾清理 · {} 个项目，可释放 {}", scan.items.len(), util::format_size(bytes)));
            self.scan = Some(scan);
            self.last_report = None;
            self.show_all_groups = false;
            self.rebuild_groups(env);
        }
        if let Some((report, restore_error)) = job_take(&self.delete_job) {
            let message = format!(
                "清理完成 · 删除 {} 文件 / {} 目录 · 释放 {}",
                report.deleted_files,
                report.deleted_dirs,
                util::format_size(report.freed)
            );
            env.toast(message.clone(), if report.failed == 0 { ToastKind::Success } else { ToastKind::Warn });
            env.set_status(message);
            env.config.cleaner.last_run = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
            env.config.cleaner.history.push(format!(
                "{} · 释放 {} · {} 项",
                env.config.cleaner.last_run,
                util::format_size(report.freed),
                report.deleted_files + report.deleted_dirs
            ));
            while env.config.cleaner.history.len() > 20 {
                env.config.cleaner.history.remove(0);
            }
            env.mark_dirty();
            if let Some(error) = restore_error {
                env.toast(error, ToastKind::Warn);
            }
            self.last_report = Some(report);
            self.scan = None;
            self.groups.clear();
            self.selected_rules.clear();
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let enabled_rules = cleaner::active_rules(&env.config.cleaner).len();
                stat_row(
                    ui,
                    &[
                        ("启用规则", enabled_rules.to_string(), theme::ACCENT),
                        ("内置规则", cleaner::builtin_count().to_string(), theme::BLUE),
                        ("清理方式", if env.config.cleaner.move_to_trash { "回收站" } else { "永久删除" }.into(), if env.config.cleaner.move_to_trash { theme::SUCCESS } else { theme::WARN }),
                        ("定时清理", if env.config.cleaner.schedule_enabled { env.config.cleaner.schedule_time.clone() } else { "关闭".into() }, theme::TEXT_DIM),
                    ],
                );
                ui.add_space(10.0);
                self.draw_options(ui, env);
                if self.scan.is_some() {
                    ui.add_space(8.0);
                    self.draw_summary(ui, env);
                } else if self.last_report.is_none() {
                    ui.add_space(8.0);
                    section_card(ui, "清洁建议", None, |ui| {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Broom,
                            "安全释放磁盘空间",
                            "扫描系统、浏览器、开发工具与应用缓存；受保护路径会被自动跳过，默认移入回收站。",
                        );
                    });
                }
                ui.add_space(6.0);
            });
        self.draw_rules_window(ui.ctx(), env);
    }
}
