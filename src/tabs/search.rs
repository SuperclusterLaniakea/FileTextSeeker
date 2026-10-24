use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::config::AppConfig;
use crate::search::index::{build_database, FileRecord, IndexOptions, IndexSummary, SearchDb};
use crate::search::query::{QueryOptions, QueryProgram, SortField};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{add_folders_button, job_cancel, job_running, job_start, job_take, job_view, path_chips, progress_row, section_card, stat_row, toggle_chip, Job, TabEnv};

const MAX_PENDING_EVENTS: usize = 20_000;

pub struct SearchTab {
    pub query: String,
    roots: Vec<String>,
    indexed_roots: Vec<String>,
    index_options: IndexOptions,
    index_file: PathBuf,
    legacy_file: PathBuf,
    index_ready: bool,
    index_files: u64,
    index_folders: u64,
    index_errors: u64,
    pending_events: Vec<PathBuf>,
    pending_keys: HashSet<String>,
    pub hits: Vec<FileRecord>,
    query_error: Option<String>,
    job: Job<IndexSummary>,
    last_elapsed_ms: u128,
    searched: bool,
    advanced: bool,
    exclusions_text: String,
    exclude_patterns_text: String,
    exact_name: bool,
    auto_loaded: bool,
    loading_index: bool,
}

impl SearchTab {
    pub fn new(config: &AppConfig) -> Self {
        let index_file = PathBuf::from(&config.search.index_file);
        let legacy_file = index_file.with_extension("json");
        Self {
            query: config.search.last_query.clone(),
            roots: config.search.roots.clone(),
            indexed_roots: Vec::new(),
            index_options: IndexOptions {
                excluded_dirs: config.search.excluded_dirs.clone(),
                exclude_patterns: config.search.exclude_patterns.clone(),
                include_hidden: config.search.include_hidden,
                include_system_dirs: config.search.include_system_dirs,
            },
            index_file,
            legacy_file,
            index_ready: false,
            index_files: 0,
            index_folders: 0,
            index_errors: 0,
            pending_events: Vec::new(),
            pending_keys: HashSet::new(),
            hits: Vec::new(),
            query_error: None,
            job: super::new_job(),
            last_elapsed_ms: 0,
            searched: false,
            advanced: false,
            exclusions_text: config.search.excluded_dirs.join(", "),
            exclude_patterns_text: config.search.exclude_patterns.join(", "),
            exact_name: config.search.exact_name,
            auto_loaded: false,
            loading_index: false,
        }
    }

    fn query_options(&self, env: &TabEnv) -> QueryOptions {
        QueryOptions {
            match_case: env.config.search.match_case,
            match_path: env.config.search.match_path,
            whole_word: env.config.search.whole_word,
            regex_mode: env.config.search.regex_mode,
        }
    }

    fn indexed_root_paths(&self) -> Vec<PathBuf> {
        self.indexed_roots.iter().map(PathBuf::from).collect()
    }

    fn run_search(&mut self, env: &mut TabEnv) {
        if !self.index_ready {
            env.toast("请先建立文件索引", crate::app::ToastKind::Warn);
            return;
        }
        let db = SearchDb::new(self.index_file.clone());
        if !db.exists() {
            self.index_ready = false;
            env.toast("索引数据库不存在，请重新建立索引", crate::app::ToastKind::Warn);
            return;
        }
        let options = self.query_options(env);
        let started = std::time::Instant::now();
        let sort = SortField::from_id(&env.config.search.sort_field);
        let ascending = env.config.search.sort_asc;
        let limit = env.config.search.max_results.max(1);
        let result: Result<Vec<FileRecord>, String> = if self.exact_name {
            let needle = self.query.trim().to_lowercase();
            if needle.is_empty() {
                Ok(Vec::new())
            } else {
                db.exact(&needle, sort, ascending, limit)
            }
        } else {
            QueryProgram::parse(&self.query, &options)
                .and_then(|program| db.query(&program, sort, ascending, limit))
        };
        match result {
            Ok(hits) => {
                self.hits = hits;
                self.query_error = None;
                self.last_elapsed_ms = started.elapsed().as_millis();
                self.searched = true;
                let q = self.query.trim().to_string();
                if !q.is_empty() {
                    env.config.search.last_query = q.clone();
                    env.config.search.history.retain(|h| h != &q);
                    env.config.search.history.insert(0, q);
                    env.config.search.history.truncate(12);
                    env.mark_dirty();
                }
                env.set_status(format!(
                    "文件搜索 · {} · 找到 {} 个结果，用时 {} ms",
                    if self.exact_name { "精确匹配" } else { "查询" },
                    self.hits.len(),
                    self.last_elapsed_ms
                ));
            }
            Err(error) => {
                self.query_error = Some(error.clone());
                env.set_status(format!("查询语法错误 · {error}"));
            }
        }
    }

