use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::config::{AppConfig, DupMethod, HashChoice, ImageHashKind};
use crate::duplicates::{delete_paths, export_groups_csv, save_scan, scan, DupGroup, FileEntry, ScanMode, ScanOptions, ScanOutcome, ScanResult};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{add_folders_button, confirm_dialog, job_start, job_take, job_view, path_chips, progress_row, section_card, stat_row, toggle_chip, Job, TabEnv};

const MODES: [ScanMode; 9] = [
    ScanMode::Duplicates,
    ScanMode::SimilarImages,
    ScanMode::EmptyFiles,
    ScanMode::EmptyFolders,
    ScanMode::BigFiles,
    ScanMode::TempFiles,
    ScanMode::BrokenFiles,
    ScanMode::BadExtensions,
    ScanMode::InvalidSymlinks,
];

fn mode_id(mode: ScanMode) -> &'static str {
    match mode {
        ScanMode::Duplicates => "duplicates",
        ScanMode::SimilarImages => "similar_images",
        ScanMode::EmptyFiles => "empty_files",
        ScanMode::EmptyFolders => "empty_folders",
        ScanMode::BigFiles => "big_files",
        ScanMode::TempFiles => "temp_files",
        ScanMode::BrokenFiles => "broken_files",
        ScanMode::BadExtensions => "bad_extensions",
        ScanMode::InvalidSymlinks => "invalid_symlinks",
    }
}

fn mode_from_id(id: &str) -> ScanMode {
    MODES.iter().copied().find(|m| mode_id(*m) == id).unwrap_or(ScanMode::Duplicates)
}

pub struct DuplicatesTab {
    mode: ScanMode,
    roots: Vec<String>,
    exclusions_text: String,
    extensions_text: String,
    include_hidden: bool,
    include_system: bool,
    min_size_mb: u64,
    method: DupMethod,
    hash: HashChoice,
    image_threshold: u32,
    image_hash_size: u32,
    image_kind: ImageHashKind,
    ignore_same_resolution: bool,
    top_n: usize,
    big_min_mb: u64,
    temp_suffixes_text: String,
    move_to_trash: bool,
    job: Job<ScanResult>,
    delete_job: Job<(crate::duplicates::DeleteReport, Vec<PathBuf>, u64)>,
    result: Option<ScanResult>,
    selected: HashSet<PathBuf>,
    confirm_delete: bool,
    show_advanced: bool,
    scan_roots: Vec<PathBuf>,
    show_all_groups: bool,
}

impl DuplicatesTab {
    pub fn new(config: &AppConfig) -> Self {
        let cfg = &config.duplicates;
        Self {
            mode: mode_from_id(&cfg.mode),
            roots: cfg.roots.clone(),
            exclusions_text: cfg.exclusions.join(", "),
            extensions_text: cfg.extensions.clone(),
            include_hidden: cfg.include_hidden,
            include_system: cfg.include_system,
            min_size_mb: cfg.min_size_bytes / 1024 / 1024,
            method: cfg.method,
            hash: cfg.hash,
            image_threshold: cfg.image_threshold,
            image_hash_size: cfg.image_hash_size,
            image_kind: cfg.image_kind,
            ignore_same_resolution: cfg.ignore_same_resolution,
            top_n: cfg.top_n,
            big_min_mb: cfg.big_min_mb,
            temp_suffixes_text: cfg.temp_suffixes.clone(),
            move_to_trash: cfg.move_to_trash,
            job: super::new_job(),
            delete_job: super::new_job(),
            result: None,
            selected: HashSet::new(),
            confirm_delete: false,
            show_advanced: false,
            scan_roots: Vec::new(),
            show_all_groups: false,
        }
    }

    fn sync_config(&mut self, env: &mut TabEnv) {
        let cfg = &mut env.config.duplicates;
        cfg.mode = mode_id(self.mode).to_string();
        cfg.roots = self.roots.clone();
        cfg.exclusions = split_list(&self.exclusions_text);
        cfg.extensions = self.extensions_text.clone();
        cfg.include_hidden = self.include_hidden;
        cfg.include_system = self.include_system;
        cfg.min_size_bytes = self.min_size_mb * 1024 * 1024;
        cfg.method = self.method;
        cfg.hash = self.hash;
        cfg.image_threshold = self.image_threshold;
        cfg.image_hash_size = self.image_hash_size;
        cfg.image_kind = self.image_kind;
        cfg.ignore_same_resolution = self.ignore_same_resolution;
        cfg.top_n = self.top_n;
        cfg.big_min_mb = self.big_min_mb;
        cfg.temp_suffixes = self.temp_suffixes_text.clone();
        cfg.move_to_trash = self.move_to_trash;
    }

