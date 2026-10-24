use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::{DupMethod, HashChoice, ImageHashKind};
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanMode {
    Duplicates,
    SimilarImages,
    EmptyFiles,
    EmptyFolders,
    BigFiles,
    TempFiles,
    BrokenFiles,
    BadExtensions,
    InvalidSymlinks,
}

impl ScanMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Duplicates => "重复文件",
            Self::SimilarImages => "相似图片",
            Self::EmptyFiles => "空文件",
            Self::EmptyFolders => "空目录",
            Self::BigFiles => "大文件",
            Self::TempFiles => "临时文件",
            Self::BrokenFiles => "损坏文件",
            Self::BadExtensions => "错误扩展名",
            Self::InvalidSymlinks => "无效快捷方式",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub exclusions: Vec<String>,
    pub extensions: Vec<String>,
    pub include_hidden: bool,
    pub include_system: bool,
    pub min_size: u64,
    pub method: DupMethod,
    pub hash: HashChoice,
    pub image_threshold: u32,
    pub image_hash_size: u32,
    pub image_kind: ImageHashKind,
    pub ignore_same_resolution: bool,
    pub big_min_bytes: u64,
    pub temp_suffixes: Vec<String>,
    pub top_n: usize,
    pub mode: ScanMode,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            exclusions: Vec::new(),
            extensions: Vec::new(),
            include_hidden: false,
            include_system: false,
            min_size: 1024,
            method: DupMethod::Hash,
            hash: HashChoice::Blake3,
            image_threshold: 8,
            image_hash_size: 16,
            image_kind: ImageHashKind::Difference,
            ignore_same_resolution: false,
            big_min_bytes: 500 * 1024 * 1024,
            temp_suffixes: vec![".tmp".into(), ".temp".into(), ".bak".into()],
            top_n: 200,
            mode: ScanMode::Duplicates,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
}

#[derive(Clone, Debug)]
pub struct DupGroup {
    pub label: String,
    pub files: Vec<FileEntry>,
    pub wasted: u64,
}

#[derive(Clone, Debug)]
pub enum ScanOutcome {
    Groups(Vec<DupGroup>),
    Files(Vec<FileEntry>),
    Dirs(Vec<PathBuf>),
}

