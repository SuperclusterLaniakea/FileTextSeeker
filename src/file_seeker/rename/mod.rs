/// Multi-file rename functionality - batch rename, copy to, move to

use std::path::{Path, PathBuf};

/// Rename operation type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenameOperation {
    Rename,
    CopyTo,
    MoveTo,
}

/// Rename rule
#[derive(Debug, Clone)]
pub struct RenameRule {
    /// Search and replace mode
    pub find_text: String,
    pub replace_text: String,
    /// Counter format (e.g., " (1)", "_{n}")
    pub counter_format: Option<String>,
    /// Counter start value
    pub counter_start: u32,
    /// Change extension
    pub new_extension: Option<String>,
    /// Prefix to add
    pub prefix: Option<String>,
    /// Suffix to add
    pub suffix: Option<String>,
}

/// Multi-file renamer
pub struct Renamer {
    pub files: Vec<PathBuf>,
    pub operation: RenameOperation,
    pub target_directory: Option<PathBuf>,
    pub rules: Vec<RenameRule>,
}

impl Renamer {
    pub fn new(operation: RenameOperation) -> Self {
        Self {
            files: Vec::new(),
            operation,
            target_directory: None,
            rules: Vec::new(),
        }
    }

    /// Add files to rename
    pub fn add_files(&mut self, files: Vec<PathBuf>) {
        self.files = files;
    }

    /// Add a rename rule
    pub fn add_rule(&mut self, rule: RenameRule) {
        self.rules.push(rule);
    }

    /// Preview the new filenames
    pub fn preview(&self) -> Vec<(PathBuf, PathBuf)> {
        let mut results = Vec::new();
        let mut counter = 0u32;

        for file in &self.files {
            counter += 1;
            let new_name = self.apply_rules(file, counter);
            let new_path = match &self.target_directory {
                Some(dir) => dir.join(&new_name),
                None => file.parent().unwrap_or(Path::new(".")).join(&new_name),
            };
            results.push((file.clone(), new_path));
        }

        results
    }

    /// Execute the rename operation
    pub fn execute(&self) -> Result<Vec<(PathBuf, PathBuf)>, String> {
        let previews = self.preview();
        let mut completed = Vec::new();

        for (old_path, new_path) in &previews {
            match self.operation {
                RenameOperation::Rename => {
                    std::fs::rename(old_path, new_path)
                        .map_err(|e| format!("Failed to rename {}: {}", old_path.display(), e))?;
                }
                RenameOperation::CopyTo => {
                    if old_path.is_dir() {
                        copy_dir_recursive(old_path, new_path)?;
                    } else {
                        std::fs::copy(old_path, new_path)
                            .map_err(|e| format!("Failed to copy {}: {}", old_path.display(), e))?;
                    }
                }
                RenameOperation::MoveTo => {
                    std::fs::rename(old_path, new_path)
                        .map_err(|e| format!("Failed to move {}: {}", old_path.display(), e))?;
                }
            }
            completed.push((old_path.clone(), new_path.clone()));
        }

        Ok(completed)
    }

    /// Apply rename rules to a single file
    fn apply_rules(&self, file: &Path, counter: u32) -> String {
        let filename = file.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = file.extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut result = filename;

        for rule in &self.rules {
            // Find and replace
            if !rule.find_text.is_empty() {
                result = result.replace(&rule.find_text, &rule.replace_text);
            }

            // Prefix
            if let Some(prefix) = &rule.prefix {
                result = format!("{}{}", prefix, result);
            }

            // Suffix
            if let Some(suffix) = &rule.suffix {
                result = format!("{}{}", result, suffix);
            }

            // Counter
            if let Some(counter_fmt) = &rule.counter_format {
                let counter_str = counter_fmt
                    .replace("{n}", &(counter + rule.counter_start - 1).to_string())
                    .replace("{N}", &format!("{:03}", counter + rule.counter_start - 1));
                // Find the last occurrence of the counter format marker
                if !counter_str.contains('{') {
                    result.push_str(&counter_str);
                }
            }
        }

        // New extension
        let final_ext = self.rules.iter()
            .find_map(|r| r.new_extension.clone())
            .unwrap_or(ext);

        if final_ext.is_empty() {
            result
        } else {
            format!("{}.{}", result, final_ext)
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create directory {}: {}", dst.display(), e))?;

    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("Failed to read directory {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let file_type = entry.file_type()
            .map_err(|e| format!("Failed to get file type: {}", e))?;

        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {}: {}", src_path.display(), e))?;
        }
    }

    Ok(())
}