    fn start_scan(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        self.sync_config(env);
        env.mark_dirty();
        let roots: Vec<PathBuf> = self.roots.iter().map(PathBuf::from).collect();
        if roots.is_empty() {
            env.toast("请先添加要扫描的目录", crate::app::ToastKind::Warn);
            return;
        }
        let options = ScanOptions {
            roots: roots.clone(),
            exclusions: split_list(&self.exclusions_text),
            extensions: split_list(&self.extensions_text),
            include_hidden: self.include_hidden,
            include_system: self.include_system,
            min_size: self.min_size_mb * 1024 * 1024,
            method: self.method,
            hash: self.hash,
            image_threshold: self.image_threshold,
            image_hash_size: self.image_hash_size,
            image_kind: self.image_kind,
            ignore_same_resolution: self.ignore_same_resolution,
            big_min_bytes: self.big_min_mb * 1024 * 1024,
            temp_suffixes: split_list(&self.temp_suffixes_text),
            top_n: self.top_n.max(1),
            mode: self.mode,
        };
        self.scan_roots = roots;
        self.selected.clear();
        self.result = None;
        job_start(&self.job, ctx, move |cancel, sink| {
            sink.set("扫描目录", 0, 0, "");
            let result = scan(&options, cancel, &mut |progress| {
                sink.set(progress.stage, progress.done, progress.total, progress.message);
            });
            Ok(result)
        });
    }

    fn start_delete(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
        if paths.is_empty() {
            env.toast("没有选中要删除的项目", crate::app::ToastKind::Warn);
            return;
        }
        let trash = self.move_to_trash;
        let freed = self.selected_bytes();
        job_start(&self.delete_job, ctx, move |_cancel, sink| {
            sink.set(if trash { "移入回收站" } else { "永久删除" }, 0, paths.len() as u64, "");
            let report = delete_paths(&paths, trash);
            Ok((report, paths, freed))
        });
    }

    fn selected_bytes(&self) -> u64 {
        let Some(result) = &self.result else { return 0 };
        match &result.outcome {
            ScanOutcome::Groups(groups) => groups
                .iter()
                .flat_map(|g| g.files.iter())
                .filter(|f| self.selected.contains(&f.path))
                .map(|f| f.size)
                .sum(),
            ScanOutcome::Files(files) => files.iter().filter(|f| self.selected.contains(&f.path)).map(|f| f.size).sum(),
            ScanOutcome::Dirs(_) => 0,
        }
    }