#[derive(Clone, Debug)]
pub struct ScanResult {
    pub outcome: ScanOutcome,
    pub scanned: u64,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub struct ScanProgress {
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct DeleteReport {
    pub deleted: u64,
    pub failed: u64,
    pub blocked: u64,
    pub errors: Vec<String>,
}

const SYSTEM_DIRS: [&str; 9] = [
    "windows",
    "programdata",
    "program files",
    "program files (x86)",
    "perflogs",
    "recovery",
    "$windows.~bt",
    "$windows.~ws",
    "system volume information",
];

const COMMON_SKIP: [&str; 7] = [".git", ".svn", "node_modules", "__pycache__", "$recycle.bin", ".idea", ".vscode"];

pub fn scan(
    opts: &ScanOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ScanProgress),
) -> ScanResult {
    let mut errors = Vec::new();
    let mut cancelled = false;
    let mut scanned = 0u64;
    let mut files: Vec<FileEntry> = Vec::new();
    let mut symlinks: Vec<FileEntry> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for root in &opts.roots {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        if !root.is_dir() {
            errors.push(format!("目录不存在: {}", root.display()));
            continue;
        }
        let mut it = WalkDir::new(root).follow_links(false).into_iter();
        loop {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let entry = match it.next() {
                Some(Ok(e)) => e,
                Some(Err(e)) => {
                    if errors.len() < 200 {
                        errors.push(format!("{e}"));
                    }
                    continue;
                }
                None => break,
            };
            if entry.depth() == 0 {
                if entry.file_type().is_dir() {
                    dirs.push(entry.path().to_path_buf());
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let is_dir = entry.file_type().is_dir();
            if is_dir {
                if COMMON_SKIP.contains(&name.as_str()) || SYSTEM_DIRS.contains(&name.as_str()) && !opts.include_system {
                    it.skip_current_dir();
                    continue;
                }
                if excluded(opts, entry.path(), &name) || (!opts.include_hidden && util::is_hidden_path(entry.path())) {
                    it.skip_current_dir();
                    continue;
                }
                dirs.push(entry.path().to_path_buf());
                continue;
            }
            if entry.file_type().is_symlink() {
                if opts.mode == ScanMode::InvalidSymlinks {
                    let broken = std::fs::metadata(entry.path()).is_err();
                    if broken {
                        symlinks.push(FileEntry { path: entry.path().to_path_buf(), size: 0, modified: 0 });
                    }
                }
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            if excluded(opts, entry.path(), &name) {
                continue;
            }
            if !opts.include_hidden && util::is_hidden_path(entry.path()) {
                continue;
            }
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !opts.extensions.is_empty() && !opts.extensions.contains(&ext) {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    if errors.len() < 200 {
                        errors.push(format!("{}: {e}", entry.path().display()));
                    }
                    continue;
                }
            };
            scanned += 1;
            files.push(FileEntry {
                path: entry.path().to_path_buf(),
                size: meta.len(),
                modified: meta.modified().ok().map(util::system_time_secs).unwrap_or(0),
            });
            if scanned % 512 == 0 {
                progress(ScanProgress {
                    stage: "扫描文件",
                    done: scanned,
                    total: 0,
                    message: entry.path().display().to_string(),
                });
            }
        }
    }

    let outcome = match opts.mode {
        ScanMode::Duplicates => {
            let list: Vec<FileEntry> = files
                .into_iter()
                .filter(|f| f.size >= opts.min_size)
                .collect();
            ScanOutcome::Groups(find_duplicates(&list, opts, cancel, progress, &mut errors))
        }
        ScanMode::SimilarImages => ScanOutcome::Groups(find_similar_images(&files, opts, cancel, progress, &mut errors)),
        ScanMode::EmptyFiles => {
            let mut list: Vec<FileEntry> = files.into_iter().filter(|f| f.size == 0).collect();
            list.sort_by(|a, b| a.path.cmp(&b.path));
            ScanOutcome::Files(list)
        }
        ScanMode::EmptyFolders => {
            let mut empty: Vec<PathBuf> = dirs
                .into_iter()
                .filter(|d| {
                    std::fs::read_dir(d)
                        .map(|mut it| it.next().is_none())
                        .unwrap_or(false)
                })
                .collect();
            empty.sort();
            ScanOutcome::Dirs(empty)
        }
        ScanMode::BigFiles => {
            let mut list: Vec<FileEntry> = files
                .into_iter()
                .filter(|f| f.size >= opts.big_min_bytes)
                .collect();
            list.sort_by(|a, b| b.size.cmp(&a.size));
            list.truncate(opts.top_n.max(1));
            ScanOutcome::Files(list)
        }
        ScanMode::TempFiles => {
            let mut list: Vec<FileEntry> = files
                .into_iter()
                .filter(|f| {
                    let name = f
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    name.starts_with('~')
                        || opts.temp_suffixes.iter().any(|s| name.ends_with(&s.to_lowercase()))
                })
                .collect();
            list.sort_by(|a, b| b.size.cmp(&a.size));
            list.truncate(opts.top_n.max(1));
            ScanOutcome::Files(list)
        }
        ScanMode::BrokenFiles => {
            let mut list = Vec::new();
            for (i, f) in files.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                if i % 64 == 0 {
                    progress(ScanProgress {
                        stage: "校验文件",
                        done: i as u64,
                        total: files.len() as u64,
                        message: f.path.display().to_string(),
                    });
                }
                if file_is_broken(&f.path) == Some(true) {
                    list.push(f.clone());
                }
            }
            ScanOutcome::Files(list)
        }
        ScanMode::BadExtensions => {
            let mut list = Vec::new();
            for f in &files {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                if extension_is_wrong(&f.path) {
                    list.push(f.clone());
                }
            }
            ScanOutcome::Files(list)
        }
        ScanMode::InvalidSymlinks => ScanOutcome::Files(symlinks),
    };
    ScanResult { outcome, scanned, errors, cancelled }
}

fn excluded(opts: &ScanOptions, path: &Path, name: &str) -> bool {
    opts.exclusions.iter().any(|e| {
        let e = e.trim().to_lowercase();
        if e.is_empty() {
            return false;
        }
        let e = e.replace('/', "\\");
        name == e
            || path
                .to_string_lossy()
                .to_lowercase()
                .replace('/', "\\")
                .contains(&e)
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum GroupKey {
    Name(String),
    Size(u64),
    SizeName(u64, String),
    Hash(Vec<u8>),
}

fn group_files<F>(files: &[FileEntry], key: F) -> HashMap<GroupKey, Vec<FileEntry>>
where
    F: Fn(&FileEntry) -> Option<GroupKey>,
{
    let mut map: HashMap<GroupKey, Vec<FileEntry>> = HashMap::new();
    for f in files {
        if let Some(k) = key(f) {
            map.entry(k).or_default().push(f.clone());
        }
    }
    map
}

fn finish_groups(mut map: HashMap<GroupKey, Vec<FileEntry>>, top_n: usize) -> Vec<DupGroup> {
    let mut groups: Vec<DupGroup> = map
        .drain()
        .filter(|(_, v)| v.len() > 1)
        .map(|(_, mut files)| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            let wasted = files.iter().skip(1).map(|f| f.size).sum();
            let label = files
                .first()
                .and_then(|f| f.path.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            DupGroup { label, files, wasted }
        })
        .collect();
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted).then(a.label.cmp(&b.label)));
    if top_n > 0 {
        groups.truncate(top_n);
    }
    groups
}

fn find_duplicates(
    files: &[FileEntry],
    opts: &ScanOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ScanProgress),
    errors: &mut Vec<String>,
) -> Vec<DupGroup> {
    match opts.method {
        DupMethod::Name => finish_groups(
            group_files(files, |f| {
                f.path
                    .file_name()
                    .map(|n| GroupKey::Name(n.to_string_lossy().to_lowercase()))
            }),
            opts.top_n,
        ),
        DupMethod::Size => finish_groups(group_files(files, |f| Some(GroupKey::Size(f.size))), opts.top_n),
        DupMethod::SizeName => finish_groups(
            group_files(files, |f| {
                f.path.file_name().map(|n| {
                    GroupKey::SizeName(f.size, n.to_string_lossy().to_lowercase())
                })
            }),
            opts.top_n,
        ),
        DupMethod::Hash => {
            let size_groups = group_files(files, |f| Some(GroupKey::Size(f.size)));
            let mut candidates: Vec<FileEntry> = Vec::new();
            for (_, group) in size_groups {
                if group.len() > 1 {
                    candidates.extend(group);
                }
            }
            if candidates.is_empty() {
                return Vec::new();
            }
            let mut prefix_map: HashMap<(u64, Vec<u8>), Vec<FileEntry>> = HashMap::new();
            for (i, f) in candidates.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                if i % 64 == 0 {
                    progress(ScanProgress {
                        stage: "计算预哈希",
                        done: i as u64,
                        total: candidates.len() as u64,
                        message: f.path.display().to_string(),
                    });
                }
                match hash_file(&f.path, opts.hash, Some(4096)) {
                    Ok(h) => prefix_map.entry((f.size, h)).or_default().push(f.clone()),
                    Err(e) => {
                        if errors.len() < 200 {
                            errors.push(format!("{}: {e}", f.path.display()));
                        }
                    }
                }
            }
            let mut full_candidates: Vec<FileEntry> = Vec::new();
            for (_, group) in prefix_map {
                if group.len() > 1 {
                    full_candidates.extend(group);
                }
            }
            let mut result: HashMap<GroupKey, Vec<FileEntry>> = HashMap::new();
            for (i, f) in full_candidates.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                if i % 16 == 0 {
                    progress(ScanProgress {
                        stage: "计算完整哈希",
                        done: i as u64,
                        total: full_candidates.len() as u64,
                        message: f.path.display().to_string(),
                    });
                }
                match hash_file(&f.path, opts.hash, None) {
                    Ok(h) => result.entry(GroupKey::Hash(h)).or_default().push(f.clone()),
                    Err(e) => {
                        if errors.len() < 200 {
                            errors.push(format!("{}: {e}", f.path.display()));
                        }
                    }
                }
            }
            finish_groups(result, opts.top_n)
        }
    }
}

