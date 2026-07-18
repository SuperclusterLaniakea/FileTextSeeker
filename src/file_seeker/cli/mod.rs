/// CLI (ES) - Command Line Interface for Everything search
///
/// Usage: es.exe [options] [search text]

use crate::file_seeker::engine::Engine;
use crate::file_seeker::types::{SearchOptions, SortSpec, SortField, SortOrder, ExportFormat, SizeFormat, DateFormat};
use std::sync::Arc;

/// Command line options for the ES tool
#[derive(Debug)]
pub struct CliOptions {
    /// Search text
    pub search_text: String,
    /// Regex search
    pub regex: bool,
    /// Match case
    pub match_case: bool,
    /// Match whole word
    pub match_whole_word: bool,
    /// Match full path
    pub match_path: bool,
    /// Match diacritics
    pub match_diacritics: bool,
    /// Sort specification
    pub sort: Option<SortSpec>,
    /// Sort ascending
    pub sort_ascending: bool,
    /// Sort descending
    pub sort_descending: bool,
    /// Max results
    pub max_results: usize,
    /// Offset
    pub offset: usize,
    /// Show only files
    pub files_only: bool,
    /// Show only folders
    pub folders_only: bool,
    /// Export format
    pub export_format: Option<ExportFormat>,
    /// Export filename
    pub export_filename: Option<String>,
    /// Size format
    pub size_format: SizeFormat,
    /// Date format
    pub date_format: DateFormat,
    /// Highlight results
    pub highlight: bool,
    /// Highlight color
    pub highlight_color: u8,
    /// Filename color
    pub filename_color: Option<u8>,
    /// Path color
    pub path_color: Option<u8>,
    /// Show columns
    pub columns: Vec<String>,
    /// Instance name
    pub instance: Option<String>,
    /// Timeout in ms
    pub timeout: u64,
    /// Pause after each page
    pub pause: bool,
    /// Hide empty search results
    pub hide_empty: bool,
    /// Show help
    pub help: bool,
    /// Save settings
    pub save_settings: bool,
    /// Clear settings
    pub clear_settings: bool,
    /// Set run count
    pub set_run_count: Option<(String, u64)>,
    /// Increment run count
    pub inc_run_count: Option<String>,
    /// Get run count
    pub get_run_count: Option<String>,
    /// Get result count
    pub get_result_count: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            search_text: String::new(),
            regex: false,
            match_case: false,
            match_whole_word: false,
            match_path: false,
            match_diacritics: false,
            sort: None,
            sort_ascending: false,
            sort_descending: false,
            max_results: 0,
            offset: 0,
            files_only: false,
            folders_only: false,
            export_format: None,
            export_filename: None,
            size_format: SizeFormat::Auto,
            date_format: DateFormat::System,
            highlight: false,
            highlight_color: 0x0a,
            filename_color: None,
            path_color: None,
            columns: Vec::new(),
            instance: None,
            timeout: 0,
            pause: false,
            hide_empty: false,
            help: false,
            save_settings: false,
            clear_settings: false,
            set_run_count: None,
            inc_run_count: None,
            get_run_count: None,
            get_result_count: false,
        }
    }
}

/// Parse command line arguments into CliOptions
pub fn parse_args(args: &[String]) -> Result<(CliOptions, Vec<String>), String> {
    let mut opts = CliOptions::default();
    let mut extra = Vec::new();
    let mut i = 1; // Skip program name

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-r" | "-regex" => opts.regex = true,
            "-i" | "-case" => opts.match_case = true,
            "-w" | "-whole-word" => opts.match_whole_word = true,
            "-ww" => opts.match_whole_word = true,
            "-p" | "-match-path" => opts.match_path = true,
            "-a" | "-diacritics" => opts.match_diacritics = true,
            "-h" | "-help" => opts.help = true,
            "-s" => opts.match_path = true, // in CLI context, -s means sort by path
            "-hide-empty-search-results" => opts.hide_empty = true,
            "-empty-search-help" => opts.help = true,
            "-highlight" => opts.highlight = true,
            "-csv" => opts.export_format = Some(ExportFormat::Csv),
            "-efu" => opts.export_format = Some(ExportFormat::Efu),
            "-txt" => opts.export_format = Some(ExportFormat::Txt),
            "-m3u" => opts.export_format = Some(ExportFormat::M3u),
            "-m3u8" => opts.export_format = Some(ExportFormat::M3u8),
            "-save-settings" => opts.save_settings = true,
            "-clear-settings" => opts.clear_settings = true,
            "-get-result-count" => opts.get_result_count = true,
            "-pause" | "-more" => opts.pause = true,
            "-sort-ascending" => opts.sort_ascending = true,
            "-sort-descending" => opts.sort_descending = true,
            "-n" | "-max-results" => {
                i += 1;
                if i < args.len() {
                    opts.max_results = args[i].parse().unwrap_or(0);
                }
            }
            "-o" | "-offset" => {
                i += 1;
                if i < args.len() {
                    opts.offset = args[i].parse().unwrap_or(0);
                }
            }
            "-sort" => {
                i += 1;
                if i < args.len() {
                    opts.sort = crate::file_seeker::engine::sorter::parse_sort_name(&args[i]);
                }
            }
            "-instance" => {
                i += 1;
                if i < args.len() {
                    opts.instance = Some(args[i].clone());
                }
            }
            "-timeout" => {
                i += 1;
                if i < args.len() {
                    opts.timeout = args[i].parse().unwrap_or(0);
                }
            }
            "-highlight-color" => {
                i += 1;
                if i < args.len() {
                    opts.highlight_color = parse_hex_color(&args[i]);
                }
            }
            "-filename-color" | "-name-color" => {
                i += 1;
                if i < args.len() {
                    opts.filename_color = Some(parse_hex_color(&args[i]));
                }
            }
            "-path-color" => {
                i += 1;
                if i < args.len() {
                    opts.path_color = Some(parse_hex_color(&args[i]));
                }
            }
            "-set-run-count" => {
                i += 1;
                let filename = if i < args.len() { args[i].clone() } else { String::new() };
                i += 1;
                let count = if i < args.len() { args[i].parse().unwrap_or(0) } else { 0 };
                opts.set_run_count = Some((filename, count));
            }
            "-inc-run-count" => {
                i += 1;
                if i < args.len() {
                    opts.inc_run_count = Some(args[i].clone());
                }
            }
            "-get-run-count" => {
                i += 1;
                if i < args.len() {
                    opts.get_run_count = Some(args[i].clone());
                }
            }
            "-name" | "-path-column" | "-size" | "-extension" | "-ext"
            | "-date-created" | "-dc" | "-date-modified" | "-dm"
            | "-date-accessed" | "-da" | "-attributes" | "-attribs" | "-attrib"
            | "-file-list-file-name" | "-run-count" | "-date-run"
            | "-date-recently-changed" | "-rc" => {
                opts.columns.push(arg[1..].to_string());
            }
            _ => {
                extra.push(arg.clone());
            }
        }
        i += 1;
    }

    Ok((opts, extra))
}