    fn start_build(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        self.sync_config(env);
        let roots: Vec<PathBuf> = self.roots.iter().map(PathBuf::from).collect();
        if roots.is_empty() {
            env.toast("请至少添加一个搜索目录", crate::app::ToastKind::Warn);
            return;
        }
        let options = IndexOptions {
            excluded_dirs: split_list(&self.exclusions_text),
            exclude_patterns: split_list(&self.exclude_patterns_text),
            include_hidden: env.config.search.include_hidden,
            include_system_dirs: env.config.search.include_system_dirs,
        };
        self.index_options = options.clone();
        self.loading_index = false;
        let path = self.index_file.clone();
        let legacy = self.legacy_file.clone();
        job_start(&self.job, ctx, move |cancel, sink| {
            sink.set("扫描文件系统", 0, 0, path.display().to_string());
            let summary = build_database(&path, &roots, &options, cancel, |count, entry| {
                sink.set("写入索引数据库", count as u64, 0, entry.display().to_string());
            })?;
            if legacy != path && legacy.is_file() {
                let _ = std::fs::remove_file(&legacy);
            }
            Ok(summary)
        });
    }

    fn sync_config(&mut self, env: &mut TabEnv) {
        if env.config.search.roots != self.roots {
            env.config.search.roots = self.roots.clone();
            env.mark_dirty();
        }
        env.config.search.excluded_dirs = split_list(&self.exclusions_text);
        env.config.search.exclude_patterns = split_list(&self.exclude_patterns_text);
        if env.config.search.exact_name != self.exact_name {
            env.config.search.exact_name = self.exact_name;
            env.mark_dirty();
        }
    }

    fn load_from_disk(&mut self, ctx: &egui::Context) {
        if job_running(&self.job) {
            return;
        }
        let path = self.index_file.clone();
        self.job = super::new_job();
        self.loading_index = true;
        job_start(&self.job, ctx, move |_cancel, sink| {
            sink.set("加载索引数据库", 0, 0, path.display().to_string());
            SearchDb::new(path).summary()
        });
    }

    pub fn watch_roots(&self) -> Vec<PathBuf> {
        if !self.index_ready || !self.index_file.is_file() {
            return Vec::new();
        }
        self.indexed_root_paths()
    }

    pub fn apply_fs_events(
        &mut self,
        paths: &[PathBuf],
        _ctx: &egui::Context,
        env: &mut TabEnv,
    ) -> bool {
        if !self.index_ready {
            if job_running(&self.job) {
                self.queue_events(paths);
                return true;
            }
            return false;
        }
        let roots = self.indexed_root_paths();
        let relevant: Vec<PathBuf> = paths
            .iter()
            .filter(|path| {
                roots
                    .iter()
                    .any(|root| path.starts_with(root) && self.index_options.accepts_path(path, root))
            })
            .cloned()
            .collect();
        if relevant.is_empty() {
            return false;
        }
        if job_running(&self.job) {
            self.queue_events(&relevant);
            return true;
        }
        if self.apply_events_now(&relevant, env) {
            self.refresh_query(env);
        }
        true
    }

    fn queue_events(&mut self, paths: &[PathBuf]) {
        for path in paths {
            if self.pending_events.len() >= MAX_PENDING_EVENTS {
                break;
            }
            let key = path.to_string_lossy().to_lowercase();
            if self.pending_keys.insert(key) {
                self.pending_events.push(path.clone());
            }
        }
    }

    fn apply_pending_events(&mut self, env: &mut TabEnv) {
        if self.pending_events.is_empty() || !self.index_ready {
            return;
        }
        let events = std::mem::take(&mut self.pending_events);
        self.pending_keys.clear();
        self.apply_events_now(&events, env);
    }

    fn apply_events_now(&mut self, events: &[PathBuf], env: &mut TabEnv) -> bool {
        let db = SearchDb::new(self.index_file.clone());
        let roots = self.indexed_root_paths();
        match db.apply_changes(events, &roots, &self.index_options) {
            Ok(stats) => {
                self.index_files = (self.index_files as i64 + stats.files_delta).max(0) as u64;
                self.index_folders = (self.index_folders as i64 + stats.folders_delta).max(0) as u64;
                self.index_errors += stats.errors;
                if stats.added + stats.updated + stats.removed > 0 {
                    env.set_status(format!(
                        "索引增量更新 · 新增 {} · 更新 {} · 删除 {}",
                        stats.added, stats.updated, stats.removed
                    ));
                }
                true
            }
            Err(error) => {
                env.toast(format!("索引增量更新失败: {error}"), crate::app::ToastKind::Error);
                false
            }
        }
    }