enum HashCtx {
    Blake3(blake3::Hasher),
    Xxh3(Box<xxhash_rust::xxh3::Xxh3>),
    Crc32(crc32fast::Hasher),
}

impl HashCtx {
    fn new(choice: HashChoice) -> Self {
        match choice {
            HashChoice::Blake3 => Self::Blake3(blake3::Hasher::new()),
            HashChoice::Xxh3 => Self::Xxh3(Box::new(xxhash_rust::xxh3::Xxh3::new())),
            HashChoice::Crc32 => Self::Crc32(crc32fast::Hasher::new()),
        }
    }
    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Blake3(h) => {
                h.update(data);
            }
            Self::Xxh3(h) => h.update(data),
            Self::Crc32(h) => h.update(data),
        }
    }
    fn finish(self) -> Vec<u8> {
        match self {
            Self::Blake3(h) => h.finalize().as_bytes().to_vec(),
            Self::Xxh3(h) => h.digest().to_le_bytes().to_vec(),
            Self::Crc32(h) => h.finalize().to_le_bytes().to_vec(),
        }
    }
}

fn hash_file(path: &Path, choice: HashChoice, limit: Option<u64>) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut ctx = HashCtx::new(choice);
    let mut remaining = limit.unwrap_or(u64::MAX);
    let mut buf = [0u8; 256 * 1024];
    while remaining > 0 {
        let want = (buf.len() as u64).min(remaining) as usize;
        let n = reader.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
        remaining -= n as u64;
    }
    Ok(ctx.finish())
}

