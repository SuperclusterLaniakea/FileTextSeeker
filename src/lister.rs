use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use walkdir::WalkDir;

use crate::util;

#[derive(Clone, Debug)]
pub struct ListerOptions {
    pub source: PathBuf,
    pub recursive: bool,
    pub extensions: Vec<String>,
    pub filter: String,
}

impl Default for ListerOptions {
    fn default() -> Self {
        Self {
            source: PathBuf::new(),
            recursive: false,
            extensions: Vec::new(),
            filter: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileRow {
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
}

#[derive(Clone, Debug, Default)]
pub struct ListerResult {
    pub rows: Vec<FileRow>,
    pub errors: Vec<String>,
    pub truncated: bool,
    pub cancelled: bool,
    pub total_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct ListerProgress {
    pub done: u64,
    pub message: String,
}

const MAX_ROWS: usize = 200_000;

pub fn parse_extensions(raw: &str) -> Vec<String> {
    raw.split([',', ';', ' ', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_start_matches('.').to_lowercase())
        .collect()
}

pub fn scan(opts: &ListerOptions, cancel: &AtomicBool, progress: &mut dyn FnMut(ListerProgress)) -> ListerResult {
    let mut result = ListerResult::default();
    if opts.source.as_os_str().is_empty() {
        result.errors.push("请选择要扫描的文件夹".into());
        return result;
    }
    if !opts.source.is_dir() {
        result.errors.push("所选路径不是文件夹".into());
        return result;
    }
    let walker = if opts.recursive {
        WalkDir::new(&opts.source).follow_links(false).sort_by_file_name()
    } else {
        WalkDir::new(&opts.source).min_depth(1).max_depth(1).follow_links(false).sort_by_file_name()
    };
    let filter = opts.filter.trim().to_lowercase();
    let mut visited = 0u64;
    for entry in walker.into_iter().filter_map(Result::ok) {
        if cancel.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !opts.extensions.is_empty()
            && !opts.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext))
        {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                if result.errors.len() < 100 {
                    result.errors.push(format!("{}: {e}", path.display()));
                }
                continue;
            }
        };
        result.total_bytes += meta.len();
        result.rows.push(FileRow {
            name,
            extension: ext,
            size: meta.len(),
            path: path.to_path_buf(),
            modified: meta.modified().ok(),
            created: meta.created().ok(),
            accessed: meta.accessed().ok(),
        });
        visited += 1;
        if visited % 1000 == 0 {
            progress(ListerProgress { done: visited, message: path.display().to_string() });
        }
        if result.rows.len() >= MAX_ROWS {
            result.truncated = true;
            break;
        }
    }
    result
}

pub const HEADERS: [&str; 9] = [
    "序号",
    "文件名",
    "扩展名",
    "大小(字节)",
    "大小",
    "完整路径",
    "创建时间",
    "修改时间",
    "访问时间",
];

fn time_text(time: Option<SystemTime>) -> String {
    time.map(util::format_time).unwrap_or_else(|| "-".into())
}

pub fn export_csv(path: &Path, rows: &[FileRow]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| format!("创建文件失败: {e}"))?;
    file.write_all(&[0xEF, 0xBB, 0xBF]).map_err(|e| e.to_string())?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .terminator(csv::Terminator::CRLF)
        .from_writer(file);
    writer.write_record(HEADERS).map_err(|e| e.to_string())?;
    for (index, row) in rows.iter().enumerate() {
        let record = [
            (index + 1).to_string(),
            row.name.clone(),
            row.extension.clone(),
            row.size.to_string(),
            util::format_size(row.size),
            row.path.display().to_string(),
            time_text(row.created),
            time_text(row.modified),
            time_text(row.accessed),
        ];
        writer.write_record(record).map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn export_xlsx(path: &Path, rows: &[FileRow]) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("文件清单").map_err(|e| e.to_string())?;
    let header_format = Format::new().set_bold();
    for (col, header) in HEADERS.iter().enumerate() {
        worksheet
            .write_string_with_format(0, col as u16, *header, &header_format)
            .map_err(|e| e.to_string())?;
    }
    for (index, row) in rows.iter().enumerate() {
        let r = (index + 1) as u32;
        worksheet.write_number(r, 0, (index + 1) as f64).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 1, &row.name).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 2, &row.extension).map_err(|e| e.to_string())?;
        worksheet.write_number(r, 3, row.size as f64).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 4, util::format_size(row.size)).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 5, row.path.display().to_string()).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 6, time_text(row.created)).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 7, time_text(row.modified)).map_err(|e| e.to_string())?;
        worksheet.write_string(r, 8, time_text(row.accessed)).map_err(|e| e.to_string())?;
    }
    let widths = [7.0, 36.0, 10.0, 14.0, 12.0, 70.0, 20.0, 20.0, 20.0];
    for (col, width) in widths.iter().enumerate() {
        worksheet.set_column_width(col as u16, *width).map_err(|e| e.to_string())?;
    }
    workbook.save(path).map_err(|e| format!("保存 Excel 失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fts_list_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("sub")).unwrap();
        path
    }

    #[test]
    fn scans_with_recursion_extension_and_name_filter() {
        let root = temp("scan");
        fs::write(root.join("alpha.txt"), "a").unwrap();
        fs::write(root.join("beta.rs"), "b").unwrap();
        fs::write(root.join("sub/gamma.txt"), "c").unwrap();
        let opts = ListerOptions { source: root.clone(), recursive: false, ..Default::default() };
        let result = scan(&opts, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(result.rows.len(), 2);
        assert!(result.rows.iter().all(|r| !r.name.contains("gamma")));

        let opts = ListerOptions {
            source: root.clone(),
            recursive: true,
            extensions: vec!["txt".into()],
            filter: "gamma".into(),
        };
        let result = scan(&opts, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].name, "gamma.txt");
        assert_eq!(result.total_bytes, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn exports_csv_with_bom_and_chinese_headers() {
        let root = temp("csv");
        fs::write(root.join("报告.txt"), "hello").unwrap();
        let opts = ListerOptions { source: root.clone(), ..Default::default() };
        let result = scan(&opts, &AtomicBool::new(false), &mut |_| {});
        let path = root.join("out.csv");
        export_csv(&path, &result.rows).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = util::decode_text(&bytes);
        assert!(text.contains("文件名"));
        assert!(text.contains("报告.txt"));
        assert!(text.contains("完整路径"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn exports_xlsx() {
        let root = temp("xlsx");
        fs::write(root.join("data.json"), "{}").unwrap();
        let opts = ListerOptions { source: root.clone(), ..Default::default() };
        let result = scan(&opts, &AtomicBool::new(false), &mut |_| {});
        let path = root.join("out.xlsx");
        export_xlsx(&path, &result.rows).unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert!(meta.len() > 1000);
        let _ = fs::remove_dir_all(&root);
    }
}
