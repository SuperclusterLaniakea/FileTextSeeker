use chrono::Local;
use eframe::egui;
use rfd::FileDialog;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 单个文件的条目
#[derive(Clone)]
struct FileEntry {
    path: PathBuf,
    relative_path: String,
    file_name: String,
    selected: bool,
}

/// 应用状态
pub struct CodeMergerApp {
    selected_dir: Option<PathBuf>,
    files: Vec<FileEntry>,
    analysis_done: bool,
    status: String,
    search_text: String,
}

impl Default for CodeMergerApp {
    fn default() -> Self {
        Self {
            selected_dir: None,
            files: Vec::new(),
            analysis_done: false,
            status: "请选择文件夹并点击「分析」".to_string(),
            search_text: String::new(),
        }
    }
}

impl CodeMergerApp {
    /// 分析目录，收集所有支持的代码文件
    fn analyze(&mut self) {
        let dir = match &self.selected_dir {
            Some(d) => d,
            None => {
                self.status = "请先选择文件夹".to_string();
                return;
            }
        };

        self.files.clear();
        // 支持的扩展名（小写）
        let extensions = vec![
            "rs", "c", "cpp", "h", "hpp", "cxx", "cc", "hh",
            "py", "java", "go", "js", "ts", "rb", "php",
            "swift", "kt", "cs", "fs", "lua", "r", "m", "mm",
            "pl", "pm", "sh", "bash", "zsh", "ps1", "bat", "cmd",
            // 配置文件扩展名
            "toml", "env", "ini", "cfg", "conf",
            "yaml", "yml", "json", "xml", "properties",
            "lock", "txt", "md",
            "requirements", "req", "in",
        ];

        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if extensions.contains(&ext_lower.as_str()) {
                    let rel_path = entry.path().strip_prefix(dir).unwrap_or(entry.path());
                    let rel_str = rel_path.to_string_lossy().to_string();
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    self.files.push(FileEntry {
                        path: entry.path().to_path_buf(),
                        relative_path: rel_str,
                        file_name,
                        selected: true, // 默认选中
                    });
                }
            }
        }

        self.files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.analysis_done = true;
        self.status = format!("分析完成，找到 {} 个文件", self.files.len());
    }

    /// 合并选中的文件并保存
    fn merge(&mut self) {
        if !self.analysis_done {
            self.status = "请先执行分析".to_string();
            return;
        }

        let selected: Vec<&FileEntry> = self.files.iter().filter(|f| f.selected).collect();
        if selected.is_empty() {
            self.status = "没有选中任何文件".to_string();
            return;
        }

        // 生成默认文件名：项目文件夹名 + 当前时间
        let dir_name = self
            .selected_dir
            .as_ref()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let default_name = format!("{}_{}.txt", dir_name, timestamp);

        // 弹出保存对话框
        if let Some(save_path) = FileDialog::new()
            .set_file_name(&default_name)
            .save_file()
        {
            match self.write_merged(&save_path, &selected) {
                Ok(_) => {
                    self.status = format!("合并完成，保存至: {}", save_path.display());
                }
                Err(e) => {
                    self.status = format!("写入失败: {}", e);
                }
            }
        } else {
            self.status = "保存已取消".to_string();
        }
    }

    /// 实际写入合并文件
    fn write_merged(&self, path: &Path, selected: &[&FileEntry]) -> std::io::Result<()> {
        let mut file = fs::File::create(path)?;
        for entry in selected {
            writeln!(file, "===== 文件: {} =====", entry.relative_path)?;
            writeln!(file, "路径: {}", entry.relative_path)?;
            writeln!(file, "文件名: {}", entry.file_name)?;
            writeln!(file, "--- 代码 ---")?;
            match fs::read_to_string(&entry.path) {
                Ok(content) => {
                    write!(file, "{}", content)?;
                }
                Err(e) => {
                    writeln!(file, "[读取失败: {}]", e)?;
                }
            }
            writeln!(file, "\n--- 结束 ---\n")?;
        }
        Ok(())
    }

    /// 全选 / 取消全选
    fn select_all(&mut self, select: bool) {
        for f in &mut self.files {
            f.selected = select;
        }
    }

    /// 反选
    fn invert_selection(&mut self) {
        for f in &mut self.files {
            f.selected = !f.selected;
        }
    }

    /// 获取当前选中的文件数量
    fn selected_count(&self) -> usize {
        self.files.iter().filter(|f| f.selected).count()
    }

    /// 渲染界面
    pub fn render(&mut self, ui: &mut egui::Ui) {
        ui.heading("🛠️ 代码合并工具");

        // ---------- 选择文件夹 ----------
        ui.horizontal(|ui| {
            if ui.button("📁 选择文件夹").clicked() {
                if let Some(path) = FileDialog::new().pick_folder() {
                    self.selected_dir = Some(path);
                    self.analysis_done = false;
                    self.files.clear();
                    self.status = format!("已选择: {}", self.selected_dir.as_ref().unwrap().display());
                }
            }
            if let Some(dir) = &self.selected_dir {
                ui.label(dir.display().to_string());
            }
        });

        // ---------- 分析按钮 + 状态 ----------
        ui.horizontal(|ui| {
            if ui.button("🔍 分析").clicked() {
                self.analyze();
            }
            ui.label(&self.status);
        });

        // ---------- 如果分析完成且有文件 ----------
        if self.analysis_done && !self.files.is_empty() {
            ui.separator();

            // 统计 + 全选/取消/反选
            ui.horizontal(|ui| {
                ui.label(format!(
                    "文件总数: {}  已选: {}",
                    self.files.len(),
                    self.selected_count()
                ));
                if ui.button("全选").clicked() {
                    self.select_all(true);
                }
                if ui.button("取消全选").clicked() {
                    self.select_all(false);
                }
                if ui.button("反选").clicked() {
                    self.invert_selection();
                }
            });

            // 搜索框
            ui.horizontal(|ui| {
                ui.label("🔎 筛选:");
                ui.text_edit_singleline(&mut self.search_text);
            });

            // 文件列表（滚动区域）
            ui.separator();
            // 收集所有显示项的索引
            let display_indices: Vec<usize> = self.files
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    self.search_text.is_empty() ||
                    f.relative_path.to_lowercase().contains(&self.search_text.to_lowercase())
                })
                .map(|(i, _)| i)
                .collect();

            ui.label(format!("当前显示: {} 个文件", display_indices.len()));
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for &idx in &display_indices {
                        let entry = &mut self.files[idx];
                        ui.horizontal(|ui| {
                            let mut selected = entry.selected;
                            if ui.checkbox(&mut selected, "").changed() {
                                entry.selected = selected;
                            }
                            ui.label(&entry.relative_path);
                        });
                    }
                });

            // 合并按钮
            ui.separator();
            if ui.button("📄 合并并保存").clicked() {
                self.merge();
            }
        } else if self.analysis_done && self.files.is_empty() {
            ui.label("⚠️ 没有找到任何支持的代码文件。");
        }
    }
}