struct ImageHashEntry {
    file: FileEntry,
    width: u32,
    height: u32,
    bits: Vec<u64>,
}

fn find_similar_images(
    files: &[FileEntry],
    opts: &ScanOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ScanProgress),
    errors: &mut Vec<String>,
) -> Vec<DupGroup> {
    let candidates: Vec<&FileEntry> = files
        .iter()
        .filter(|f| {
            let ext = f
                .path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tif" | "tiff")
                && f.size <= 64 * 1024 * 1024
        })
        .collect();
    let mut images: Vec<ImageHashEntry> = Vec::new();
    for (i, f) in candidates.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(ScanProgress {
            stage: "计算图片指纹",
            done: i as u64,
            total: candidates.len() as u64,
            message: f.path.display().to_string(),
        });
        match image_hash(&f.path, opts.image_kind, opts.image_hash_size) {
            Ok((bits, width, height)) => images.push(ImageHashEntry { file: (*f).clone(), width, height, bits }),
            Err(e) => {
                if errors.len() < 200 {
                    errors.push(format!("{}: {e}", f.path.display()));
                }
            }
        }
    }
    let mut groups: Vec<Vec<ImageHashEntry>> = Vec::new();
    for img in images {
        let threshold = opts.image_threshold;
        let target = groups.iter().position(|group| {
            group.iter().any(|other| {
                if opts.ignore_same_resolution && other.width == img.width && other.height == img.height {
                    return false;
                }
                hamming(&other.bits, &img.bits) <= threshold
            })
        });
        match target {
            Some(index) => groups[index].push(img),
            None => groups.push(vec![img]),
        }
    }
    let mut out: Vec<DupGroup> = groups
        .into_iter()
        .filter(|g| g.len() > 1)
        .map(|mut g| {
            g.sort_by(|a, b| a.file.path.cmp(&b.file.path));
            let files: Vec<FileEntry> = g.iter().map(|e| e.file.clone()).collect();
            let wasted = files.iter().skip(1).map(|f| f.size).sum();
            let label = files
                .first()
                .and_then(|f| f.path.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            DupGroup { label, files, wasted }
        })
        .collect();
    out.sort_by(|a, b| b.wasted.cmp(&a.wasted).then(a.label.cmp(&b.label)));
    out.truncate(opts.top_n.max(1));
    out
}

