use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::util;

#[derive(Clone, Debug)]
pub struct TextUnit {
    pub label: String,
    pub text: String,
}

pub const MAX_FILE_BYTES: u64 = 96 * 1024 * 1024;

pub fn supported_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf" | "docx" | "pptx" | "xlsx" | "xls" | "ods" | "odt" | "txt" | "md" | "markdown"
            | "csv" | "tsv" | "json" | "xml" | "html" | "htm" | "log" | "rtf" | "ini" | "cfg"
            | "yaml" | "yml" | "toml" | "rs" | "py" | "js" | "ts" | "java" | "c" | "h" | "cpp"
            | "hpp" | "go" | "sql" | "sh" | "ps1" | "bat"
    )
}

pub fn extract_file(path: &Path) -> Result<Vec<TextUnit>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件信息失败: {e}"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("文件超过 96 MB，已跳过".into());
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let units = match ext.as_str() {
        "pdf" => extract_pdf(path)?,
        "docx" => extract_docx(path)?,
        "pptx" => extract_pptx(path)?,
        "xlsx" | "xls" | "ods" => extract_sheet(path)?,
        "odt" => extract_odt(path)?,
        "html" | "htm" | "xml" => {
            let text = util::decode_text(&std::fs::read(path).map_err(|e| e.to_string())?);
            vec![TextUnit { label: "全文".into(), text: strip_tags(&text) }]
        }
        _ => {
            let text = util::decode_text(&std::fs::read(path).map_err(|e| e.to_string())?);
            vec![TextUnit { label: "全文".into(), text }]
        }
    };
    let units: Vec<TextUnit> = units
        .into_iter()
        .filter(|u| !u.text.trim().is_empty())
        .map(|mut u| {
            if u.text.len() > 8 * 1024 * 1024 {
                u.text.truncate(8 * 1024 * 1024);
            }
            u
        })
        .collect();
    Ok(units)
}

fn extract_pdf(path: &Path) -> Result<Vec<TextUnit>, String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text(path)
    }));
    match result {
        Ok(Ok(text)) => {
            let pages: Vec<&str> = text.split('\u{000c}').collect();
            let multi = pages.len() > 1;
            Ok(pages
                .into_iter()
                .enumerate()
                .map(|(i, page)| TextUnit {
                    label: if multi { format!("第 {} 页", i + 1) } else { "全文".into() },
                    text: page.to_string(),
                })
                .collect())
        }
        Ok(Err(e)) => Err(format!("PDF 解析失败: {e}")),
        Err(_) => Err("PDF 解析器在处理该文件时出错".into()),
    }
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(file).map_err(|e| format!("压缩包读取失败: {e}"))
}

fn extract_docx(path: &Path) -> Result<Vec<TextUnit>, String> {
    let mut archive = open_zip(path)?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| "DOCX 中缺少 word/document.xml".to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    Ok(vec![TextUnit { label: "全文".into(), text: text_from_xml(&xml, b"t") }])
}

fn extract_pptx(path: &Path) -> Result<Vec<TextUnit>, String> {
    let mut archive = open_zip(path)?;
    let mut slides: Vec<(u32, String)> = Vec::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i).map(|f| f.name().to_string()).unwrap_or_default();
        if let Some(rest) = name.strip_prefix("ppt/slides/slide") {
            if let Some(num) = rest.strip_suffix(".xml").and_then(|n| n.parse::<u32>().ok()) {
                let mut xml = String::new();
                archive
                    .by_index(i)
                    .map_err(|e| e.to_string())?
                    .read_to_string(&mut xml)
                    .map_err(|e| e.to_string())?;
                slides.push((num, text_from_xml(&xml, b"t")));
            }
        }
    }
    slides.sort_by_key(|(n, _)| *n);
    Ok(slides
        .into_iter()
        .map(|(n, text)| TextUnit { label: format!("幻灯片 {n}"), text })
        .collect())
}

fn extract_odt(path: &Path) -> Result<Vec<TextUnit>, String> {
    let mut archive = open_zip(path)?;
    let mut xml = String::new();
    archive
        .by_name("content.xml")
        .map_err(|_| "ODT 中缺少 content.xml".to_string())?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    Ok(vec![TextUnit { label: "全文".into(), text: text_from_xml(&xml, b"p") }])
}

fn extract_sheet(path: &Path) -> Result<Vec<TextUnit>, String> {
    use calamine::{Data, Reader as _};
    let mut workbook = calamine::open_workbook_auto(path).map_err(|e| format!("表格读取失败: {e}"))?;
    let names = workbook.sheet_names().to_vec();
    let mut units = Vec::new();
    for name in names {
        let range = match workbook.worksheet_range(&name) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut text = String::new();
        for row in range.rows() {
            let mut line = String::new();
            for cell in row {
                if matches!(cell, Data::Empty) {
                    continue;
                }
                if !line.is_empty() {
                    line.push('\t');
                }
                line.push_str(&cell.to_string());
            }
            if !line.is_empty() {
                text.push_str(&line);
                text.push('\n');
            }
        }
        units.push(TextUnit { label: format!("工作表 {name}"), text });
    }
    Ok(units)
}

/// Extract visible text from Office XML while preserving paragraph/line breaks.
fn text_from_xml(xml: &str, text_tag: &[u8]) -> String {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name();
                let local = name.as_ref();
                if local == text_tag {
                    in_text = true;
                } else if local == b"tab" {
                    out.push('\t');
                } else if local == b"br" {
                    out.push('\n');
                }
            }
            Ok(Event::End(e)) => {
                let name = e.local_name();
                let local = name.as_ref();
                if local == text_tag {
                    in_text = false;
                } else if local == b"p" {
                    out.push('\n');
                }
            }
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Ok(decoded) = e.unescape() {
                        out.push_str(&decoded);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out
}

fn strip_tags(input: &str) -> String {
    static RES: std::sync::OnceLock<[regex::Regex; 3]> = std::sync::OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap(),
            regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap(),
            regex::Regex::new(r"(?is)<[^>]+>").unwrap(),
        ]
    });
    let without_scripts = res[0].replace_all(input, " ");
    let without_styles = res[1].replace_all(&without_scripts, " ");
    let text = res[2].replace_all(&without_styles, " ");
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(if ch == '\n' { '\n' } else { ' ' });
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_text_and_strips_html() {
        let dir = std::env::temp_dir().join(format!("fts_extract_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let txt = dir.join("note.txt");
        std::fs::write(&txt, "第一个文件\n中文内容 test").unwrap();
        let units = extract_file(&txt).unwrap();
        assert_eq!(units.len(), 1);
        assert!(units[0].text.contains("中文内容"));

        let html = dir.join("page.html");
        std::fs::write(&html, "<html><style>x{}</style><body><h1>标题</h1><p>正文内容</p></body></html>").unwrap();
        let units = extract_file(&html).unwrap();
        assert!(units[0].text.contains("标题"));
        assert!(units[0].text.contains("正文内容"));
        assert!(!units[0].text.contains("style"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_docx_text() {
        let dir = std::env::temp_dir().join(format!("fts_docx_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.docx");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        use std::io::Write as _;
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(r#"<?xml version="1.0"?><w:document xmlns:w="w"><w:body><w:p><w:r><w:t>你好</w:t></w:r><w:r><w:t>世界</w:t></w:r></w:p><w:p><w:r><w:t>第二段</w:t></w:r></w:p></w:body></w:document>"#.as_bytes()).unwrap();
        zip.finish().unwrap();
        let units = extract_file(&path).unwrap();
        assert!(units[0].text.contains("你好世界"));
        assert!(units[0].text.contains("第二段"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
