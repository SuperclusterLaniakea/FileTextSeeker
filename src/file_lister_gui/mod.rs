use eframe::egui;
use std::collections::HashSet;
use chrono::{DateTime, Local};
use std::time::SystemTime;
use rfd::FileDialog;

#[derive(PartialEq, Clone, Copy)]
enum SaveFormat {
    Csv,
    Xlsx,
}

impl SaveFormat {
    fn as_str(&self) -> &str {
        match self {
            SaveFormat::Csv => "CSV",
            SaveFormat::Xlsx => "XLSX",
        }
    }
}

struct FileInfo {
    index: usize,
    name: String,
    extension: String,
    path: String,
    created: String,
    modified: String,
    size: u64,
}

pub struct FileListerApp {
    source_dir: String,
    recursive: bool,
    all_extensions: Vec<String>,
    selected_extensions: HashSet<String>,
    files: Vec<FileInfo>,
    save_dir: String,
    save_format: SaveFormat,
    log: Vec<String>,
    total_files: usize,
    total_size: u64,
    picking_source: bool,
    picking_save: bool,
    scanned_extensions: bool,
}

impl Default for FileListerApp {
    fn default() -> Self {
        Self {
            source_dir: String::new(),
            recursive: false,
            all_extensions: Vec::new(),
            selected_extensions: HashSet::new(),
            files: Vec::new(),
            save_dir: String::new(),
            save_format: SaveFormat::Xlsx,
            log: vec![String::from("就绪，请选择源文件夹。")],
            total_files: 0,
            total_size: 0,
            picking_source: false,
            picking_save: false,
            scanned_extensions: false,
        }
    }
}

