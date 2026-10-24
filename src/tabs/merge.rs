use std::path::PathBuf;

use eframe::egui::{self, RichText};

use crate::app::ToastKind;
use crate::config::{AppConfig, MergeFormat};
use crate::merge::{self, MergeOptions, MergeOutput};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{job_cancel, job_start, job_take, job_view, pick_folder, progress_row, section_card, stat_row, toggle_chip, Job, TabEnv};

pub struct MergeTab {
    job: Job<MergeOutput>,
    output: Option<MergeOutput>,
    preview: String,
    show_all_skipped: bool,
}

impl MergeTab {
    pub fn new(_config: &AppConfig) -> Self {
        Self {
            job: super::new_job(),
            output: None,
            preview: String::new(),
            show_all_skipped: false,
        }
    }

    fn options(env: &TabEnv) -> MergeOptions {
        let settings = &env.config.merge;
        MergeOptions {
            dir: PathBuf::from(settings.dir.trim()),
            extensions: merge::parse_list(&settings.extensions),
            excludes: merge::parse_list(&settings.excludes),
            format: settings.format,
            include_tree: settings.include_tree,
            include_headers: settings.include_headers,
            include_line_numbers: settings.include_line_numbers,
            max_file_kb: settings.max_file_kb,
        }
    }

    fn start(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let opts = Self::options(env);
        if opts.dir.as_os_str().is_empty() {
            env.toast("请选择代码目录", ToastKind::Warn);
            return;
        }
        if !opts.dir.is_dir() {
            env.toast("代码目录不存在", ToastKind::Warn);
            return;
        }
        env.config.merge.dir = opts.dir.display().to_string();
        env.mark_dirty();
        self.output = None;
        self.preview.clear();
        job_start(&self.job, ctx, move |cancel, sink| {
            sink.set("收集代码文件", 0, 0, opts.dir.display().to_string());
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("已取消".into());
            }
            let mut output = merge::merge(&opts)?;
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("已取消".into());
            }
            if output.files == 0 {
                return Err("目录中没有匹配的文件，请检查扩展名与排除规则".into());
            }
            output.text.reserve(0);
            Ok(output)
        });
    }

    fn save_output(&mut self, env: &mut TabEnv) {
        let Some(output) = &self.output else {
            return;
        };
        let (name, filter_name, extensions) = match env.config.merge.format {
            MergeFormat::Markdown => ("merged.md", "Markdown", vec!["md"]),
            MergeFormat::Plain => ("merged.txt", "文本文件", vec!["txt"]),
        };
        let Some(path) = super::save_file(name, filter_name, &extensions) else {
            return;
        };
        match merge::save(&path, &output.text) {
            Ok(()) => {
                env.toast(format!("合并文档已保存到 {}", path.display()), ToastKind::Success);
                env.set_status(format!("代码合并 · 已保存 {}", path.display()));
            }
            Err(error) => env.toast(format!("保存失败: {error}"), ToastKind::Error),
        }
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        section_card(ui, "代码目录", Some("递归收集文本文件，跳过二进制、隐藏依赖与超大文件"), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("目录").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 100.0).max(200.0);
                if ui
                    .add_sized(
                        [width, 28.0],
                        egui::TextEdit::singleline(&mut env.config.merge.dir)
                            .hint_text("例如 E:\\Projects\\my-app")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "浏览").clicked() {
                    if let Some(path) = pick_folder() {
                        env.config.merge.dir = path.display().to_string();
                        env.mark_dirty();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("扩展名").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 90.0).max(160.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut env.config.merge.extensions)
                            .hint_text("rs,py,md")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("排除目录").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 90.0).max(160.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut env.config.merge.excludes)
                            .hint_text("node_modules,target,.git")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
        });
        ui.add_space(8.0);
        section_card(ui, "输出格式", None, |ui| {
            ui.horizontal_wrapped(|ui| {
                for format in [MergeFormat::Markdown, MergeFormat::Plain] {
                    if toggle_chip(ui, env.config.merge.format == format, format.label()) {
                        env.config.merge.format = format;
                        env.mark_dirty();
                    }
                }
                ui.add_space(8.0);
                if toggle_chip(ui, env.config.merge.include_tree, "目录树") {
                    env.config.merge.include_tree = !env.config.merge.include_tree;
                    env.mark_dirty();
                }
                if toggle_chip(ui, env.config.merge.include_headers, "文件标题") {
                    env.config.merge.include_headers = !env.config.merge.include_headers;
                    env.mark_dirty();
                }
                if toggle_chip(ui, env.config.merge.include_line_numbers, "行号") {
                    env.config.merge.include_line_numbers = !env.config.merge.include_line_numbers;
                    env.mark_dirty();
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("单文件上限").size(11.5).color(theme::dim(ui)));
                ui.add(egui::DragValue::new(&mut env.config.merge.max_file_kb).range(1..=102_400).suffix(" KB"));
                super::small_muted(ui, "超过上限的文件会被跳过，避免生成过大的合并文档");
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let (running, _, _, _) = job_view(&self.job);
            if running {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消").clicked() {
                    job_cancel(&self.job);
                }
            } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Layers), "生成合并文档").clicked() {
                self.start(ui.ctx(), env);
            }
            super::small_muted(ui, "所有内容仅在本机读取与生成");
        });
        progress_row(ui, &self.job);
    }

    fn draw_output(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        let Some(output) = &self.output else {
            return;
        };
        stat_row(
            ui,
            &[
                ("合并文件", output.files.to_string(), theme::ACCENT),
                ("总行数", output.lines.to_string(), theme::BLUE),
                ("文档体积", util::format_size(output.bytes), theme::SUCCESS),
                ("跳过", output.skipped.len().to_string(), theme::WARN),
            ],
        );
        ui.add_space(8.0);
        let mut save_clicked = false;
        section_card(ui, "合并结果", Some("预览截取前 400 行，保存时输出完整内容"), |ui| {
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, Some(widgets::ToolIcon::Export), "保存文档").clicked() {
                    save_clicked = true;
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Copies), "复制全文").clicked() {
                    ui.ctx().copy_text(output.text.clone());
                    env.toast("合并文档已复制到剪贴板", ToastKind::Success);
                }
                if !output.errors.is_empty() {
                    widgets::pill(ui, format!("读取错误 {}", output.errors.len()), theme::DANGER);
                }
            });
            if !output.skipped.is_empty() {
                ui.add_space(3.0);
                let show = if self.show_all_skipped { output.skipped.len() } else { output.skipped.len().min(6) };
                for skipped in output.skipped.iter().take(show) {
                    ui.label(
                        RichText::new(format!("跳过 {} · {}", skipped.rel, skipped.reason))
                            .size(10.5)
                            .color(theme::WARN),
                    );
                }
                if output.skipped.len() > show {
                    if ui.small_button(format!("展开其余 {} 项", output.skipped.len() - show)).clicked() {
                        self.show_all_skipped = true;
                    }
                }
            }
            if !output.errors.is_empty() {
                for error in output.errors.iter().take(3) {
                    ui.label(RichText::new(error).size(10.5).color(theme::DANGER));
                }
            }
            ui.add_space(4.0);
            let mut preview = self.preview.clone();
            egui::ScrollArea::vertical()
                .id_salt("merge_preview")
                .max_height(420.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut preview)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                });
        });
        if save_clicked {
            self.save_output(env);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        if let Some(output) = job_take(&self.job) {
            self.preview = build_preview(&output.text, 400);
            env.toast(
                format!("合并完成 · {} 个文件 · {} 行", output.files, output.lines),
                ToastKind::Success,
            );
            env.set_status(format!("代码合并 · {} 个文件 / {} 行 / {}", output.files, output.lines, util::format_size(output.bytes)));
            self.output = Some(output);
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let extension_count = merge::parse_list(&env.config.merge.extensions).len();
                stat_row(
                    ui,
                    &[
                        ("输出格式", env.config.merge.format.label().into(), theme::ACCENT),
                        ("扩展名数量", extension_count.to_string(), theme::BLUE),
                        ("体积上限", format!("{} KB", env.config.merge.max_file_kb), theme::SUCCESS),
                        ("结构目录树", if env.config.merge.include_tree { "包含" } else { "不包含" }.into(), theme::TEXT_DIM),
                    ],
                );
                ui.add_space(10.0);
                self.draw_options(ui, env);
                if self.output.is_some() {
                    ui.add_space(8.0);
                    self.draw_output(ui, env);
                } else {
                    ui.add_space(8.0);
                    section_card(ui, "使用提示", None, |ui| {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Layers,
                            "把整个代码目录汇成一份文档",
                            "适合提交给评审、归档或交给 AI 阅读；二进制文件和依赖目录会被自动跳过。",
                        );
                    });
                }
            });
    }
}

fn build_preview(text: &str, max_lines: usize) -> String {
    let mut out = String::new();
    for (index, line) in text.lines().enumerate() {
        if index >= max_lines {
            out.push_str("\n… 预览已截断，保存后可查看完整内容");
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