    fn refresh_query(&mut self, env: &mut TabEnv) {
        if !self.searched || self.query.trim().is_empty() {
            return;
        }
        self.run_search(env);
    }

    fn finish_job(&mut self, summary: IndexSummary, env: &mut TabEnv) {
        let loading = self.loading_index;
        self.loading_index = false;
        self.index_ready = true;
        self.index_files = summary.files;
        self.index_folders = summary.folders;
        self.index_errors = summary.errors;
        if !summary.roots.is_empty() {
            self.roots = summary.roots.iter().map(|path| path.display().to_string()).collect();
            self.indexed_roots = self.roots.clone();
        }
        if let Some(options) = summary.options {
            self.index_options = options;
        }
        self.apply_pending_events(env);
        self.run_search(env);
        env.mark_dirty();
        env.toast(
            format!(
                "{} · {} 个文件 · {} 个文件夹 · SQLite",
                if loading { "索引已加载" } else { "索引就绪" },
                self.index_files,
                self.index_folders
            ),
            crate::app::ToastKind::Success,
        );
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        self.sync_config(env);
        if let Some(summary) = job_take(&self.job) {
            self.finish_job(summary, env);
        }
        if !self.auto_loaded {
            self.auto_loaded = true;
            if !self.index_ready && !job_running(&self.job) {
                if self.index_file.is_file() {
                    self.load_from_disk(ui.ctx());
                } else if self.legacy_file.is_file() && !self.roots.is_empty() {
                    env.toast(
                        "检测到旧版 JSON 索引，正在改用 SQLite 数据库重建",
                        crate::app::ToastKind::Info,
                    );
                    self.start_build(ui.ctx(), env);
                }
            }
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                stat_row(
                    ui,
                    &[
                        ("索引文件", format_count(self.index_files as usize), theme::ACCENT),
                        ("结果数量", format_count(self.hits.len()), theme::BLUE),
                        ("搜索目录", format_count(self.roots.len()), theme::SUCCESS),
                        ("查询耗时", if self.searched { format!("{} ms", self.last_elapsed_ms) } else { "—".into() }, theme::WARN),
                    ],
                );
                ui.add_space(10.0);

                section_card(ui, "搜索范围", Some("索引会跳过隐藏、系统目录与排除规则；修改范围后需要重建"), |ui| {
                    path_chips(ui, &mut self.roots, "尚未添加搜索目录");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if add_folders_button(ui, &mut self.roots, "添加目录") {
                            env.mark_dirty();
                        }
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空").clicked() {
                            self.roots.clear();
                            env.mark_dirty();
                        }
                        let index_outdated = self.index_ready && self.roots != self.indexed_roots;
                        let (running, _, _, _) = job_view(&self.job);
                        if running {
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消索引").clicked() {
                                job_cancel(&self.job);
                            }
                        } else if widgets::primary_button(ui, Some(widgets::ToolIcon::Refresh), if self.index_ready { "重建索引" } else { "建立索引" }).clicked() {
                            self.start_build(ui.ctx(), env);
                        }
                        if !running {
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Open), "重新加载").clicked() {
                                self.load_from_disk(ui.ctx());
                            }
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "打开目录").clicked() {
                                let root = self.indexed_roots.first().or(self.roots.first());
                                if let Some(root) = root {
                                    util::open_path(std::path::Path::new(root));
                                }
                            }
                        }
                        if index_outdated {
                            widgets::pill(ui, "目录已变更，索引需重建", theme::WARN);
                        } else if self.index_ready {
                            widgets::pill(ui, format!("已索引 {} 项", self.index_files + self.index_folders), theme::SUCCESS);
                        }
                    });
                    progress_row(ui, &self.job);
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        super::small_muted(ui, "索引数据库");
                        let index_path = self.index_file.display().to_string();
                        ui.label(RichText::new(super::truncate_path(&index_path, 58)).size(10.5).color(theme::dim(ui)))
                            .on_hover_text(index_path.clone());
                        if self.index_file.is_file() {
                            widgets::pill(ui, "SQLite · 实时写入", theme::SUCCESS);
                        }
                        if !index_path.is_empty() {
                            if ui.small_button("定位").clicked() {
                                util::reveal_path(&self.index_file);
                            }
                        }
                    });
                    if env.config.monitoring && self.index_ready {
                        ui.label(
                            RichText::new("实时监控只增量写入新增、修改和删除的条目，不会重建整个索引")
                                .size(10.5)
                                .color(theme::dim(ui)),
                        );
                    }
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        if widgets::toggle_switch(ui, &mut env.config.search.include_hidden).changed() {
                            env.mark_dirty();
                        }
                        super::small_muted(ui, "包含隐藏文件");
                        ui.add_space(8.0);
                        if widgets::toggle_switch(ui, &mut env.config.search.include_system_dirs).changed() {
                            env.mark_dirty();
                        }
                        super::small_muted(ui, "包含系统目录");
                        ui.add_space(8.0);
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::ChevronDown), "高级排除").clicked() {
                            self.advanced = !self.advanced;
                        }
                    });
                    if self.advanced {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("排除目录").size(11.0).color(theme::dim(ui)));
                            let w = (ui.available_width() - 90.0).max(160.0);
                            if ui.add_sized([w, 26.0], egui::TextEdit::singleline(&mut self.exclusions_text).hint_text("node_modules, .git")).changed() {
                                env.mark_dirty();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("排除文件").size(11.0).color(theme::dim(ui)));
                            let w = (ui.available_width() - 90.0).max(160.0);
                            if ui.add_sized([w, 26.0], egui::TextEdit::singleline(&mut self.exclude_patterns_text).hint_text("*.tmp, *.log")).changed() {
                                env.mark_dirty();
                            }
                        });
                    }
                });
                ui.add_space(8.0);

                section_card(ui, "搜索条件", None, |ui| {
                    let mut do_search = false;
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 260.0).max(220.0);
                        let hint = if self.exact_name {
                            "输入完整文件名（不区分大小写），例如：report.docx"
                        } else {
                            "输入文件名，支持  AND / OR / NOT、size:>10mb、ext:rs、date:today 等语法"
                        };
                        let response = ui.add_sized(
                            [width, 32.0],
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text(hint)
                                .margin(egui::Vec2::new(9.0, 7.0)),
                        );
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            do_search = true;
                        }
                        if widgets::primary_button(ui, Some(widgets::ToolIcon::Search), "搜索").clicked() {
                            do_search = true;
                        }
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清除").clicked() {
                            self.query.clear();
                            self.hits.clear();
                            self.searched = false;
                            self.query_error = None;
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        let mut opts_changed = false;
                        if toggle_chip(ui, self.exact_name, "仅精确文件名") {
                            self.exact_name = !self.exact_name;
                            env.mark_dirty();
                            do_search = self.searched;
                        }
                        for (label, field) in [
                            ("区分大小写", &mut env.config.search.match_case),
                            ("匹配路径", &mut env.config.search.match_path),
                            ("全词匹配", &mut env.config.search.whole_word),
                            ("正则模式", &mut env.config.search.regex_mode),
                        ] {
                            if toggle_chip(ui, *field, label) {
                                *field = !*field;
                                opts_changed = true;
                            }
                        }
                        if opts_changed {
                            env.mark_dirty();
                            do_search = self.searched;
                        }
                        ui.add_space(8.0);
                        super::small_muted(ui, "排序");
                        let current = SortField::from_id(&env.config.search.sort_field);
                        egui::ComboBox::from_id_salt("search_sort")
                            .selected_text(current.label())
                            .width(92.0)
                            .show_ui(ui, |ui| {
                                for sort in [SortField::Name, SortField::Path, SortField::Size, SortField::Modified, SortField::Type, SortField::Extension] {
                                    if ui.selectable_value(&mut env.config.search.sort_field, sort.id().to_string(), sort.label()).changed() {
                                        env.mark_dirty();
                                        do_search = self.searched;
                                    }
                                }
                            });
                        if ui.selectable_label(env.config.search.sort_asc, if env.config.search.sort_asc { "升序" } else { "降序" }).clicked() {
                            env.config.search.sort_asc = !env.config.search.sort_asc;
                            env.mark_dirty();
                            do_search = self.searched;
                        }
                    });
                    if let Some(error) = &self.query_error {
                        ui.label(RichText::new(error).size(11.5).color(theme::DANGER));
                    }
                    if !env.config.search.history.is_empty() {
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            super::small_muted(ui, "最近：");
                            let history = env.config.search.history.clone();
                            for item in history.iter().take(6) {
                                if ui.small_button(item).clicked() {
                                    self.query = item.clone();
                                    do_search = true;
                                }
                            }
                        });
                    }
                    if do_search {
                        self.run_search(env);
                    }
                });
                ui.add_space(8.0);

                section_card(ui, "搜索结果", Some("双击打开，右键查看更多操作"), |ui| {
                    if !self.index_ready {
                        widgets::empty_state(ui, widgets::ToolIcon::Search, "等待建立索引", "添加目录并点击“建立索引”，索引会写入本地 SQLite 数据库，并随文件变更增量更新。");
                        return;
                    }
                    if self.hits.is_empty() {
                        widgets::empty_state(
                            ui,
                            widgets::ToolIcon::Search,
                            if self.searched { "没有匹配的文件" } else { "输入条件开始搜索" },
                            "支持组合条件、通配符、正则以及 size / ext / date 过滤器。",
                        );
                        return;
                    }
                    let rows = &self.hits;
                    TableBuilder::new(ui)
                        .striped(true)
                        .sense(egui::Sense::click())
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::initial(300.0).at_least(180.0).resizable(true).clip(true))
                        .column(Column::remainder().at_least(220.0).clip(true))
                        .column(Column::initial(92.0).at_least(70.0).clip(true))
                        .column(Column::initial(138.0).at_least(120.0).clip(true))
                        .max_scroll_height(360.0)
                        .header(24.0, |mut header| {
                            for title in ["名称", "所在位置", "大小", "修改时间"] {
                                header.col(|ui| {
                                    ui.label(RichText::new(title).size(11.0).strong().color(theme::dim(ui)));
                                });
                            }
                        })
                        .body(|body| {
                            body.rows(24.0, rows.len(), |mut row| {
                                let index = row.index();
                                let record = &rows[index];
                                row.col(|ui| {
                                    let icon = if record.is_dir { widgets::ToolIcon::Folder } else { widgets::ToolIcon::Document };
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                                        widgets::paint_icon(ui, rect, icon, if record.is_dir { theme::WARN } else { theme::ACCENT });
                                        let name = ui.add(
                                            egui::Label::new(RichText::new(&record.name).size(12.0))
                                                .sense(egui::Sense::click()),
                                        );
                                        super::path_actions(name, &record.path);
                                    });
                                });
                                row.col(|ui| {
                                    let parent = record.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                                    ui.label(RichText::new(super::truncate_path(&parent, 70)).size(11.0).color(theme::dim(ui)))
                                        .on_hover_text(parent);
                                });
                                row.col(|ui| {
                                    ui.label(RichText::new(if record.is_dir { "—".into() } else { util::format_size(record.size) }).size(11.0).color(theme::dim(ui)));
                                });
                                row.col(|ui| {
                                    let time = if record.modified > 0 {
                                        chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + std::time::Duration::from_secs(record.modified as u64))
                                            .format("%Y-%m-%d %H:%M")
                                            .to_string()
                                    } else {
                                        "—".into()
                                    };
                                    ui.label(RichText::new(time).size(11.0).color(theme::dim(ui)));
                                });
                                super::path_actions(row.response(), &record.path);
                            });
                        });
                });
                ui.add_space(6.0);
            });
    }
}