    fn apply_deleted(&mut self, paths: &[PathBuf]) {
        let Some(result) = &mut self.result else { return };
        let set: HashSet<&Path> = paths.iter().map(|p| p.as_path()).collect();
        match &mut result.outcome {
            ScanOutcome::Groups(groups) => {
                for group in groups.iter_mut() {
                    group.files.retain(|f| !set.contains(f.path.as_path()));
                }
                groups.retain(|g| g.files.len() > 1);
            }
            ScanOutcome::Files(files) => files.retain(|f| !set.contains(f.path.as_path())),
            ScanOutcome::Dirs(dirs) => dirs.retain(|d| !set.contains(d.as_path())),
        }
        self.selected.clear();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        self.sync_config(env);
        if let Some(result) = job_take(&self.job) {
            let scanned = result.scanned;
            let cancelled = result.cancelled;
            self.result = Some(result);
            self.selected.clear();
            let msg = if cancelled { "扫描已取消，展示部分结果" } else { "扫描完成" };
            env.toast(format!("{msg} · 已分析 {scanned} 项"), crate::app::ToastKind::Info);
            env.set_status(format!("重复查找 · {msg} · {scanned} 项"));
        }
        if let Some((report, paths, freed)) = job_take(&self.delete_job) {
            self.apply_deleted(&paths);
            let mut msg = format!("已删除 {} 项，释放 {}", report.deleted, util::format_size(freed));
            env.set_status(msg.clone());
            if report.failed > 0 || report.blocked > 0 {
                msg.push_str(&format!("，{} 项失败 / {} 项受保护", report.failed, report.blocked));
                env.toast(msg, crate::app::ToastKind::Warn);
            } else {
                env.toast(format!("清理完成 · 释放 {}", util::format_size(freed)), crate::app::ToastKind::Success);
            }
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let (scanned, group_count, wasted, files_count) = match &self.result {
                    Some(result) => match &result.outcome {
                        ScanOutcome::Groups(groups) => (
                            result.scanned,
                            groups.len(),
                            groups.iter().map(|g| g.wasted).sum::<u64>(),
                            groups.iter().map(|g| g.files.len()).sum::<usize>(),
                        ),
                        ScanOutcome::Files(files) => (result.scanned, 0, files.iter().map(|f| f.size).sum(), files.len()),
                        ScanOutcome::Dirs(dirs) => (result.scanned, dirs.len(), 0, dirs.len()),
                    },
                    None => (0, 0, 0, 0),
                };
                let selected_count = self.selected.len();
                stat_row(
                    ui,
                    &[
                        ("已分析项目", scanned.to_string(), theme::ACCENT),
                        (if matches!(self.mode, ScanMode::Duplicates | ScanMode::SimilarImages) { "重复组" } else { "结果数" }, group_count.max(files_count).to_string(), theme::BLUE),
                        ("可释放空间", util::format_size(wasted), theme::SUCCESS),
                        ("已选中", format!("{} · {}", selected_count, util::format_size(self.selected_bytes())), theme::WARN),
                    ],
                );
                ui.add_space(10.0);

                section_card(ui, "查找模式", Some("九种扫描方式，覆盖重复、图片相似与空间占用分析"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for mode in MODES {
                            if toggle_chip(ui, self.mode == mode, mode.label()) {
                                self.mode = mode;
                                self.result = None;
                                self.selected.clear();
                                env.mark_dirty();
                            }
                        }
                    });
                    ui.add_space(6.0);
                    path_chips(ui, &mut self.roots, "尚未添加扫描目录");
                    ui.horizontal(|ui| {
                        if add_folders_button(ui, &mut self.roots, "添加目录") {
                            env.mark_dirty();
                        }
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空").clicked() {
                            self.roots.clear();
                            env.mark_dirty();
                        }
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::ChevronDown), "高级设置").clicked() {
                            self.show_advanced = !self.show_advanced;
                        }
                    });
                    if self.show_advanced {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            super::small_muted(ui, "排除目录");
                            let width = (ui.available_width() - 80.0).max(200.0);
                            if ui.add_sized([width, 26.0], egui::TextEdit::singleline(&mut self.exclusions_text).hint_text("node_modules, .git")).changed() {
                                env.mark_dirty();
                            }
                        });
                        ui.horizontal(|ui| {
                            super::small_muted(ui, "仅含扩展名");
                            let width = (ui.available_width() - 90.0).max(200.0);
                            if ui.add_sized([width, 26.0], egui::TextEdit::singleline(&mut self.extensions_text).hint_text("留空表示全部，例如 jpg,png,mp4")).changed() {
                                env.mark_dirty();
                            }
                        });
                        ui.horizontal(|ui| {
                            if widgets::toggle_switch(ui, &mut self.include_hidden).changed() {
                                env.mark_dirty();
                            }
                            super::small_muted(ui, "包含隐藏文件");
                            if widgets::toggle_switch(ui, &mut self.include_system).changed() {
                                env.mark_dirty();
                            }
                            super::small_muted(ui, "包含系统目录");
                            super::small_muted(ui, "|");
                            super::small_muted(ui, "最大分组数");
                            ui.add(egui::DragValue::new(&mut self.top_n).range(10..=5000).speed(10));
                        });
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        match self.mode {
                            ScanMode::Duplicates => {
                                super::small_muted(ui, "比较方式");
                                egui::ComboBox::from_id_salt("dup_method")
                                    .selected_text(match self.method {
                                        DupMethod::Hash => "内容哈希",
                                        DupMethod::Name => "同名",
                                        DupMethod::Size => "同大小",
                                        DupMethod::SizeName => "同名且同大小",
                                    })
                                    .width(130.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.method, DupMethod::Hash, "内容哈希");
                                        ui.selectable_value(&mut self.method, DupMethod::Name, "同名");
                                        ui.selectable_value(&mut self.method, DupMethod::Size, "同大小");
                                        ui.selectable_value(&mut self.method, DupMethod::SizeName, "同名且同大小");
                                    });
                                if self.method == DupMethod::Hash {
                                    super::small_muted(ui, "哈希");
                                    egui::ComboBox::from_id_salt("dup_hash")
                                        .selected_text(match self.hash {
                                            HashChoice::Blake3 => "BLAKE3",
                                            HashChoice::Xxh3 => "XXH3",
                                            HashChoice::Crc32 => "CRC32",
                                        })
                                        .width(90.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.hash, HashChoice::Blake3, "BLAKE3");
                                            ui.selectable_value(&mut self.hash, HashChoice::Xxh3, "XXH3");
                                            ui.selectable_value(&mut self.hash, HashChoice::Crc32, "CRC32");
                                        });
                                }
                                super::small_muted(ui, "最小体积 MB");
                                ui.add(egui::DragValue::new(&mut self.min_size_mb).range(0..=1_000_000));
                            }
                            ScanMode::SimilarImages => {
                                super::small_muted(ui, "相似阈值");
                                ui.add(egui::Slider::new(&mut self.image_threshold, 0..=32).show_value(true));
                                super::small_muted(ui, "指纹");
                                egui::ComboBox::from_id_salt("image_kind")
                                    .selected_text(match self.image_kind {
                                        ImageHashKind::Difference => "dHash",
                                        ImageHashKind::Average => "aHash",
                                        ImageHashKind::Perceptual => "pHash",
                                    })
                                    .width(80.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.image_kind, ImageHashKind::Difference, "dHash");
                                        ui.selectable_value(&mut self.image_kind, ImageHashKind::Average, "aHash");
                                        ui.selectable_value(&mut self.image_kind, ImageHashKind::Perceptual, "pHash");
                                    });
                                super::small_muted(ui, "尺寸");
                                ui.add(egui::DragValue::new(&mut self.image_hash_size).range(8..=64));
                                if widgets::toggle_switch(ui, &mut self.ignore_same_resolution).changed() {
                                    env.mark_dirty();
                                }
                                super::small_muted(ui, "忽略同尺寸");
                            }
                            ScanMode::BigFiles => {
                                super::small_muted(ui, "最小体积 MB");
                                ui.add(egui::DragValue::new(&mut self.big_min_mb).range(1..=1_000_000));
                            }
                            ScanMode::TempFiles => {
                                super::small_muted(ui, "临时后缀");
                                let width = (ui.available_width() - 90.0).max(200.0);
                                if ui.add_sized([width, 26.0], egui::TextEdit::singleline(&mut self.temp_suffixes_text).hint_text(".tmp,.bak,~")).changed() {
                                    env.mark_dirty();
                                }
                            }
                            _ => {
                                super::small_muted(ui, "该模式会扫描整个目录并列出符合条件的项目。");
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let running = job_view(&self.job).0;
                        if running {
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消扫描").clicked() {
                                super::job_cancel(&self.job);
                            }
                        } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Search), "开始扫描").clicked() {
                            self.start_scan(ui.ctx(), env);
                        }
                        if widgets::toggle_switch(ui, &mut self.move_to_trash).changed() {
                            env.mark_dirty();
                        }
                        super::small_muted(ui, "删除时移入回收站");
                    });
                    progress_row(ui, &self.job);
                });
                ui.add_space(8.0);

                if let Some(result) = &self.result {
                    let errors = result.errors.clone();
                    section_card(ui, "扫描结果", Some("勾选需要清理的项目；重复组建议至少保留一份"), |ui| {
                        if !errors.is_empty() {
                            let first = &errors[0];
                            ui.label(RichText::new(format!("{} 项读取失败：{}", errors.len(), first)).size(11.0).color(theme::WARN));
                        }
                        match &result.outcome {
                            ScanOutcome::Groups(groups) => {
                                if groups.is_empty() {
                                    widgets::empty_state(ui, widgets::ToolIcon::Copies, "没有发现重复项目", "尝试调整最小体积、比较方式或扫描目录。");
                                    return;
                                }
                                ui.horizontal(|ui| {
                                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Check), "每组保留第一个").clicked() {
                                        let mut add: Vec<PathBuf> = Vec::new();
                                        for group in groups {
                                            add.extend(group.files.iter().skip(1).map(|f| f.path.clone()));
                                        }
                                        for path in add {
                                            self.selected.insert(path);
                                        }
                                    }
                                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空选择").clicked() {
                                        self.selected.clear();
                                    }
                                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Export), "导出 CSV").clicked() {
                                        if let Some(path) = super::save_file("duplicates.csv", "CSV 文件", &["csv"]) {
                                            match export_groups_csv(&path, groups) {
                                                Ok(()) => env.toast("重复文件清单已导出", crate::app::ToastKind::Success),
                                                Err(e) => env.toast(format!("导出失败: {e}"), crate::app::ToastKind::Error),
                                            }
                                        }
                                    }
                                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Export), "保存结果").clicked() {
                                        if let Some(path) = super::save_file("duplicates.fts.json", "JSON 文件", &["json"]) {
                                            match save_scan(&path, result, &self.scan_roots, self.mode) {
                                                Ok(()) => env.toast("扫描结果已保存，可在以后载入", crate::app::ToastKind::Success),
                                                Err(e) => env.toast(format!("保存失败: {e}"), crate::app::ToastKind::Error),
                                            }
                                        }
                                    }
                                    if !self.selected.is_empty() && widgets::danger_button(ui, Some(widgets::ToolIcon::Trash), &format!("删除选中 ({})", self.selected.len())).clicked() {
                                        self.confirm_delete = true;
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if !self.show_all_groups && groups.len() > 80 {
                                            if ui.small_button("显示全部").clicked() {
                                                self.show_all_groups = true;
                                            }
                                        }
                                    });
                                });
                                ui.add_space(4.0);
                                let display_count = if self.show_all_groups { groups.len() } else { groups.len().min(80) };
                                for (gi, group) in groups.iter().take(display_count).enumerate() {
                                    draw_group(ui, gi, group, &mut self.selected);
                                }
                                if display_count < groups.len() {
                                    super::small_muted(ui, format!("仅显示前 80 组，共 {} 组；可导出 CSV 查看全部。", groups.len()));
                                }
                            }
                            ScanOutcome::Files(files) => {
                                draw_files_table(ui, files, &mut self.selected, &mut self.confirm_delete, env);
                            }
                            ScanOutcome::Dirs(dirs) => {
                                ui.horizontal(|ui| {
                                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Check), "全选").clicked() {
                                        for dir in dirs {
                                            self.selected.insert(dir.clone());
                                        }
                                    }
                                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空选择").clicked() {
                                        self.selected.clear();
                                    }
                                    if !self.selected.is_empty() && widgets::danger_button(ui, Some(widgets::ToolIcon::Trash), &format!("删除选中 ({})", self.selected.len())).clicked() {
                                        self.confirm_delete = true;
                                    }
                                });
                                egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, false]).show(ui, |ui| {
                                    for dir in dirs {
                                        let mut checked = self.selected.contains(dir);
                                        ui.horizontal(|ui| {
                                            if ui.checkbox(&mut checked, "").changed() {
                                                if checked {
                                                    self.selected.insert(dir.clone());
                                                } else {
                                                    self.selected.remove(dir);
                                                }
                                            }
                                            ui.label(RichText::new(dir.display().to_string()).size(11.0)).on_hover_text(dir.display().to_string());
                                        });
                                    }
                                });
                            }
                        }
                    });
                } else if !job_view(&self.job).0 {
                    section_card(ui, "扫描结果", None, |ui| {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Copies,
                            "准备好了",
                            "选择模式与目录后点击“开始扫描”。所有计算与删除均在本机执行。",
                        );
                    });
                }
                ui.add_space(6.0);
            });

        if self.result.is_some() {
            progress_row(ui, &self.delete_job);
            if self.confirm_delete {
                let count = self.selected.len();
                let bytes = self.selected_bytes();
                let action = if self.move_to_trash { "移入回收站" } else { "永久删除" };
                let body = format!(
                    "即将{action} {count} 个项目，合计 {}。
此操作不可通过本程序撤销，请确认已保留需要保留的文件。",
                    util::format_size(bytes)
                );
                if confirm_dialog(ui.ctx(), &mut self.confirm_delete, "确认清理", &body, action, true) {
                    self.start_delete(ui.ctx(), env);
                }
            }
        } else if self.confirm_delete {
            self.confirm_delete = false;
        }
    }
}
fn draw_files_table(ui: &mut egui::Ui, files: &[FileEntry], selected: &mut HashSet<PathBuf>, confirm_delete: &mut bool, env: &mut TabEnv) {
        ui.horizontal(|ui| {
            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Check), "全选").clicked() {
                for file in files {
                    selected.insert(file.path.clone());
                }
            }
            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空选择").clicked() {
                selected.clear();
            }
            if !selected.is_empty() && widgets::danger_button(ui, Some(widgets::ToolIcon::Trash), &format!("删除选中 ({})", selected.len())).clicked() {
                *confirm_delete = true;
            }
            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Export), "导出 CSV").clicked() {
                if let Some(path) = super::save_file("scan.csv", "CSV 文件", &["csv"]) {
                    let groups: Vec<DupGroup> = files
                        .iter()
                        .map(|f| DupGroup { label: f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), files: vec![f.clone()], wasted: 0 })
                        .collect();
                    match export_groups_csv(&path, &groups) {
                        Ok(()) => env.toast("清单已导出", crate::app::ToastKind::Success),
                        Err(e) => env.toast(format!("导出失败: {e}"), crate::app::ToastKind::Error),
                    }
                }
            }
        });
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(30.0).clip(true))
            .column(Column::remainder().at_least(260.0).clip(true))
            .column(Column::initial(90.0).at_least(70.0).clip(true))
            .column(Column::initial(140.0).at_least(110.0).clip(true))
            .max_scroll_height(400.0)
            .header(24.0, |mut header| {
                for title in ["", "路径", "大小", "修改时间"] {
                    header.col(|ui| {
                        ui.label(RichText::new(title).size(11.0).strong().color(theme::dim(ui)));
                    });
                }
            })
            .body(|body| {
                body.rows(24.0, files.len(), |mut row| {
                    let file = &files[row.index()];
                    row.col(|ui| {
                        let mut checked = selected.contains(&file.path);
                        if ui.checkbox(&mut checked, "").changed() {
                            if checked {
                                selected.insert(file.path.clone());
                            } else {
                                selected.remove(&file.path);
                            }
                        }
                    });
                    let mut path_hit = None;
                    row.col(|ui| {
                        path_hit = Some(
                            ui.add(
                                egui::Label::new(
                                    RichText::new(super::truncate_path(&file.path.display().to_string(), 100)).size(11.0),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text(file.path.display().to_string()),
                        );
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(util::format_size(file.size)).size(11.0).color(theme::dim(ui)));
                    });
                    row.col(|ui| {
                        let text = if file.modified > 0 {
                            chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + std::time::Duration::from_secs(file.modified as u64))
                                .format("%Y-%m-%d %H:%M")
                                .to_string()
                        } else {
                            "—".into()
                        };
                        ui.label(RichText::new(text).size(10.5).color(theme::dim(ui)));
                    });
                    if let Some(hit) = path_hit {
                        super::path_actions(hit, &file.path);
                    }
                    super::path_actions(row.response(), &file.path);
                });
            });
    }