impl FileListerApp {
    fn scan_extensions(&mut self) {
        self.log.push("开始扫描扩展名...".to_string());
        let mut ext_set = HashSet::new();
        let source = std::path::Path::new(&self.source_dir);
        if !source.is_dir() {
            self.log.push("错误：源文件夹无效".to_string());
            return;
        }
        let walker = if self.recursive {
            walkdir::WalkDir::new(source).follow_links(false).into_iter()
        } else {
            walkdir::WalkDir::new(source).max_depth(1).follow_links(false).into_iter()
        };
        for entry in walker.filter_entry(|_| true) {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_file() {
                        let path = entry.path();
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_else(|| "(无扩展名)".to_string());
                        ext_set.insert(ext);
                    }
                }
                Err(e) => {
                    self.log.push(format!("访问错误: {}", e));
                }
            }
        }
        self.all_extensions = {
            let mut v: Vec<String> = ext_set.into_iter().collect();
            v.sort();
            v
        };
        self.selected_extensions = self.all_extensions.iter().cloned().collect();
        self.scanned_extensions = true;
        self.log.push(format!("扫描完成，找到 {} 种扩展名", self.all_extensions.len()));
        if self.all_extensions.is_empty() {
            self.log.push("未找到任何文件".to_string());
        }
    }

    fn generate_and_save(&mut self) {
        if self.source_dir.is_empty() || self.save_dir.is_empty() {
            self.log.push("错误：请先选择源文件夹和保存位置".to_string());
            return;
        }
        if self.selected_extensions.is_empty() {
            self.log.push("错误：未选择任何扩展名".to_string());
            return;
        }
        self.log.push("开始生成文件清单...".to_string());
        let mut files = Vec::new();
        let mut total_size: u64 = 0;
        let source = std::path::Path::new(&self.source_dir);
        let walker = if self.recursive {
            walkdir::WalkDir::new(source).follow_links(false).into_iter()
        } else {
            walkdir::WalkDir::new(source).max_depth(1).follow_links(false).into_iter()
        };
        let mut index = 0;
        for entry in walker.filter_entry(|_| true) {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_file() {
                        let path = entry.path();
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_else(|| "(无扩展名)".to_string());
                        if !self.selected_extensions.contains(&ext) {
                            continue;
                        }
                        if let Ok(metadata) = path.metadata() {
                            let size = metadata.len();
                            let created = metadata.created().ok();
                            let modified = metadata.modified().ok();
                            let format_time = |st: Option<SystemTime>| -> String {
                                st.map(|t| {
                                    let dt: DateTime<Local> =
                                        DateTime::<chrono::Utc>::from(t).with_timezone(&Local);
                                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                                })
                                .unwrap_or_else(|| String::new())
                            };
                            index += 1;
                            let file_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let file_path = path.display().to_string();
                            files.push(FileInfo {
                                index,
                                name: file_name,
                                extension: ext,
                                path: file_path,
                                created: format_time(created),
                                modified: format_time(modified),
                                size,
                            });
                            total_size += size;
                        } else {
                            self.log
                                .push(format!("无法读取元数据: {}", path.display()));
                        }
                    }
                }
                Err(e) => {
                    self.log.push(format!("访问错误: {}", e));
                }
            }
        }
        self.files = files;
        self.total_files = self.files.len();
        self.total_size = total_size;
        self.log.push(format!(
            "找到 {} 个符合条件的文件，总大小 {} 字节",
            self.total_files, self.total_size
        ));

        let dir_name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let now = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let file_stem = format!("{}_{}", dir_name, now);
        let ext = match self.save_format {
            SaveFormat::Csv => "csv",
            SaveFormat::Xlsx => "xlsx",
        };
        let file_name = format!("{}.{}", file_stem, ext);
        let filepath = std::path::Path::new(&self.save_dir).join(&file_name);

        match self.save_format {
            SaveFormat::Csv => match self.save_csv(&filepath) {
                Ok(_) => self.log.push(format!("成功保存CSV文件: {}", filepath.display())),
                Err(e) => self.log.push(format!("保存CSV失败: {}", e)),
            },
            SaveFormat::Xlsx => match self.save_xlsx(&filepath) {
                Ok(_) => self.log.push(format!("成功保存XLSX文件: {}", filepath.display())),
                Err(e) => self.log.push(format!("保存XLSX失败: {}", e)),
            },
        }
    }

    fn save_csv(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record(&[
            "序号", "文件名", "扩展名", "文件地址", "创建时间", "修改时间", "文件大小",
        ])?;
        for f in &self.files {
            wtr.write_record(&[
                f.index.to_string(),
                f.name.clone(),
                f.extension.clone(),
                f.path.clone(),
                f.created.clone(),
                f.modified.clone(),
                f.size.to_string(),
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }

    fn save_xlsx(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        use rust_xlsxwriter::*;
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.write_string(0, 0, "序号")?;
        worksheet.write_string(0, 1, "文件名")?;
        worksheet.write_string(0, 2, "扩展名")?;
        worksheet.write_string(0, 3, "文件地址")?;
        worksheet.write_string(0, 4, "创建时间")?;
        worksheet.write_string(0, 5, "修改时间")?;
        worksheet.write_string(0, 6, "文件大小")?;
        for (i, f) in self.files.iter().enumerate() {
            let row = (i + 1) as u32;
            worksheet.write_string(row, 0, &f.index.to_string())?;
            worksheet.write_string(row, 1, &f.name)?;
            worksheet.write_string(row, 2, &f.extension)?;
            worksheet.write_string(row, 3, &f.path)?;
            worksheet.write_string(row, 4, &f.created)?;
            worksheet.write_string(row, 5, &f.modified)?;
            worksheet.write_number(row, 6, f.size as f64)?;
        }
        workbook.save(path)?;
        Ok(())
    }

    pub fn render(&mut self, ui: &mut egui::Ui) {
        if self.picking_source {
            if let Some(path) = FileDialog::new().pick_folder() {
                self.source_dir = path.display().to_string();
                self.log.push(format!("已选择源文件夹: {}", self.source_dir));
                self.scanned_extensions = false;
                self.all_extensions.clear();
                self.selected_extensions.clear();
            }
            self.picking_source = false;
        }
        if self.picking_save {
            if let Some(path) = FileDialog::new().pick_folder() {
                self.save_dir = path.display().to_string();
                self.log.push(format!("已选择保存位置: {}", self.save_dir));
            }
            self.picking_save = false;
        }

        ui.horizontal(|ui| {
            if ui.button("选择源文件夹").clicked() {
                self.picking_source = true;
            }
            ui.label("源文件夹:");
            ui.label(&self.source_dir);
        });
        ui.checkbox(&mut self.recursive, "遍历子文件夹");

        if ui.button("扫描文件扩展名").clicked() {
            self.scan_extensions();
        }

        if self.scanned_extensions && !self.all_extensions.is_empty() {
            ui.separator();
            ui.label("选择要包含的扩展名：");
            ui.horizontal(|ui| {
                if ui.button("全选").clicked() {
                    self.selected_extensions = self.all_extensions.iter().cloned().collect();
                }
                if ui.button("取消全选").clicked() {
                    self.selected_extensions.clear();
                }
            });
            // 使用 horizontal_wrapped 让复选框自动换行
            ui.horizontal_wrapped(|ui| {
                let mut to_remove = Vec::new();
                for ext in &self.all_extensions {
                    let mut selected = self.selected_extensions.contains(ext);
                    ui.checkbox(&mut selected, ext);
                    if selected {
                        self.selected_extensions.insert(ext.clone());
                    } else {
                        to_remove.push(ext.clone());
                    }
                }
                for ext in to_remove {
                    self.selected_extensions.remove(&ext);
                }
            });
        }

        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("选择保存位置").clicked() {
                self.picking_save = true;
            }
            ui.label("保存位置:");
            ui.label(&self.save_dir);
        });

        ui.horizontal(|ui| {
            ui.label("保存格式:");
            egui::ComboBox::from_id_source("fl_format_combo")
                .selected_text(self.save_format.as_str())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.save_format, SaveFormat::Xlsx, "XLSX");
                    ui.selectable_value(&mut self.save_format, SaveFormat::Csv, "CSV");
                });
        });

        if ui.button("生成文件清单").clicked() {
            self.generate_and_save();
        }

        ui.separator();

        ui.label(format!(
            "文件总数: {}    总大小: {} 字节 (约 {:.2} MB)",
            self.total_files,
            self.total_size,
            self.total_size as f64 / 1_048_576.0
        ));

        ui.label("日志:");
        // 日志区域：设置文本自动换行
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(200.0)
            .show(ui, |ui| {
                ui.style_mut().wrap = Some(true);
                for msg in self.log.iter().rev() {
                    ui.label(msg);
                }
            });
    }
}