/// Run ES CLI with the given options
pub fn run_cli(opts: &CliOptions, engine: &Arc<Engine>) -> Result<i32, String> {
    if opts.help {
        print_help();
        return Ok(0);
    }

    // Handle run count operations
    if let Some((filename, count)) = &opts.set_run_count {
        engine.set_run_count(filename, *count)?;
        return Ok(0);
    }

    if let Some(filename) = &opts.inc_run_count {
        engine.increment_run_count(filename)?;
        return Ok(0);
    }

    if let Some(filename) = &opts.get_run_count {
        let count = engine.get_run_count(filename).unwrap_or(0);
        println!("{}", count);
        return Ok(0);
    }

    // Build search options
    let mut search_opts = SearchOptions {
        regex: opts.regex,
        match_case: opts.match_case,
        match_whole_word: opts.match_whole_word,
        match_path: opts.match_path,
        match_diacritics: opts.match_diacritics,
        max_results: opts.max_results,
        offset: opts.offset,
        files_only: opts.files_only,
        folders_only: opts.folders_only,
        sort: opts.sort.unwrap_or(SortSpec::default()),
    };

    // Handle sort direction
    if opts.sort_descending {
        search_opts.sort.order = SortOrder::Descending;
    } else if opts.sort_ascending {
        search_opts.sort.order = SortOrder::Ascending;
    }

    let search_text = &opts.search_text;

    // Check if we should hide empty results
    if opts.hide_empty && search_text.is_empty() {
        return Ok(0);
    }

    // Search
    let results = engine.search(search_text, &search_opts)?;

    // Get result count if requested
    if opts.get_result_count {
        println!("{}", results.len());
        return Ok(0);
    }

    // Print results
    if results.is_empty() {
        println!("No results found.");
        return Ok(0);
    }

    for entry in &results {
        if opts.highlight {
            let highlighted = crate::file_seeker::engine::searcher::SearchEngine::highlight_text(
                &entry.full_path.to_string_lossy(),
                search_text
            );
            println!("{}", highlighted);
        } else {
            println!("{}", entry.full_path.display());
        }
    }

    Ok(0)
}

fn parse_hex_color(s: &str) -> u8 {
    let s = s.trim_start_matches("0x");
    u8::from_str_radix(s, 16).unwrap_or(0)
}

pub fn print_help() {
    println!("Everything ES (文件检索助手CLI)");
    println!("Usage: es.exe [options] [search text]");
    println!();
    println!("Options:");
    println!("  -r, -regex <search>       Search using regular expressions");
    println!("  -i, -case                 Match case");
    println!("  -w, -ww, -whole-word      Match whole words");
    println!("  -p, -match-path           Match full path and file name");
    println!("  -h, -help                 Display this help");
    println!("  -n, -max-results <num>    Limit number of results");
    println!("  -o, -offset <num>         Show results starting from offset");
    println!("  -sort <field>             Sort by field (name, path, size, etc.)");
    println!("  -sort-ascending           Sort ascending");
    println!("  -sort-descending          Sort descending");
    println!("  -highlight                Highlight search terms");
    println!("  -highlight-color <color>  Set highlight color");
    println!("  -csv                      CSV output format");
    println!("  -efu                      EFU output format");
    println!("  -txt                      TXT output format");
    println!("  -instance <name>          Connect to named instance");
    println!("  -save-settings            Save current settings");
    println!("  -clear-settings           Clear saved settings");
    println!("  -get-result-count         Show result count only");
    println!("  -set-run-count <file> <n> Set run count for file");
    println!("  -inc-run-count <file>     Increment run count");
    println!("  -get-run-count <file>     Get run count for file");
}

