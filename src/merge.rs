use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::MergeFormat;
use crate::util;

#[derive(Clone, Debug)]
pub struct MergeOptions {
    pub dir: PathBuf,
    pub extensions: Vec<String>,
    pub excludes: Vec<String>,
    pub format: MergeFormat,
    pub include_tree: bool,
    pub include_headers: bool,
    pub include_line_numbers: bool,
    pub max_file_kb: u64,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            extensions: vec!["rs".into(), "py".into(), "md".into()],
            excludes: vec!["node_modules".into(), "target".into(), ".git".into()],
            format: MergeFormat::Markdown,
            include_tree: true,
            include_headers: true,
            include_line_numbers: false,
            max_file_kb: 2048,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MergeFile {
    pub path: PathBuf,
    pub rel: String,
    pub size: u64,
    pub content: String,
    pub lines: usize,
    pub language: String,
}

#[derive(Clone, Debug)]
pub struct SkippedFile {
    pub rel: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default)]
pub struct MergeOutput {
    pub files: usize,
    pub bytes: u64,
    pub lines: usize,
    pub text: String,
    pub skipped: Vec<SkippedFile>,
    pub errors: Vec<String>,
}

pub fn parse_list(raw: &str) -> Vec<String> {
    raw.split([',', ';', '\n', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_start_matches('.').to_lowercase())
        .collect()
}

pub fn language_for(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "rs" => "rust",
        "c" | "h" => "c",
        "cpp" | "cxx" | "cc" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "fs" => "fsharp",
        "py" => "python",
        "java" => "java",
        "go" => "go",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "jsx" => "jsx",
        "tsx" => "tsx",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "r" => "r",
        "m" => "objectivec",
        "mm" => "objectivecpp",
        "pl" | "pm" => "perl",
        "sh" | "bash" | "zsh" => "bash",
        "ps1" => "powershell",
        "bat" | "cmd" => "batchfile",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "vue" => "vue",
        "svelte" => "svelte",
        "sql" => "sql",
        "md" | "markdown" => "markdown",
        "ini" | "cfg" | "conf" | "env" | "properties" => "ini",
        "dockerfile" => "dockerfile",
        _ => "text",
    }
}

fn excluded(rel: &Path, excludes: &[String]) -> bool {
    let rel_text = rel.to_string_lossy().replace('\\', "/").to_lowercase();
    excludes.iter().any(|rule| {
        let rule = rule.trim().to_lowercase();
        if rule.is_empty() {
            return false;
        }
        if let Ok(pattern) = glob::Pattern::new(&rule) {
            if pattern.matches(&rel_text) {
                return true;
            }
        }
        rel.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(&rule))
    })
}

pub fn collect(opts: &MergeOptions) -> Result<Vec<MergeFile>, String> {
    if opts.dir.as_os_str().is_empty() {
        return Err("请选择要合并的代码目录".into());
    }
    if !opts.dir.is_dir() {
        return Err("目录不存在或不是文件夹".into());
    }
    let extensions = if opts.extensions.is_empty() {
        Vec::new()
    } else {
        opts.extensions.clone()
    };
    let mut files: Vec<MergeFile> = Vec::new();
    for entry in WalkDir::new(&opts.dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(rel_path) = path.strip_prefix(&opts.dir) else { continue };
        if excluded(rel_path, &opts.excludes) {
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !extensions.is_empty() && !extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                return Err(format!("读取 {} 失败: {e}", path.display()));
            }
        };
        if opts.max_file_kb > 0 && meta.len() > opts.max_file_kb * 1024 {
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                return Err(format!("读取 {} 失败: {e}", path.display()));
            }
        };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            continue;
        }
        let content = util::decode_text(&bytes).replace("\r\n", "\n");
        let language = language_for(&ext).to_string();
        let rel = rel_path.to_string_lossy().replace('\\', "/");
        files.push(MergeFile {
            path: path.to_path_buf(),
            rel,
            size: meta.len(),
            lines: content.lines().count(),
            content,
            language,
        });
    }
    files.sort_by(|a, b| a.rel.to_lowercase().cmp(&b.rel.to_lowercase()));
    Ok(files)
}

#[derive(Default)]
struct TreeNode {
    dirs: BTreeMap<String, TreeNode>,
    files: Vec<String>,
}

fn build_tree(files: &[MergeFile]) -> TreeNode {
    let mut root = TreeNode::default();
    for file in files {
        let parts: Vec<&str> = file.rel.split('/').collect();
        let mut node = &mut root;
        for part in &parts[..parts.len().saturating_sub(1)] {
            node = node.dirs.entry((*part).to_string()).or_default();
        }
        if let Some(name) = parts.last() {
            node.files.push((*name).to_string());
        }
    }
    root
}

fn render_tree(node: &TreeNode, prefix: &str, out: &mut String) {
    let dir_count = node.dirs.len();
    let file_count = node.files.len();
    let total = dir_count + file_count;
    let mut index = 0;
    for (name, child) in &node.dirs {
        index += 1;
        let last = index == total;
        out.push_str(prefix);
        out.push_str(if last { "└── " } else { "├── " });
        out.push_str(name);
        out.push('/');
        out.push('\n');
        let next_prefix = format!("{prefix}{}", if last { "    " } else { "│   " });
        render_tree(child, &next_prefix, out);
    }
    for (i, name) in node.files.iter().enumerate() {
        let last = index + i + 1 == total;
        out.push_str(prefix);
        out.push_str(if last { "└── " } else { "├── " });
        out.push_str(name);
        out.push('\n');
    }
}

