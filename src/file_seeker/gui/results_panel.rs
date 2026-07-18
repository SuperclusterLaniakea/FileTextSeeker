/// Results panel using egui_extras::TableBuilder for proper column alignment

use eframe::egui::{self, Color32, Frame, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use crate::file_seeker::engine::indexer::format_size;
use crate::file_seeker::types::{SortField, SortOrder};
use crate::file_seeker::gui::app::EverythingApp;

pub fn render_results(ui: &mut egui::Ui, app: &EverythingApp) -> Option<ResultAction> {
    if app.results.is_empty() {
        return render_empty(ui, app);
    }
    render_detail(ui, app)
}

fn render_empty(ui: &mut egui::Ui, app: &EverythingApp) -> Option<ResultAction> {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(RichText::new("搜索").size(40.0));
        ui.add_space(4.0);
        ui.label("按 Enter 或点击搜索按钮查找");
        if app.engine.total_entries() == 0 {
            ui.label(RichText::new("无索引数据，请在选项->高级中添加路径并重建索引").color(Color32::RED));
        } else {
            ui.label(format!("索引: {} 文件 / {} 文件夹", app.engine.total_file_count(), app.engine.total_folder_count()));
        }
    });
    None
}

fn render_detail(ui: &mut egui::Ui, app: &EverythingApp) -> Option<ResultAction> {
    let mut action = None;
    let total = app.results.len();
    let ctx_idx_id = egui::Id::new("ctx_idx");
    let ctx_id = egui::Id::new("ctx_menu");

    let right_clickable_label = |ui: &mut egui::Ui, text: String, row_idx: usize| {
        let label = egui::Label::new(
            RichText::new(text).size(12.0)
        ).sense(Sense::click());
        let resp = ui.add(label);
        if resp.secondary_clicked() {
            let pos = ui.input(|i| i.pointer.interact_pos());
            ui.ctx().memory_mut(|mem| {
                mem.data.insert_temp(ctx_idx_id, row_idx);
                if let Some(p) = pos {
                    mem.data.insert_temp(egui::Id::new("ctx_pos"), p);
                }
                mem.toggle_popup(ctx_id);
            });
        }
        resp
    };

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(180.0).resizable(true).clip(true))
        .column(Column::initial(400.0).resizable(true).clip(true)) // 加宽路径列
        .column(Column::initial(80.0).resizable(true).clip(true))
        .column(Column::initial(60.0).resizable(true).clip(true))
        .column(Column::remainder().resizable(true))
        .header(22.0, |mut header| {
            let headers = [
                ("名称", SortField::Name),
                ("路径", SortField::Path),
                ("大小", SortField::Size),
                ("扩展名", SortField::Extension),
                ("修改日期", SortField::DateModified),
            ];
            for (name, field) in &headers {
                let arrow = if *field == app.sort_field {
                    match app.sort_order {
                        SortOrder::Ascending => " ↑",
                        SortOrder::Descending => " ↓",
                    }
                } else {
                    ""
                };
                let text = format!("{}{}", name, arrow);
                header.col(|ui| {
                    let resp = ui.button(RichText::new(text).strong().size(12.0));
                    if resp.clicked() {
                        action = Some(ResultAction::Sort(*field));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(22.0, total, |mut row| {
                let row_idx = row.index();
                if row_idx >= total { return; }
                let entry = &app.results[row_idx];
                let sel = app.selected_index == Some(row_idx);

                row.set_selected(sel);

                row.col(|ui| {
                    let label = egui::Label::new(
                        RichText::new(&entry.file_name).size(12.0)
                    ).sense(Sense::click());
                    let resp = ui.add(label);
                    if resp.double_clicked() {
                        action = Some(ResultAction::Open(row_idx));
                    }
                    if resp.secondary_clicked() {
                        let pos = ui.input(|i| i.pointer.interact_pos());
                        ui.ctx().memory_mut(|mem| {
                            mem.data.insert_temp(ctx_idx_id, row_idx);
                            if let Some(p) = pos {
                                mem.data.insert_temp(egui::Id::new("ctx_pos"), p);
                            }
                            mem.toggle_popup(ctx_id);
                        });
                    }
                });

                // 路径列：不截断，显示完整路径
                row.col(|ui| {
                    right_clickable_label(ui, get_path_display(entry), row_idx);
                });

                row.col(|ui| {
                    let sz = if entry.is_directory { String::new() } else { format_size(entry.size) };
                    right_clickable_label(ui, sz, row_idx);
                });

                row.col(|ui| {
                    right_clickable_label(ui, entry.extension.clone(), row_idx);
                });

                row.col(|ui| {
                    let dm = entry.date_modified.map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
                    right_clickable_label(ui, dm, row_idx);
                });
            });
        });

    // Context menu with fixed width
    let is_open = ui.ctx().memory(|mem| mem.is_popup_open(ctx_id));
    if is_open {
        let stored_idx = ui.ctx().memory(|mem| mem.data.get_temp::<usize>(ctx_idx_id));
        let stored_pos = ui.ctx().memory(|mem| mem.data.get_temp::<egui::Pos2>(egui::Id::new("ctx_pos")));
        if let Some(idx) = stored_idx {
            let ctx_handle = ui.ctx().clone();
            let fixed_pos = stored_pos.unwrap_or(egui::pos2(100.0, 100.0));
            egui::Area::new(ctx_id)
                .interactable(true)
                .current_pos(fixed_pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    ui.set_width(220.0);
                    Frame::none()
                        .fill(Color32::from_rgb(250,250,255))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(160,160,180)))
                        .show(ui, |ui| {
                            ui.label(RichText::new("文件操作").strong().size(13.0));
                            ui.separator();
                            if ui.button("打开文件").clicked() {
                                action = Some(ResultAction::Open(idx));
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                            if ui.button("打开所在文件夹").clicked() {
                                action = Some(ResultAction::OpenPath(idx));
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                            if ui.button("定位文件").clicked() {
                                if let Some(e) = app.results.get(idx) {
                                    #[cfg(windows)] {
                                        let _ = std::process::Command::new("explorer")
                                            .args(&["/select,", &e.full_path.to_string_lossy().as_ref()])
                                            .spawn();
                                    }
                                }
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                            if ui.button("复制文件名").clicked() {
                                action = Some(ResultAction::CopyName(idx));
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                            if ui.button("复制完整路径").clicked() {
                                action = Some(ResultAction::CopyPath(idx));
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                            if ui.button("复制目录路径").clicked() {
                                if let Some(e) = app.results.get(idx) {
                                    ctx_handle.copy_text(e.parent_path.to_string_lossy().to_string());
                                }
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                            ui.separator();
                            if ui.button("属性").clicked() {
                                action = Some(ResultAction::Properties(idx));
                                ui.ctx().memory_mut(|mem| mem.close_popup());
                            }
                        });
                });
        }
    }
    action
}

/// 截断中文字符串（保留完整字符）
fn trunc_cjk(text: &str, max: usize) -> String {
    let cs: Vec<char> = text.chars().collect();
    if cs.len() <= max {
        text.to_string()
    } else {
        format!("{}...", cs[..max.saturating_sub(3)].iter().collect::<String>())
    }
}

/// 获取显示用的路径（父目录）
fn get_path_display(entry: &crate::file_seeker::types::FileEntry) -> String {
    let p = entry.parent_path.to_string_lossy();
    if p.is_empty() || p == "\\" || p == "/" {
        if let Some(pp) = entry.full_path.parent() {
            let ps = pp.to_string_lossy();
            if ps.is_empty() || ps == "\\" || ps == "/" {
                entry.full_path.to_string_lossy().chars().take(3).collect()
            } else {
                ps.to_string()
            }
        } else {
            String::new()
        }
    } else {
        p.to_string()
    }
}

#[derive(Debug, Clone)]
pub enum ResultAction {
    Open(usize),
    OpenPath(usize),
    Properties(usize),
    Sort(SortField),
    CopyPath(usize),
    CopyName(usize),
}