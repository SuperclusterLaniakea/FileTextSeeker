use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::app::ToastKind;
use crate::config::{AppConfig, ListerFormat};
use crate::lister::{self, ListerOptions, ListerResult};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{job_cancel, job_start, job_take, job_view, pick_folder, progress_row, section_card, stat_row, toggle_chip, Job, TabEnv};

const QUICK_EXTENSIONS: &[&str] = &[
    "docx", "xlsx", "pptx", "pdf", "txt", "md", "jpg", "png", "mp4", "zip", "exe", "dll",
];

pub struct ListerTab {
    ext_text: String,
    job: Job<ListerResult>,
    result: Option<ListerResult>,
    display_limit: usize,
    last_export: Option<PathBuf>,
}

impl ListerTab {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            ext_text: config.lister.selected_extensions.join(","),
            job: super::new_job(),
            result: None,
            display_limit: 300,
            last_export: None,
        }
    }

    fn extensions(&self) -> Vec<String> {
        lister::parse_extensions(&self.ext_text)
    }

    fn sync_config(&mut self, env: &mut TabEnv) {
        let parsed = self.extensions();
        if env.config.lister.selected_extensions != parsed {
            env.config.lister.selected_extensions = parsed;
            env.mark_dirty();
        }
    }

    fn start(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        self.sync_config(env);
        let source = env.config.lister.source.trim().to_string();
        if source.is_empty() {
            env.toast("请选择要盘点的目录", ToastKind::Warn);
            return;
        }
        if !std::path::Path::new(&source).is_dir() {
            env.toast("目录不存在，请重新选择", ToastKind::Warn);
            return;
        }
        let opts = ListerOptions {
            source: PathBuf::from(source),
            recursive: env.config.lister.recursive,
            extensions: self.extensions(),
            filter: env.config.lister.filter_text.trim().to_string(),
        };
        self.last_export = None;
        job_start(&self.job, ctx, move |cancel, sink| {
            sink.set("扫描目录", 0, 0, opts.source.display().to_string());
            let mut result = lister::scan(&opts, cancel, &mut |progress| {
                sink.set("盘点文件", progress.done, 0, progress.message);
            });
            result.errors.truncate(200);
            Ok(result)
        });
    }

    fn export(&mut self, env: &mut TabEnv) {
        let Some(result) = &self.result else {
            return;
        };
        if result.rows.is_empty() {
            env.toast("没有可导出的记录", ToastKind::Warn);
            return;
        }
        let format = env.config.lister.format;
        let (default_name, filter_name, extensions) = match format {
            ListerFormat::Xlsx => ("文件清单.xlsx", "Excel 工作簿", vec!["xlsx"]),
            ListerFormat::Csv => ("文件清单.csv", "CSV 文件", vec!["csv"]),
        };
        let Some(path) = super::save_file(default_name, filter_name, &extensions) else {
            return;
        };
        let outcome = match format {
            ListerFormat::Xlsx => lister::export_xlsx(&path, &result.rows),
            ListerFormat::Csv => lister::export_csv(&path, &result.rows),
        };
        match outcome {
            Ok(()) => {
                env.toast(format!("文件清单已导出到 {}", path.display()), ToastKind::Success);
                env.set_status(format!("文件清单 · 已导出 {}", path.display()));
                self.last_export = Some(path);
            }
            Err(error) => env.toast(format!("导出失败: {error}"), ToastKind::Error),
        }
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        section_card(ui, "盘点目录", Some("递归深度、扩展名与文件名过滤可自由组合"), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("目录").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 100.0).max(200.0);
                if ui
                    .add_sized(
                        [width, 28.0],
                        egui::TextEdit::singleline(&mut env.config.lister.source)
                            .hint_text("选择需要生成清单的文件夹")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "浏览").clicked() {
                    if let Some(path) = pick_folder() {
                        env.config.lister.source = path.display().to_string();
                        env.mark_dirty();
                    }
                }
            });
            ui.horizontal(|ui| {
                if widgets::toggle_switch(ui, &mut env.config.lister.recursive).changed() {
                    env.mark_dirty();
                }
                super::small_muted(ui, "包含子目录");
                ui.add_space(10.0);
                ui.label(RichText::new("文件名包含").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 140.0).max(120.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut env.config.lister.filter_text)
                            .hint_text("模糊匹配文件名，留空表示全部")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("扩展名").size(11.5).color(theme::dim(ui)));
                let width = (ui.available_width() - 90.0).max(160.0);
                if ui
                    .add_sized(
                        [width, 26.0],
                        egui::TextEdit::singleline(&mut self.ext_text)
                            .hint_text("留空表示全部，例如 docx,xlsx,pdf")
                            .margin(egui::Vec2::new(9.0, 5.0)),
                    )
                    .changed()
                {
                    env.mark_dirty();
                }
            });
            ui.horizontal_wrapped(|ui| {
                super::small_muted(ui, "快捷选择");
                for ext in QUICK_EXTENSIONS {
                    let active = self.extensions().iter().any(|e| e == ext);
                    if super::toggle_chip(ui, active, ext) {
                        let mut list = self.extensions();
                        if active {
                            list.retain(|e| e != ext);
                        } else {
                            list.push((*ext).to_string());
                        }
                        self.ext_text = list.join(",");
                        env.mark_dirty();
                    }
                }
                if ui.small_button("全部清空").clicked() {
                    self.ext_text.clear();
                    env.mark_dirty();
                }
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("导出格式").size(11.5).color(theme::dim(ui)));
                for format in [ListerFormat::Xlsx, ListerFormat::Csv] {
                    if toggle_chip(ui, env.config.lister.format == format, format.label()) {
                        env.config.lister.format = format;
                        env.mark_dirty();
                    }
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let (running, _, _, _) = job_view(&self.job);
            if running {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消").clicked() {
                    job_cancel(&self.job);
                }
            } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Grid), "开始盘点").clicked() {
                self.start(ui.ctx(), env);
            }
            if self.result.is_some() {
                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Export), "导出清单").clicked() {
                    self.export(env);
                }
                if let Some(path) = &self.last_export {
                    widgets::pill(ui, format!("已导出 {}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), theme::SUCCESS);
                }
            }
        });
        progress_row(ui, &self.job);
    }

    fn draw_result(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        let Some(result) = &self.result else {
            return;
        };
        stat_row(
            ui,
            &[
                ("文件数量", result.rows.len().to_string(), theme::ACCENT),
                ("总体积", util::format_size(result.total_bytes), theme::BLUE),
                ("读取错误", result.errors.len().to_string(), if result.errors.is_empty() { theme::TEXT_DIM } else { theme::WARN }),
                ("导出格式", env.config.lister.format.label().into(), theme::SUCCESS),
            ],
        );
        if result.truncated {
            ui.label(RichText::new("记录超过 200,000 行，已截断；建议缩小目录范围。").size(11.0).color(theme::WARN));
        }
        ui.add_space(8.0);
        let mut export_clicked = false;
        section_card(ui, "文件清单", Some("表格仅预览部分记录，导出时包含全部"), |ui| {
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, Some(widgets::ToolIcon::Export), "导出清单").clicked() {
                    export_clicked = true;
                }
                if !result.errors.is_empty() {
                    widgets::pill(ui, format!("读取错误 {}", result.errors.len()), theme::WARN);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if result.rows.len() > self.display_limit {
                        if ui.small_button("显示更多").clicked() {
                            self.display_limit = (self.display_limit + 500).min(result.rows.len());
                        }
                    }
                });
            });
            ui.add_space(4.0);
            let limit = result.rows.len().min(self.display_limit);
            TableBuilder::new(ui)
                .striped(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(180.0).at_least(110.0).clip(true))
                .column(Column::initial(64.0).clip(true))
                .column(Column::initial(80.0).at_least(60.0).clip(true))
                .column(Column::initial(130.0).at_least(90.0).clip(true))
                .column(Column::remainder().at_least(150.0).clip(true))
                .max_scroll_height(400.0)
                .header(24.0, |mut header| {
                    for title in ["名称", "类型", "大小", "修改时间", "路径"] {
                        header.col(|ui| {
                            ui.label(RichText::new(title).size(11.0).strong().color(theme::dim(ui)));
                        });
                    }
                })
                .body(|body| {
                    body.rows(24.0, limit, |mut row| {
                        let file = &result.rows[row.index()];
                        row.col(|ui| {
                            let name = ui.add(
                                egui::Label::new(RichText::new(&file.name).size(11.0))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text(&file.name);
                            super::path_actions(name, &file.path);
                        });
                        row.col(|ui| {
                            let ext = if file.extension.is_empty() { "—" } else { file.extension.as_str() };
                            ui.label(RichText::new(ext).size(10.5).color(theme::dim(ui)));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(util::format_size(file.size)).size(10.5).color(theme::dim(ui)));
                        });
                        row.col(|ui| {
                            let modified = file.modified.map(util::format_time).unwrap_or_else(|| "—".into());
                            ui.label(RichText::new(modified).size(10.5).color(theme::dim(ui)));
                        });
                        row.col(|ui| {
                            let parent = file.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                            ui.label(RichText::new(super::truncate_path(&parent, 70)).size(10.5).color(theme::dim(ui)))
                                .on_hover_text(file.path.display().to_string());
                        });
                        super::path_actions(row.response(), &file.path);
                    });
                });
            if limit < result.rows.len() {
                super::small_muted(ui, format!("已显示 {limit} / {} 条，可继续加载或直接导出。", result.rows.len()));
            }
        });
        if export_clicked {
            self.export(env);
        }
        ui.add_space(6.0);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        self.sync_config(env);
        if let Some(result) = job_take(&self.job) {
            let count = result.rows.len();
            env.toast(
                format!("盘点完成 · {count} 个文件 · {}", util::format_size(result.total_bytes)),
                ToastKind::Success,
            );
            env.set_status(format!("文件清单 · {count} 个文件，总计 {}", util::format_size(result.total_bytes)));
            self.result = Some(result);
            self.display_limit = 300;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let file_count = self.result.as_ref().map(|r| r.rows.len()).unwrap_or(0);
                stat_row(
                    ui,
                    &[
                        ("盘点状态", if self.result.is_some() { "已完成" } else { "待盘点" }.into(), if self.result.is_some() { theme::SUCCESS } else { theme::TEXT_DIM }),
                        ("文件数量", if self.result.is_some() { file_count.to_string() } else { "—".into() }, theme::ACCENT),
                        ("递归", if env.config.lister.recursive { "开启" } else { "关闭" }.into(), theme::BLUE),
                        ("导出格式", env.config.lister.format.label().into(), theme::SUCCESS),
                    ],
                );
                ui.add_space(10.0);
                self.draw_options(ui, env);
                if self.result.is_some() {
                    ui.add_space(8.0);
                    self.draw_result(ui, env);
                } else {
                    ui.add_space(8.0);
                    section_card(ui, "使用提示", None, |ui| {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Grid,
                            "目录盘点与清单导出",
                            "扫描指定目录并导出 Excel / CSV，包含名称、类型、大小、三个时间戳与完整路径。",
                        );
                    });
                }
            });
    }
}
