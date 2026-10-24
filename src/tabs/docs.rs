use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::config::{AppConfig, DocDatabase};
use crate::docs::{build_index, db_size_bytes, BuildMode, BuildStats, DocHit, DocQueryOptions, DocSort, DocStore};
use crate::theme;
use crate::util;
use crate::widgets;

use super::{job_start, job_take, job_view, progress_row, section_card, stat_row, Job, TabEnv};

struct BuildDone {
    stats: BuildStats,
    doc_count: u64,
    db_size: u64,
    db_id: String,
}

pub struct DocsTab {
    active_db: String,
    query: String,
    hits: Vec<DocHit>,
    build_job: Job<BuildDone>,
    search_job: Job<Vec<DocHit>>,
    show_add: bool,
    new_name: String,
    new_root: String,
    new_desc: String,
    last_build_note: String,
    last_index_time: String,
    pending_dbs: Vec<String>,
    pending_since: Option<Instant>,
    size_db: String,
    size_at: Option<Instant>,
    size_value: u64,
}

impl DocsTab {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            active_db: config.docs.active_db.clone(),
            query: String::new(),
            hits: Vec::new(),
            build_job: super::new_job(),
            search_job: super::new_job(),
            show_add: false,
            new_name: String::new(),
            new_root: String::new(),
            new_desc: String::new(),
            last_build_note: String::new(),
            last_index_time: String::new(),
            pending_dbs: Vec::new(),
            pending_since: None,
            size_db: String::new(),
            size_at: None,
            size_value: 0,
        }
    }

    /// Directory sizes are expensive to walk, so refresh at most every few
    /// seconds instead of on every repaint.
    fn cached_size(&mut self, env: &TabEnv) -> u64 {
        if self.active_db.is_empty() {
            return 0;
        }
        let path = Self::db_path(env, &self.active_db);
        let stale = self.size_db != self.active_db
            || self
                .size_at
                .map_or(true, |at| at.elapsed() >= Duration::from_secs(5));
        if stale {
            self.size_value = db_size_bytes(&path);
            self.size_db = self.active_db.clone();
            self.size_at = Some(Instant::now());
        }
        self.size_value
    }

    fn db_path(env: &TabEnv, id: &str) -> PathBuf {
        env.data_dir.join("docs").join(id)
    }

    fn current_db<'a>(config: &'a AppConfig, active: &str) -> Option<&'a DocDatabase> {
        config.docs.databases.iter().find(|d| d.id == active)
    }

    fn sync_selection(&mut self, env: &mut TabEnv) {
        env.config.docs.active_db = self.active_db.clone();
    }

    fn start_build(&mut self, ctx: &egui::Context, env: &mut TabEnv, mode: BuildMode) {
        self.start_build_for(ctx, env, self.active_db.clone(), mode);
    }

    fn start_build_for(&mut self, ctx: &egui::Context, env: &mut TabEnv, db_id: String, mode: BuildMode) {
        let Some(db) = Self::current_db(env.config, &db_id).cloned() else {
            env.toast("请先创建或选择一个文档库", crate::app::ToastKind::Warn);
            return;
        };
        if !Path::new(&db.root_dir).is_dir() {
            env.toast("文档库目录不存在，请检查路径", crate::app::ToastKind::Error);
            return;
        }
        let base = Self::db_path(env, &db.id);
        let root = PathBuf::from(&db.root_dir);
        let db_id = db.id.clone();
        self.last_build_note.clear();
        job_start(&self.build_job, ctx, move |cancel, sink| {
            let mut message = String::new();
            let stats = build_index(
                &base,
                &root,
                mode,
                cancel,
                &mut |scanned, indexed, path| {
                    sink.set("解析并写入文档索引", scanned, indexed, path.display().to_string());
                },
                &mut |log| {
                    message = log;
                },
            )?;
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("已取消索引".into());
            }
            let doc_count = DocStore::open(&base).map(|s| s.doc_count()).unwrap_or(0);
            let db_size = db_size_bytes(&base);
            let _ = message;
            Ok(BuildDone { stats, doc_count, db_size, db_id })
        });
    }

    fn start_search(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let Some(db) = Self::current_db(env.config, &self.active_db).cloned() else {
            env.toast("请先选择一个文档库", crate::app::ToastKind::Warn);
            return;
        };
        let base = Self::db_path(env, &db.id);
        if !base.join("index").is_dir() {
            env.toast("该文档库尚未建立索引", crate::app::ToastKind::Warn);
            return;
        }
        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.hits.clear();
            return;
        }
        let all_terms = env.config.docs.all_terms;
        let limit = env.config.docs.max_results.max(1);
        env.config.docs.active_db = self.active_db.clone();
        env.config.docs.history.retain(|h| h != &query);
        env.config.docs.history.insert(0, query.clone());
        env.config.docs.history.truncate(12);
        env.mark_dirty();
        job_start(&self.search_job, ctx, move |_cancel, sink| {
            sink.set("全文检索", 0, 0, &query);
            let store = DocStore::open(&base)?;
            store.search(&query, &DocQueryOptions { all_terms, limit })
        });
    }

    fn add_database(&mut self, env: &mut TabEnv) {
        let name = self.new_name.trim();
        let root = self.new_root.trim();
        if name.is_empty() || root.is_empty() {
            env.toast("请填写文档库名称和目录", crate::app::ToastKind::Warn);
            return;
        }
        let id = format!("db_{}", chrono::Local::now().format("%Y%m%d%H%M%S%3f"));
        let db = DocDatabase {
            id: id.clone(),
            name: name.to_string(),
            description: self.new_desc.trim().to_string(),
            root_dir: root.to_string(),
            created_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            last_index_time: "从未建立".into(),
            indexed_files: 0,
        };
        env.config.docs.databases.push(db);
        env.config.docs.active_db = id.clone();
        self.active_db = id;
        self.hits.clear();
        self.show_add = false;
        self.new_name.clear();
        self.new_root.clear();
        self.new_desc.clear();
        env.mark_dirty();
        self.sync_selection(env);
        env.toast("文档库已创建，接下来可以建立索引", crate::app::ToastKind::Success);
    }

    pub fn watch_roots(config: &AppConfig, data_dir: &Path) -> Vec<PathBuf> {
        if !config.docs.auto_update {
            return Vec::new();
        }
        config
            .docs
            .databases
            .iter()
            .filter(|db| {
                data_dir
                    .join("docs")
                    .join(&db.id)
                    .join("index")
                    .join("meta.json")
                    .exists()
            })
            .map(|db| PathBuf::from(&db.root_dir))
            .collect()
    }

    pub fn apply_fs_events(&mut self, paths: &[PathBuf], env: &mut TabEnv) -> bool {
        if !env.config.docs.auto_update || !env.config.monitoring {
            return false;
        }
        let data_dir = env.data_dir;
        let mut affected = false;
        for db in &env.config.docs.databases {
            let indexed = data_dir
                .join("docs")
                .join(&db.id)
                .join("index")
                .join("meta.json")
                .exists();
            if !indexed {
                continue;
            }
            if paths.iter().any(|path| path.starts_with(&db.root_dir)) {
                affected = true;
                if !self.pending_dbs.contains(&db.id) {
                    self.pending_dbs.push(db.id.clone());
                }
            }
        }
        if affected && self.pending_since.is_none() {
            self.pending_since = Some(Instant::now());
        }
        affected
    }

    pub fn tick(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        if self.pending_dbs.is_empty() {
            return;
        }
        if !env.config.monitoring || !env.config.docs.auto_update {
            self.pending_dbs.clear();
            self.pending_since = None;
            return;
        }
        if job_view(&self.build_job).0 {
            return;
        }
        let ready = self
            .pending_since
            .map(|since| since.elapsed() >= Duration::from_secs(3))
            .unwrap_or(true);
        if !ready {
            return;
        }
        let db_id = self.pending_dbs.remove(0);
        self.pending_since = if self.pending_dbs.is_empty() {
            None
        } else {
            Some(Instant::now())
        };
        self.start_build_for(ctx, env, db_id, BuildMode::Update);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, env: &mut TabEnv) {
        self.sync_selection(env);
        if let Some(done) = job_take(&self.build_job) {
            self.size_db = done.db_id.clone();
            self.size_value = done.db_size;
            self.size_at = Some(Instant::now());
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
            if let Some(db) = env.config.docs.databases.iter_mut().find(|d| d.id == done.db_id) {
                db.indexed_files = done.doc_count;
                db.last_index_time = now.clone();
            }
            env.mark_dirty();
            self.last_build_note = format!(
                "扫描 {} · 已索引 {} · 跳过 {} · 单元 {}",
                done.stats.scanned, done.stats.indexed_files, done.stats.skipped, done.stats.units
            );
            self.last_index_time = now;
            env.toast(
                format!("文档索引完成 · {} 个文档", done.doc_count),
                crate::app::ToastKind::Success,
            );
            env.set_status(format!("文档索引完成 · {} 个文档 · 数据库 {:.1} MB", done.doc_count, done.db_size as f64 / 1024.0 / 1024.0));
        }
        if let Some(mut hits) = job_take(&self.search_job) {
            let count = hits.len();
            DocSort::from_id(&env.config.docs.sort_by).apply(&mut hits);
            self.hits = hits;
            env.set_status(format!("文档搜索 · 命中 {count} 个文档"));
        }
        let cached_size = self.cached_size(env);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let db = Self::current_db(env.config, &self.active_db).cloned();
                let (doc_count, db_size, last_index, db_root) = if let Some(db) = &db {
                    (db.indexed_files, cached_size, db.last_index_time.clone(), db.root_dir.clone())
                } else {
                    (0, 0, "—".into(), String::new())
                };
                stat_row(
                    ui,
                    &[
                        ("文档库", env.config.docs.databases.len().to_string(), theme::ACCENT),
                        ("已索引文档", super::truncate_path(&doc_count.to_string(), 24), theme::BLUE),
                        ("索引占用", format!("{:.1} MB", db_size as f64 / 1024.0 / 1024.0), theme::SUCCESS),
                        ("命中结果", self.hits.len().to_string(), theme::WARN),
                    ],
                );
                ui.add_space(10.0);

                section_card(ui, "文档库", Some("PDF / Word / Excel / PPT / 文本 全文索引，全部在本机完成"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let db_name = db.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "尚未创建文档库".into());
                        egui::ComboBox::from_id_salt("doc_db_select")
                            .selected_text(db_name)
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                let databases = env.config.docs.databases.clone();
                                for database in databases {
                                    if ui.selectable_value(&mut self.active_db, database.id.clone(), format!("{} · {}", database.name, database.indexed_files)).clicked() {
                                        self.hits.clear();
                                        env.config.docs.active_db = self.active_db.clone();
                                        env.mark_dirty();
                                    }
                                }
                            });
                        if widgets::ghost_button(ui, Some(widgets::ToolIcon::Plus), "新建文档库").clicked() {
                            self.show_add = true;
                        }
                        if let Some(db) = &db {
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "打开目录").clicked() {
                                util::open_path(Path::new(&db.root_dir));
                            }
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "移除").clicked() {
                                env.config.docs.databases.retain(|d| d.id != db.id);
                                if env.config.docs.active_db == db.id {
                                    self.active_db = env.config.docs.databases.first().map(|d| d.id.clone()).unwrap_or_default();
                                    env.config.docs.active_db = self.active_db.clone();
                                }
                                self.hits.clear();
                                env.mark_dirty();
                                env.toast("文档库已从列表移除（索引文件仍保留在配置目录）", crate::app::ToastKind::Info);
                            }
                        }
                    });
                    if let Some(db) = &db {
                        let db_path = super::path_actions(
                            ui.label(RichText::new(format!("{} · 目录：{}", if db.description.is_empty() { "无描述" } else { &db.description }, super::truncate_path(&db_root, 80))).size(11.0).color(theme::dim(ui))),
                            Path::new(&db_root),
                        );
                        db_path.on_hover_text("双击打开目录，右键更多操作");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let running = job_view(&self.build_job).0;
                            if running {
                                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Stop), "取消索引").clicked() {
                                    super::job_cancel(&self.build_job);
                                }
                            } else {
                                if widgets::primary_button(ui, Some(widgets::ToolIcon::Bolt), "重建索引").clicked() {
                                    self.start_build(ui.ctx(), env, BuildMode::Rebuild);
                                }
                                if widgets::ghost_button(ui, Some(widgets::ToolIcon::Refresh), "增量更新").clicked() {
                                    self.start_build(ui.ctx(), env, BuildMode::Update);
                                }
                            }
                            ui.add_space(6.0);
                            super::small_muted(ui, format!("上次索引：{last_index}"));
                            if !self.last_build_note.is_empty() {
                                widgets::pill(ui, &self.last_build_note, theme::SUCCESS);
                            }
                        });
                        progress_row(ui, &self.build_job);
                    } else {
                        widgets::empty_state(ui, widgets::ToolIcon::Document, "还没有文档库", "点击“新建文档库”，选择一个文件夹并建立全文索引。");
                    }
                });
                ui.add_space(8.0);

                if db.is_some() {
                    section_card(ui, "全文检索", Some("支持中文分词、短语与多词组合，结果显示相关片段"), |ui| {
                        let mut do_search = false;
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - 230.0).max(220.0);
                            let response = ui.add_sized(
                                [width, 32.0],
                                egui::TextEdit::singleline(&mut self.query)
                                    .hint_text("输入关键词，例如：项目 计划书、\"年度 报告\"、预算 -草稿")
                                    .margin(egui::Vec2::new(9.0, 7.0)),
                            );
                            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                do_search = true;
                            }
                            if widgets::primary_button(ui, Some(widgets::ToolIcon::Search), "检索").clicked() {
                                do_search = true;
                            }
                            if widgets::ghost_button(ui, Some(widgets::ToolIcon::Close), "清空").clicked() {
                                self.query.clear();
                                self.hits.clear();
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if super::toggle_chip(ui, env.config.docs.all_terms, "同时包含所有词") {
                                env.config.docs.all_terms = !env.config.docs.all_terms;
                                env.mark_dirty();
                            }
                            if super::toggle_chip(ui, env.config.docs.detailed_view, "显示详细片段") {
                                env.config.docs.detailed_view = !env.config.docs.detailed_view;
                                env.mark_dirty();
                            }
                            if super::toggle_chip(ui, env.config.docs.auto_update, "监控增量更新") {
                                env.config.docs.auto_update = !env.config.docs.auto_update;
                                env.mark_dirty();
                            }
                            ui.add_space(8.0);
                            super::small_muted(ui, "排序");
                            let current = DocSort::from_id(&env.config.docs.sort_by);
                            egui::ComboBox::from_id_salt("doc_sort")
                                .selected_text(current.label())
                                .width(96.0)
                                .show_ui(ui, |ui| {
                                    for sort in [
                                        DocSort::Relevance,
                                        DocSort::Frequency,
                                        DocSort::NameAsc,
                                        DocSort::NameDesc,
                                        DocSort::Newest,
                                        DocSort::Oldest,
                                    ] {
                                        if ui.selectable_label(current == sort, sort.label()).clicked() {
                                            env.config.docs.sort_by = sort.id().to_string();
                                            sort.apply(&mut self.hits);
                                            env.mark_dirty();
                                        }
                                    }
                                });
                            ui.add_space(8.0);
                            super::small_muted(ui, "最多结果");
                            ui.add(egui::DragValue::new(&mut env.config.docs.max_results).range(10..=2000).speed(10));
                        });
                        if !env.config.docs.history.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                super::small_muted(ui, "最近：");
                                let history = env.config.docs.history.clone();
                                for item in history.iter().take(6) {
                                    if ui.small_button(item).clicked() {
                                        self.query = item.clone();
                                        do_search = true;
                                    }
                                }
                            });
                        }
                        progress_row(ui, &self.search_job);
                        if do_search {
                            self.start_search(ui.ctx(), env);
                        }
                    });
                    ui.add_space(8.0);

                    section_card(ui, "检索结果", Some("双击用系统默认程序打开文档"), |ui| {
                        if self.hits.is_empty() {
                            widgets::empty_state(
                                ui,
                                widgets::ToolIcon::Document,
                                if job_view(&self.search_job).0 { "正在检索…" } else { "输入关键词开始全文检索" },
                                "索引覆盖文档正文、工作表单元格、演示文稿文本、PDF 页面与网页正文。",
                            );
                            return;
                        }
                        let detailed = env.config.docs.detailed_view;
                        let hits = &self.hits;
                        TableBuilder::new(ui)
                            .striped(true)
                            .sense(egui::Sense::click())
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(Column::initial(220.0).at_least(140.0).resizable(true).clip(true))
                            .column(Column::initial(110.0).at_least(80.0).clip(true))
                            .column(Column::remainder().at_least(250.0).clip(true))
                            .column(Column::initial(54.0).at_least(44.0).clip(true))
                            .column(Column::initial(70.0).at_least(50.0).clip(true))
                            .max_scroll_height(if detailed { 380.0 } else { 330.0 })
                            .header(24.0, |mut header| {
                                for title in ["文档", "位置", "匹配片段", "词频", "相关度"] {
                                    header.col(|ui| {
                                        ui.label(RichText::new(title).size(11.0).strong().color(theme::dim(ui)));
                                    });
                                }
                            })
                            .body(|body| {
                                let height = if detailed { 42.0 } else { 24.0 };
                                body.rows(height, hits.len(), |mut row| {
                                    let hit = &hits[row.index()];
                                    row.col(|ui| {
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                                            widgets::paint_icon(ui, rect, widgets::ToolIcon::Document, theme::ACCENT);
                                            let name = ui.add(
                                                egui::Label::new(RichText::new(&hit.name).size(12.0))
                                                    .truncate()
                                                    .sense(egui::Sense::click()),
                                            )
                                            .on_hover_text(&hit.name);
                                            super::path_actions(name, Path::new(&hit.path));
                                        });
                                    });
                                    row.col(|ui| {
                                        ui.add(egui::Label::new(RichText::new(if hit.part.is_empty() { "—" } else { &hit.part }).size(10.5).color(theme::dim(ui))).truncate());
                                    });
                                    row.col(|ui| {
                                        let snippet = hit.snippet.replace('\n', " ");
                                        if detailed {
                                            ui.vertical(|ui| {
                                                ui.add(egui::Label::new(RichText::new(super::truncate_path(&snippet, 150)).size(10.5).color(theme::dim(ui))).truncate())
                                                    .on_hover_text(hit.snippet.clone());
                                                ui.add(egui::Label::new(RichText::new(super::truncate_path(&hit.path, 90)).size(9.5).color(theme::TEXT_DIM.gamma_multiply(0.8))).truncate())
                                                    .on_hover_text(hit.path.clone());
                                            });
                                        } else {
                                            ui.add(egui::Label::new(RichText::new(super::truncate_path(&snippet, 80)).size(10.5).color(theme::dim(ui))).truncate())
                                                .on_hover_text(hit.snippet.clone());
                                        }
                                    });
                                    row.col(|ui| {
                                        let color = if hit.keyword_count > 0 { theme::WARN } else { theme::dim(ui) };
                                        ui.label(RichText::new(hit.keyword_count.to_string()).size(10.5).color(color));
                                    });
                                    row.col(|ui| {
                                        ui.label(RichText::new(format!("{:.1}", hit.score)).size(10.5).color(theme::BLUE));
                                    });
                                    super::path_actions(row.response(), Path::new(&hit.path));
                                });
                            });
                    });
                }
                ui.add_space(6.0);
            });

        if self.show_add {
            self.draw_add_window(ui.ctx(), env);
        }
    }

    fn draw_add_window(&mut self, ctx: &egui::Context, env: &mut TabEnv) {
        let mut open = true;
        let visuals = ctx.style_of(ctx.theme()).visuals.clone();
        egui::Window::new("新建文档库")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(
                egui::Frame::new()
                    .fill(visuals.window_fill)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(18.0)
                    .shadow(visuals.window_shadow),
            )
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new("名称").size(11.5).color(theme::dim(ui)));
                ui.add_sized([ui.available_width(), 28.0], egui::TextEdit::singleline(&mut self.new_name).hint_text("例如：项目资料库"));
                ui.add_space(6.0);
                ui.label(RichText::new("目录").size(11.5).color(theme::dim(ui)));
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 80.0).max(160.0);
                    ui.add_sized([width, 28.0], egui::TextEdit::singleline(&mut self.new_root).hint_text("包含文档的文件夹"));
                    if widgets::ghost_button(ui, Some(widgets::ToolIcon::Folder), "浏览").clicked() {
                        if let Some(path) = super::pick_folder() {
                            self.new_root = path.display().to_string();
                        }
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("描述（可选）").size(11.5).color(theme::dim(ui)));
                ui.add_sized([ui.available_width(), 28.0], egui::TextEdit::singleline(&mut self.new_desc).hint_text("文档库用途说明"));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if widgets::primary_button(ui, Some(widgets::ToolIcon::Plus), "创建").clicked() {
                        self.add_database(env);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::ghost_button(ui, None, "取消").clicked() {
                            self.show_add = false;
                        }
                    });
                });
            });
        if !open {
            self.show_add = false;
        }
    }
}