fn draw_group(ui: &mut egui::Ui, index: usize, group: &DupGroup, selected: &mut HashSet<PathBuf>) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                widgets::pill(ui, format!("#{}", index + 1), theme::ACCENT);
                ui.label(RichText::new(&group.label).size(12.0).strong());
                ui.label(RichText::new(format!("{} 份", group.files.len())).size(10.5).color(theme::dim(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("可释放 {}", util::format_size(group.wasted))).size(10.5).color(theme::SUCCESS));
                    if ui.small_button("保留首项并选中其余").clicked() {
                        for file in group.files.iter().skip(1) {
                            selected.insert(file.path.clone());
                        }
                    }
                });
            });
            for file in &group.files {
                let mut checked = selected.contains(&file.path);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut checked, "").changed() {
                        if checked {
                            selected.insert(file.path.clone());
                        } else {
                            selected.remove(&file.path);
                        }
                    }
                    let response = ui.label(RichText::new(super::truncate_path(&file.path.display().to_string(), 115)).size(10.5).color(theme::dim(ui)))
                        .on_hover_text(file.path.display().to_string());
                    super::path_actions(response, &file.path);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(util::format_size(file.size)).size(10.0).color(theme::TEXT_DIM.gamma_multiply(0.9)));
                    });
                });
            }
        });
    ui.add_space(4.0);
}

fn split_list(raw: &str) -> Vec<String> {
    raw.split([',', ';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