pub fn generate(opts: &MergeOptions, mut files: Vec<MergeFile>, mut skipped: Vec<SkippedFile>) -> MergeOutput {
    let mut output = MergeOutput::default();
    let max_bytes = opts.max_file_kb.saturating_mul(1024);
    files.retain(|f| {
        if max_bytes > 0 && f.size > max_bytes {
            skipped.push(SkippedFile { rel: f.rel.clone(), reason: "文件超过大小限制".into() });
            false
        } else {
            true
        }
    });
    let root_name = opts
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| opts.dir.display().to_string());
    let mut text = String::new();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let total_lines: usize = files.iter().map(|f| f.lines).sum();

    if opts.format == MergeFormat::Markdown {
        text.push_str(&format!("# {root_name} · 代码合并\n\n"));
        text.push_str(&format!(
            "> 生成于 {now} · {} 个文件 · {} · {} 行\n\n",
            files.len(),
            util::format_size(total_bytes),
            total_lines
        ));
        if opts.include_tree {
            let tree = build_tree(&files);
            text.push_str("## 目录结构\n\n```text\n");
            text.push_str(&root_name);
            text.push_str("/\n");
            render_tree(&tree, "", &mut text);
            text.push_str("```\n\n");
        }
        for file in &files {
            if opts.include_headers {
                text.push_str(&format!("## {}\n\n", file.rel));
                text.push_str(&format!(
                    "`{}` · {} · {} 行\n\n",
                    file.language,
                    util::format_size(file.size),
                    file.lines
                ));
            }
            text.push_str(&format!("```{}\n", file.language));
            if opts.include_line_numbers {
                let width = file.lines.max(1).to_string().len();
                for (i, line) in file.content.lines().enumerate() {
                    text.push_str(&format!("{:>width$} | {line}\n", i + 1, width = width));
                }
            } else {
                text.push_str(&file.content);
                if !file.content.ends_with('\n') {
                    text.push('\n');
                }
            }
            text.push_str("```\n\n");
        }
    } else {
        text.push_str(&format!("{root_name} · 代码合并\n"));
        text.push_str(&format!(
            "生成于 {now} · {} 个文件 · {} · {} 行\n\n",
            files.len(),
            util::format_size(total_bytes),
            total_lines
        ));
        if opts.include_tree {
            let tree = build_tree(&files);
            text.push_str("目录结构:\n");
            text.push_str(&root_name);
            text.push_str("/\n");
            render_tree(&tree, "", &mut text);
            text.push('\n');
        }
        for file in &files {
            text.push_str(&"=".repeat(78));
            text.push('\n');
            if opts.include_headers {
                text.push_str(&format!("文件: {}  ({} 行, {})\n", file.rel, file.lines, util::format_size(file.size)));
                text.push_str(&"=".repeat(78));
                text.push('\n');
            }
            if opts.include_line_numbers {
                let width = file.lines.max(1).to_string().len();
                for (i, line) in file.content.lines().enumerate() {
                    text.push_str(&format!("{:>width$} | {line}\n", i + 1, width = width));
                }
            } else {
                text.push_str(&file.content);
                if !file.content.ends_with('\n') {
                    text.push('\n');
                }
            }
            text.push('\n');
        }
    }

    output.files = files.len();
    output.bytes = total_bytes;
    output.lines = total_lines;
    output.text = text;
    output.skipped = skipped;
    output
}

pub fn merge(opts: &MergeOptions) -> Result<MergeOutput, String> {
    let files = collect(opts)?;
    if files.is_empty() {
        return Err("在所选目录中没有找到符合扩展名的文本代码文件".into());
    }
    Ok(generate(opts, files, Vec::new()))
}

pub fn save(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| format!("保存失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fts_merge_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("src")).unwrap();
        path
    }

    #[test]
    fn collects_filters_and_generates_markdown() {
        let root = temp("md");
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("src/app.py"), "print('hi')\n").unwrap();
        fs::write(root.join("src/readme.txt"), "ignore\n").unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/generated.rs"), "ignored\n").unwrap();
        let opts = MergeOptions {
            dir: root.clone(),
            extensions: vec!["rs".into(), "py".into()],
            ..Default::default()
        };
        let output = merge(&opts).unwrap();
        assert_eq!(output.files, 2, "{}", output.text);
        assert!(output.text.contains("```rust"));
        assert!(output.text.contains("```python"));
        assert!(output.text.contains("src/main.rs"));
        assert!(!output.text.contains("generated.rs"));
        assert!(output.text.contains("目录结构"));
        assert!(output.skipped.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_output_with_line_numbers_and_headers() {
        let root = temp("plain");
        fs::write(root.join("src/a.rs"), "line one\nline two\n").unwrap();
        let opts = MergeOptions {
            dir: root.clone(),
            extensions: vec!["rs".into()],
            format: MergeFormat::Plain,
            include_line_numbers: true,
            ..Default::default()
        };
        let output = merge(&opts).unwrap();
        assert!(output.text.contains("1 | line one"));
        assert!(output.text.contains("2 | line two"));
        assert!(output.text.contains("文件: src/a.rs"));
        let path = root.join("out.txt");
        save(&path, &output.text).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("line two"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn oversized_files_are_reported() {
        let root = temp("big");
        fs::write(root.join("src/big.rs"), "x".repeat(4096)).unwrap();
        fs::write(root.join("src/small.rs"), "fn small() {}").unwrap();
        let opts = MergeOptions {
            dir: root.clone(),
            extensions: vec!["rs".into()],
            max_file_kb: 1,
            ..Default::default()
        };
        let files = collect(&opts).unwrap();
        assert_eq!(files.len(), 1);
        let output = generate(
            &opts,
            files,
            vec![SkippedFile { rel: "src/big.rs".into(), reason: "文件超过大小限制".into() }],
        );
        assert_eq!(output.skipped.len(), 1);
        assert!(!output.text.contains("big.rs\n"));
        let _ = fs::remove_dir_all(&root);
    }
}
