use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub theme: ThemeChoice,
    pub font_scale: f32,
    pub close_to_tray: bool,
    pub autostart: bool,
    pub start_minimized: bool,
    pub monitoring: bool,
    pub low_memory: bool,
    pub show_memory: bool,
    pub active_tab: String,
    pub search: SearchSettings,
    pub docs: DocsSettings,
    pub duplicates: DuplicatesSettings,
    pub rename: RenameSettings,
    pub sync: SyncSettings,
    pub merge: MergeSettings,
    pub lister: ListerSettings,
    pub cleaner: CleanerSettings,
    pub recent_folders: Vec<String>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        let mut cfg: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if cfg.font_scale <= 0.0 {
            cfg.font_scale = 1.0;
        }
        if cfg.active_tab.is_empty() {
            cfg.active_tab = "search".into();
        }
        cfg
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    pub roots: Vec<String>,
    pub excluded_dirs: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub include_hidden: bool,
    pub include_system_dirs: bool,
    pub max_results: usize,
    pub match_case: bool,
    pub match_path: bool,
    pub whole_word: bool,
    pub regex_mode: bool,
    pub sort_field: String,
    pub sort_asc: bool,
    pub exact_name: bool,
    pub index_file: String,
    pub last_query: String,
    pub history: Vec<String>,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            excluded_dirs: vec![
                "$Recycle.Bin".into(),
                "System Volume Information".into(),
                "node_modules".into(),
                ".git".into(),
            ],
            exclude_patterns: vec!["*.tmp".into(), "*.log".into(), "*.bak".into()],
            include_hidden: false,
            include_system_dirs: false,
            max_results: 5000,
            match_case: false,
            match_path: true,
            whole_word: false,
            regex_mode: false,
            sort_field: "name".into(),
            sort_asc: true,
            exact_name: false,
            index_file: String::new(),
            last_query: String::new(),
            history: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DocsSettings {
    pub databases: Vec<DocDatabase>,
    pub active_db: String,
    pub pdf_reader: String,
    pub ignore_case: bool,
    pub all_terms: bool,
    pub detailed_view: bool,
    pub history: Vec<String>,
    pub max_results: usize,
    pub auto_update: bool,
    pub sort_by: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocDatabase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub root_dir: String,
    pub created_at: String,
    pub last_index_time: String,
    pub indexed_files: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DupMethod {
    #[default]
    Hash,
    Name,
    Size,
    SizeName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HashChoice {
    #[default]
    Blake3,
    Xxh3,
    Crc32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImageHashKind {
    #[default]
    Difference,
    Average,
    Perceptual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DuplicatesSettings {
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
    pub extensions: String,
    pub include_hidden: bool,
    pub include_system: bool,
    pub min_size_bytes: u64,
    pub method: DupMethod,
    pub hash: HashChoice,
    pub image_threshold: u32,
    pub image_hash_size: u32,
    pub image_kind: ImageHashKind,
    pub ignore_same_resolution: bool,
    pub top_n: usize,
    pub big_min_mb: u64,
    pub temp_suffixes: String,
    pub move_to_trash: bool,
    pub mode: String,
}

impl Default for DuplicatesSettings {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            exclusions: Vec::new(),
            extensions: String::new(),
            include_hidden: false,
            include_system: false,
            min_size_bytes: 1024 * 1024,
            method: DupMethod::Hash,
            hash: HashChoice::Blake3,
            image_threshold: 8,
            image_hash_size: 16,
            image_kind: ImageHashKind::Difference,
            ignore_same_resolution: false,
            top_n: 200,
            big_min_mb: 500,
            temp_suffixes: ".tmp,.temp,.bak,~,.crdownload,.part,.cache,.dmp,.download,.partial,thumbs.db"
                .into(),
            move_to_trash: true,
            mode: "duplicates".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenameMode {
    #[default]
    Rename,
    Copy,
    Move,
}

impl RenameMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rename => "原地重命名",
            Self::Copy => "复制到新目录",
            Self::Move => "移动到新目录",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollisionRule {
    #[default]
    AutoNumber,
    Skip,
    Overwrite,
}

impl CollisionRule {
    pub fn label(self) -> &'static str {
        match self {
            Self::AutoNumber => "自动编号",
            Self::Skip => "跳过冲突",
            Self::Overwrite => "覆盖已有文件",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RenameSettings {
    pub last_dir: String,
    pub add_recursive: bool,
    pub mode: RenameMode,
    pub target_dir: String,
    pub collision: CollisionRule,
    pub preserve_structure: bool,
    pub extensions_filter: String,
}

impl Default for RenameSettings {
    fn default() -> Self {
        Self {
            last_dir: String::new(),
            add_recursive: true,
            mode: RenameMode::Rename,
            target_dir: String::new(),
            collision: CollisionRule::AutoNumber,
            preserve_structure: true,
            extensions_filter: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SyncModeChoice {
    #[default]
    Mirror,
    Update,
    Twoway,
    Custom,
}

impl SyncModeChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mirror => "镜像",
            Self::Update => "更新",
            Self::Twoway => "双向",
            Self::Custom => "自定义",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConflictChoice {
    #[default]
    Newer,
    Skip,
    Left,
    Right,
}

impl ConflictChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Newer => "较新者胜出",
            Self::Skip => "跳过冲突",
            Self::Left => "保留左侧",
            Self::Right => "保留右侧",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    pub left: String,
    pub right: String,
    pub mode: SyncModeChoice,
    pub conflict: ConflictChoice,
    pub include_patterns: String,
    pub exclude_patterns: String,
    pub regex_exclude: String,
    pub min_size_kb: u64,
    pub max_size_mb: u64,
    pub exclude_hidden: bool,
    pub buffer_mb: usize,
    pub concurrent: usize,
    pub unbuffered: bool,
    pub verify: bool,
    pub follow_symlinks: bool,
    pub preserve_timestamps: bool,
    pub auto_retry: u8,
    pub dry_run: bool,
    pub recent_jobs: Vec<String>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            left: String::new(),
            right: String::new(),
            mode: SyncModeChoice::Mirror,
            conflict: ConflictChoice::Newer,
            include_patterns: String::new(),
            exclude_patterns: "target/**,node_modules/**,.git/**".into(),
            regex_exclude: String::new(),
            min_size_kb: 0,
            max_size_mb: 0,
            exclude_hidden: false,
            buffer_mb: 2,
            concurrent: 4,
            unbuffered: false,
            verify: false,
            follow_symlinks: false,
            preserve_timestamps: true,
            auto_retry: 0,
            dry_run: false,
            recent_jobs: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MergeFormat {
    #[default]
    Markdown,
    Plain,
}

impl MergeFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Plain => "纯文本",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MergeSettings {
    pub dir: String,
    pub extensions: String,
    pub excludes: String,
    pub format: MergeFormat,
    pub include_tree: bool,
    pub include_headers: bool,
    pub include_line_numbers: bool,
    pub max_file_kb: u64,
    pub last_save_dir: String,
}

impl Default for MergeSettings {
    fn default() -> Self {
        Self {
            dir: String::new(),
            extensions: "rs,c,cpp,h,hpp,cxx,cc,hh,py,java,go,js,ts,jsx,tsx,rb,php,swift,kt,cs,fs,lua,r,m,mm,pl,pm,sh,bash,zsh,ps1,bat,cmd,toml,env,ini,cfg,conf,yaml,yml,json,xml,properties,lock,txt,md,sql,html,css,scss,vue,svelte"
                .into(),
            excludes: "node_modules,target,.git,dist,build,.idea,.vscode,__pycache__".into(),
            format: MergeFormat::Markdown,
            include_tree: true,
            include_headers: true,
            include_line_numbers: false,
            max_file_kb: 2048,
            last_save_dir: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ListerFormat {
    #[default]
    Xlsx,
    Csv,
}

impl ListerFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Xlsx => "Excel (.xlsx)",
            Self::Csv => "CSV (.csv)",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ListerSettings {
    pub source: String,
    pub save_dir: String,
    pub recursive: bool,
    pub format: ListerFormat,
    pub selected_extensions: Vec<String>,
    pub filter_text: String,
}

impl Default for ListerSettings {
    fn default() -> Self {
        Self {
            source: String::new(),
            save_dir: String::new(),
            recursive: false,
            format: ListerFormat::Xlsx,
            selected_extensions: Vec::new(),
            filter_text: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CleanKind {
    #[default]
    Dir,
    File,
    Glob,
}

impl CleanKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dir => "目录内容",
            Self::File => "单个文件",
            Self::Glob => "通配符匹配",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomCleanRule {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: CleanKind,
    pub pattern: String,
    pub note: String,
    pub enabled: bool,
    pub dangerous: bool,
}

impl Default for CustomCleanRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            path: String::new(),
            kind: CleanKind::Dir,
            pattern: "*.tmp".into(),
            note: String::new(),
            enabled: true,
            dangerous: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CleanerSettings {
    pub disabled_builtin: Vec<String>,
    pub custom_rules: Vec<CustomCleanRule>,
    pub move_to_trash: bool,
    pub create_restore_point: bool,
    pub schedule_enabled: bool,
    pub schedule_time: String,
    pub schedule_recycle: bool,
    pub last_run: String,
    pub history: Vec<String>,
}

impl Default for CleanerSettings {
    fn default() -> Self {
        Self {
            disabled_builtin: Vec::new(),
            custom_rules: Vec::new(),
            move_to_trash: true,
            create_restore_point: false,
            schedule_enabled: false,
            schedule_time: "03:00".into(),
            schedule_recycle: true,
            last_run: String::new(),
            history: Vec::new(),
        }
    }
}

pub struct ConfigStore {
    pub dir: PathBuf,
    pub path: PathBuf,
}

impl ConfigStore {
    pub fn new(dir_override: Option<PathBuf>) -> Self {
        let dir = dir_override.unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("ToolsNest")
        });
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        Self { dir, path }
    }

    pub fn load(&self) -> AppConfig {
        let mut cfg = AppConfig::load(&self.path);
        if cfg.search.index_file.is_empty() {
            cfg.search.index_file = self.dir.join("search_index.db").display().to_string();
        } else {
            let path = PathBuf::from(&cfg.search.index_file);
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                cfg.search.index_file = path.with_extension("db").display().to_string();
            }
        }
        if cfg.docs.max_results == 0 {
            cfg.docs.max_results = 200;
        }
        cfg
    }

    pub fn save(&self, cfg: &AppConfig) {
        if let Err(e) = cfg.save(&self.path) {
            eprintln!("保存配置失败: {e}");
        }
    }

    pub fn docs_dir(&self) -> PathBuf {
        self.dir.join("docdb")
    }

    pub fn rename_log_path(&self) -> PathBuf {
        self.dir.join("rename_undo.json")
    }

    pub fn log_path(&self) -> PathBuf {
        self.dir.join("app.log")
    }
}

pub fn autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
            return key.get_value::<String, _>("ToolsNest").is_ok();
        }
    }
    false
}

pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")?;
        if enabled {
            let exe = std::env::current_exe()?;
            key.set_value("ToolsNest", &format!("\"{}\" --minimized", exe.display()))?;
        } else {
            let _ = key.delete_value("ToolsNest");
        }
    }
    Ok(())
}