fn hamming(a: &[u64], b: &[u64]) -> u32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x ^ y).count_ones()).sum()
}

fn pack_bits(bits: &[bool]) -> Vec<u64> {
    let mut out = vec![0u64; bits.len().div_ceil(64)];
    for (i, b) in bits.iter().enumerate() {
        if *b {
            out[i / 64] |= 1u64 << (i % 64);
        }
    }
    out
}

fn image_hash(path: &Path, kind: ImageHashKind, size: u32) -> Result<(Vec<u64>, u32, u32), String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let (width, height) = (img.width(), img.height());
    if width == 0 || height == 0 {
        return Err("图片尺寸无效".into());
    }
    let gray = img.to_luma8();
    let n = size.clamp(4, 64);
    let bits: Vec<bool> = match kind {
        ImageHashKind::Difference => {
            let resized = image::imageops::resize(&gray, n + 1, n, FilterType::Triangle);
            let mut bits = Vec::with_capacity((n * n) as usize);
            for y in 0..n {
                for x in 0..n {
                    bits.push(resized.get_pixel(x, y)[0] < resized.get_pixel(x + 1, y)[0]);
                }
            }
            bits
        }
        ImageHashKind::Average => {
            let resized = image::imageops::resize(&gray, n, n, FilterType::Triangle);
            let sum: u64 = resized.pixels().map(|p| p[0] as u64).sum();
            let mean = sum / (n * n) as u64;
            resized.pixels().map(|p| p[0] as u64 > mean).collect()
        }
        ImageHashKind::Perceptual => {
            let n = n.clamp(4, 16);
            let resized = image::imageops::resize(&gray, n * 2, n * 2, FilterType::Triangle);
            let mut matrix = vec![vec![0f64; (n * 2) as usize]; (n * 2) as usize];
            for y in 0..n * 2 {
                for x in 0..n * 2 {
                    matrix[y as usize][x as usize] = resized.get_pixel(x, y)[0] as f64;
                }
            }
            let dct = dct_2d(&matrix);
            let mut coeffs = Vec::new();
            for y in 0..n {
                for x in 0..n {
                    if x == 0 && y == 0 {
                        continue;
                    }
                    coeffs.push(dct[y as usize][x as usize]);
                }
            }
            let mut sorted = coeffs.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = sorted[sorted.len() / 2];
            coeffs.iter().map(|v| *v > median).collect()
        }
    };
    Ok((pack_bits(&bits), width, height))
}

fn dct_2d(matrix: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = matrix.len();
    let mut temp = vec![vec![0f64; n]; n];
    for (y, row) in matrix.iter().enumerate() {
        for u in 0..n {
            let mut sum = 0f64;
            for (x, value) in row.iter().enumerate() {
                sum += value * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / (2 * n) as f64).cos();
            }
            temp[y][u] = sum;
        }
    }
    let mut out = vec![vec![0f64; n]; n];
    for u in 0..n {
        for v in 0..n {
            let mut sum = 0f64;
            for y in 0..n {
                sum += temp[y][u]
                    * ((2 * y + 1) as f64 * v as f64 * std::f64::consts::PI / (2 * n) as f64).cos();
            }
            out[v][u] = sum;
        }
    }
    out
}