fn split_list(raw: &str) -> Vec<String> {
    raw.split([',', ';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn format_count(value: usize) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 10_000 {
        format!("{:.1}K", value as f64 / 1000.0)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::index::build_database;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn incremental_events_add_and_remove_hits() {
        let root = std::env::temp_dir().join(format!("fts_search_events_{}", std::process::id()));
        let data_dir = root.join("data");
        let index_file = data_dir.join("search_index.db");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("base.txt"), "base").unwrap();

        let options = IndexOptions::default();
        let cancel = AtomicBool::new(false);
        build_database(&index_file, &[root.clone()], &options, &cancel, |_, _| {}).unwrap();

        let mut config = AppConfig::default();
        config.search.roots = vec![root.to_string_lossy().into_owned()];
        config.search.index_file = index_file.display().to_string();
        let mut tab = SearchTab::new(&config);
        tab.query = "added".into();
        tab.index_ready = true;
        tab.index_files = 1;
        tab.indexed_roots = vec![root.to_string_lossy().into_owned()];
        tab.index_options = options;
        tab.searched = true;

        let ctx = egui::Context::default();
        let mut status = String::new();
        let mut env = TabEnv {
            config: &mut config,
            data_dir: &data_dir,
            status: &mut status,
            toasts: Vec::new(),
            dirty: false,
        };

        let added = root.join("added_note.txt");
        std::fs::write(&added, "hello").unwrap();
        assert!(tab.apply_fs_events(&[added.clone()], &ctx, &mut env));
        assert!(tab.hits.iter().any(|hit| hit.name == "added_note.txt"));

        std::fs::remove_file(&added).unwrap();
        assert!(tab.apply_fs_events(&[added], &ctx, &mut env));
        assert!(!tab.hits.iter().any(|hit| hit.name == "added_note.txt"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
