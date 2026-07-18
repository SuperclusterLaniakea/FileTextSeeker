/// Sorting engine - sorts file entries by various criteria

use crate::file_seeker::types::{FileEntry, SortSpec, SortField, SortOrder};

/// Sort entries according to the given sort specification
pub fn sort_entries(entries: &mut [FileEntry], sort: &SortSpec) {
    match sort.field {
        SortField::Name => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));
            } else {
                entries.sort_by(|a, b| b.file_name.to_lowercase().cmp(&a.file_name.to_lowercase()));
            }
        }
        SortField::Path => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.full_path.cmp(&b.full_path));
            } else {
                entries.sort_by(|a, b| b.full_path.cmp(&a.full_path));
            }
        }
        SortField::Size => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.size.cmp(&b.size));
            } else {
                entries.sort_by(|a, b| b.size.cmp(&a.size));
            }
        }
        SortField::Extension => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.extension.to_lowercase().cmp(&b.extension.to_lowercase()));
            } else {
                entries.sort_by(|a, b| b.extension.to_lowercase().cmp(&a.extension.to_lowercase()));
            }
        }
        SortField::DateCreated => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.date_created.cmp(&b.date_created));
            } else {
                entries.sort_by(|a, b| b.date_created.cmp(&a.date_created));
            }
        }
        SortField::DateModified => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.date_modified.cmp(&b.date_modified));
            } else {
                entries.sort_by(|a, b| b.date_modified.cmp(&a.date_modified));
            }
        }
        SortField::DateAccessed => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.date_accessed.cmp(&b.date_accessed));
            } else {
                entries.sort_by(|a, b| b.date_accessed.cmp(&a.date_accessed));
            }
        }
        SortField::Attributes => {
            entries.sort_by(|a, b| {
                let a_val = attr_to_u64(&a.attributes);
                let b_val = attr_to_u64(&b.attributes);
                if sort.order == SortOrder::Ascending { a_val.cmp(&b_val) } else { b_val.cmp(&a_val) }
            });
        }
        SortField::FileListFileName => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.file_list_filename.cmp(&b.file_list_filename));
            } else {
                entries.sort_by(|a, b| b.file_list_filename.cmp(&a.file_list_filename));
            }
        }
        SortField::RunCount => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.run_count.cmp(&b.run_count));
            } else {
                entries.sort_by(|a, b| b.run_count.cmp(&a.run_count));
            }
        }
        SortField::DateRecentlyChanged => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.date_recently_changed.cmp(&b.date_recently_changed));
            } else {
                entries.sort_by(|a, b| b.date_recently_changed.cmp(&a.date_recently_changed));
            }
        }
        SortField::DateRun => {
            if sort.order == SortOrder::Ascending {
                entries.sort_by(|a, b| a.date_run.cmp(&b.date_run));
            } else {
                entries.sort_by(|a, b| b.date_run.cmp(&a.date_run));
            }
        }
    }
}

/// Parse a DIR-style sort string (e.g., /oN, /o-S, /oD)
pub fn parse_dir_sort(sort_str: &str) -> Option<SortSpec> {
    let chars: Vec<char> = sort_str.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let (field_char, order) = if chars[0] == '-' {
        if chars.len() < 2 { return None; }
        (chars[1], SortOrder::Descending)
    } else {
        (chars[0], SortOrder::Ascending)
    };

    let field = match field_char {
        'N' => SortField::Name,
        'S' => SortField::Size,
        'E' => SortField::Extension,
        'D' => SortField::DateModified,
        _ => return None,
    };

    Some(SortSpec { field, order })
}

/// Parse a sort name string (e.g., "name", "size", "date-modified")
pub fn parse_sort_name(name: &str) -> Option<SortSpec> {
    let (name, order) = if name.ends_with("-ascending") {
        (&name[..name.len() - 10], SortOrder::Ascending)
    } else if name.ends_with("-descending") {
        (&name[..name.len() - 11], SortOrder::Descending)
    } else {
        (name, SortOrder::Ascending)
    };

    let field = match name.to_lowercase().as_str() {
        "name" => SortField::Name,
        "path" => SortField::Path,
        "size" => SortField::Size,
        "extension" | "ext" => SortField::Extension,
        "date-created" | "dc" => SortField::DateCreated,
        "date-modified" | "dm" => SortField::DateModified,
        "date-accessed" | "da" => SortField::DateAccessed,
        "attributes" | "attribs" | "attrib" => SortField::Attributes,
        "file-list-file-name" => SortField::FileListFileName,
        "run-count" => SortField::RunCount,
        "date-recently-changed" | "rc" => SortField::DateRecentlyChanged,
        "date-run" => SortField::DateRun,
        _ => return None,
    };

    Some(SortSpec { field, order })
}

fn attr_to_u64(attr: &crate::file_seeker::types::FileAttributes) -> u64 {
    let mut val = 0u64;
    if attr.read_only { val |= 1; }
    if attr.hidden { val |= 2; }
    if attr.system { val |= 4; }
    if attr.directory { val |= 16; }
    if attr.archive { val |= 32; }
    if attr.device { val |= 64; }
    if attr.normal { val |= 128; }
    if attr.temporary { val |= 256; }
    if attr.sparse { val |= 512; }
    if attr.reparse_point { val |= 1024; }
    if attr.compressed { val |= 2048; }
    if attr.offline { val |= 4096; }
    if attr.not_content_indexed { val |= 8192; }
    if attr.encrypted { val |= 16384; }
    val
}