fn file_is_broken(path: &Path) -> Option<bool> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tif" | "tiff" => {
            Some(image::open(path).is_err())
        }
        "zip" | "docx" | "xlsx" | "pptx" | "jar" | "apk" | "epub" => {
            let file = File::open(path).ok()?;
            Some(zip::ZipArchive::new(file).is_err())
        }
        "pdf" => {
            let mut file = File::open(path).ok()?;
            let mut head = [0u8; 5];
            let n = file.read(&mut head).ok()?;
            Some(n < 5 || &head[..5] != b"%PDF-")
        }
        _ => None,
    }
}

fn extension_is_wrong(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext.is_empty()
        || matches!(
            ext.as_str(),
            "txt" | "md" | "csv" | "json" | "xml" | "log" | "rs" | "py" | "js" | "ts" | "toml"
                | "yaml" | "yml" | "ini" | "cfg" | "html" | "htm" | "sql" | "bat" | "sh" | "ps1"
                | "docx" | "xlsx" | "pptx" | "jar" | "apk" | "epub"
        )
    {
        return false;
    }
    let Ok(Some(kind)) = infer::get_from_path(path) else {
        return false;
    };
    let actual = kind.extension().to_ascii_lowercase();
    let ok = match actual.as_str() {
        "jpg" => matches!(ext.as_str(), "jpg" | "jpeg"),
        "tif" => matches!(ext.as_str(), "tif" | "tiff"),
        "htm" => matches!(ext.as_str(), "htm" | "html"),
        "zip" => matches!(ext.as_str(), "zip" | "docx" | "xlsx" | "pptx" | "jar" | "apk" | "epub"),
        _ => actual == ext,
    };
    !ok
}

pub fn delete_paths(paths: &[PathBuf], move_to_trash: bool) -> DeleteReport {
    let mut report = DeleteReport::default();
    for path in paths {
        if util::deletion_blocked(path) {
            report.blocked += 1;
            if report.errors.len() < 50 {
                report.errors.push(format!("受保护路径已跳过: {}", path.display()));
            }
            continue;
        }
        if path.symlink_metadata().is_err() {
            continue;
        }
        let result = if move_to_trash {
            trash::delete(path).map_err(|e| e.to_string())
        } else if path.is_dir() && !path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            std::fs::remove_dir_all(path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(path).map_err(|e| e.to_string())
        };
        match result {
            Ok(()) => report.deleted += 1,
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 50 {
                    report.errors.push(format!("{}: {e}", path.display()));
                }
            }
        }
    }
    report
}

pub fn export_groups_csv(path: &Path, groups: &[DupGroup]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "\u{feff}分组,文件路径,大小(字节),修改时间")?;
    for (i, group) in groups.iter().enumerate() {
        for f in &group.files {
            writeln!(
                file,
                "{},{},{},{}",
                i + 1,
                csv_escape(&f.path.display().to_string()),
                f.size,
                util::format_time(std::time::UNIX_EPOCH + std::time::Duration::from_secs(f.modified.max(0) as u64))
            )?;
        }
    }
    Ok(())
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedScan {
    pub mode: String,
    pub roots: Vec<String>,
    pub groups: Vec<SavedGroup>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedGroup {
    pub label: String,
    pub paths: Vec<String>,
}

pub fn save_scan(path: &Path, result: &ScanResult, roots: &[PathBuf], mode: ScanMode) -> std::io::Result<()> {
    let groups = match &result.outcome {
        ScanOutcome::Groups(groups) => groups
            .iter()
            .map(|g| SavedGroup {
                label: g.label.clone(),
                paths: g.files.iter().map(|f| f.path.display().to_string()).collect(),
            })
            .collect(),
        ScanOutcome::Files(files) => vec![SavedGroup {
            label: mode.label().into(),
            paths: files.iter().map(|f| f.path.display().to_string()).collect(),
        }],
        ScanOutcome::Dirs(dirs) => vec![SavedGroup {
            label: mode.label().into(),
            paths: dirs.iter().map(|p| p.display().to_string()).collect(),
        }],
    };
    let saved = SavedScan {
        mode: mode.label().into(),
        roots: roots.iter().map(|p| p.display().to_string()).collect(),
        groups,
    };
    let text = serde_json::to_string_pretty(&saved)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fts_{name}_{}", std::process::id()))
    }

    fn sync_progress(_: ScanProgress) {}

    #[test]
    fn finds_hash_duplicates_and_deletes() {
        let root = temp("dup");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("a/one.txt"), "same-content-1234567890").unwrap();
        fs::write(root.join("b/two.txt"), "same-content-1234567890").unwrap();
        fs::write(root.join("b/three.bin"), "different").unwrap();
        let opts = ScanOptions {
            roots: vec![root.clone()],
            min_size: 1,
            method: DupMethod::Hash,
            ..Default::default()
        };
        let result = scan(&opts, &AtomicBool::new(false), &mut sync_progress);
        match &result.outcome {
            ScanOutcome::Groups(groups) => {
                assert_eq!(groups.len(), 1);
                assert_eq!(groups[0].files.len(), 2);
                assert!(groups[0].wasted > 0);
                let to_delete = vec![groups[0].files[0].path.clone()];
                let report = delete_paths(&to_delete, false);
                assert_eq!(report.deleted, 1);
                assert!(report.failed == 0);
                assert!(!to_delete[0].exists());
            }
            _ => panic!("expected groups"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_empty_files_and_folders() {
        let root = temp("empty");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("empty_dir")).unwrap();
        fs::write(root.join("zero.txt"), "").unwrap();
        fs::write(root.join("data.txt"), "abc").unwrap();
        let opts = ScanOptions { roots: vec![root.clone()], ..Default::default() };
        let result = scan(&opts, &AtomicBool::new(false), &mut sync_progress);
        assert!(matches!(result.outcome, ScanOutcome::Groups(_)));
        let opts = ScanOptions { roots: vec![root.clone()], mode: ScanMode::EmptyFiles, ..Default::default() };
        let result = scan(&opts, &AtomicBool::new(false), &mut sync_progress);
        match result.outcome {
            ScanOutcome::Files(files) => assert_eq!(files.len(), 1),
            _ => panic!(),
        }
        let opts = ScanOptions { roots: vec![root.clone()], mode: ScanMode::EmptyFolders, ..Default::default() };
        let result = scan(&opts, &AtomicBool::new(false), &mut sync_progress);
        match result.outcome {
            ScanOutcome::Dirs(dirs) => {
                assert!(dirs.iter().any(|d| d.ends_with("empty_dir")));
                assert!(!dirs.iter().any(|d| d == &root));
            }
            _ => panic!(),
        }
        #[cfg(windows)]
        assert!(util::deletion_blocked(std::path::Path::new(r"C:\Windows")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn groups_similar_images() {
        let root = temp("imgs");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut a = image::RgbImage::new(32, 32);
        for (x, y, p) in a.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 4) as u8, (y * 4) as u8, 80]);
        }
        a.save(root.join("a.png")).unwrap();
        a.save(root.join("b.png")).unwrap();
        let mut c = image::RgbImage::new(32, 32);
        for p in c.pixels_mut() {
            *p = image::Rgb([255, 0, 0]);
        }
        c.save(root.join("c.png")).unwrap();
        let opts = ScanOptions {
            roots: vec![root.clone()],
            mode: ScanMode::SimilarImages,
            image_threshold: 2,
            image_hash_size: 16,
            ..Default::default()
        };
        let result = scan(&opts, &AtomicBool::new(false), &mut sync_progress);
        match result.outcome {
            ScanOutcome::Groups(groups) => {
                assert_eq!(groups.len(), 1, "groups={groups:?}");
                assert_eq!(groups[0].files.len(), 2);
            }
            _ => panic!(),
        }
        let _ = fs::remove_dir_all(&root);
    }
}
