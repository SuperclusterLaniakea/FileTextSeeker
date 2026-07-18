# fix.ps1 —— 自动修复路径引用、重复定义、生命周期等问题
$ErrorActionPreference = "Stop"
Write-Host ">>> 开始批量修复..." -ForegroundColor Cyan

# -------------------------------
# 1. 修复 src/main.rs 的生命周期错误
# -------------------------------
$main = Get-Content -Path "src\main.rs" -Raw
$main = $main -replace 'let start_minimized = .*;', ''
$main = $main -replace 'Box::new\(\|cc\| Box::new\(MergedApp::new\(cc, start_minimized\)\)\)', '{
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    Box::new(MergedApp::new(cc, start_minimized))
}'
$main = $main -replace '\)\?;', ') {
    let start_minimized = std::env::args().any(|a| a == "--minimized");
    Box::new(MergedApp::new(cc, start_minimized))
}?;'
# 简单处理：直接改为在闭包内判断
$main = @'
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod file_seeker;
mod doc_searcher;
mod merged_app;

use merged_app::MergedApp;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("超级检索工具"),
        ..Default::default()
    };

    eframe::run_native(
        "SuperSearcher",
        options,
        Box::new(|cc| {
            let start_minimized = std::env::args().any(|a| a == "--minimized");
            Box::new(MergedApp::new(cc, start_minimized))
        }),
    )?;

    Ok(())
}
'@
$main | Set-Content -Path "src\main.rs" -Encoding UTF8

# -------------------------------
# 2. 修复 file_seeker 模块下所有 crate:: 路径
# -------------------------------
Get-ChildItem -Path "src\file_seeker" -Recurse -Filter "*.rs" | ForEach-Object {
    (Get-Content $_.FullName -Raw) -replace 'crate::types', 'crate::file_seeker::types' `
        -replace 'crate::config', 'crate::file_seeker::config' `
        -replace 'crate::engine', 'crate::file_seeker::engine' `
        -replace 'crate::file_list', 'crate::file_seeker::file_list' `
        -replace 'crate::http_server', 'crate::file_seeker::http_server' `
        -replace 'crate::ftp', 'crate::file_seeker::ftp' `
        -replace 'crate::gui', 'crate::file_seeker::gui' `
        -replace 'crate::tray', 'crate::file_seeker::tray' `
        -replace 'crate::autostart', 'crate::file_seeker::autostart' `
        -replace 'crate::watcher', 'crate::file_seeker::watcher' `
        -replace 'crate::etp', 'crate::file_seeker::etp' `
        -replace 'crate::sdk', 'crate::file_seeker::sdk' `
        -replace 'crate::rename', 'crate::file_seeker::rename' `
        -replace 'crate::cli', 'crate::file_seeker::cli' `
        -replace 'crate::history', 'crate::file_seeker::history' `
        | Set-Content -Path $_.FullName -Encoding UTF8
}

# -------------------------------
# 3. 删除 app.rs 中重复的函数定义（保留第一份）
# -------------------------------
$app = Get-Content -Path "src\file_seeker\gui\app.rs" -Raw
# 删除从第 883 行开始的多余定义（手动删除比较难，我们改用简单方法：用正则删除第二个 get_windows_drives 和 get_state_file_path）
$app = $app -replace '(?s)pub fn get_windows_drives.*?Vec<String> \{.*?\n\}\s*\n\s*pub fn get_windows_drives.*?\{.*?\n\}', ''
$app | Set-Content -Path "src\file_seeker\gui\app.rs" -Encoding UTF8

# -------------------------------
# 4. 修复 doc_searcher/app.rs 和 indexer.rs 的类型冲突
# -------------------------------
# 统一使用 indexer.rs 中的 IndexMsg，删除 app.rs 中的重复定义
$app2 = Get-Content -Path "src\doc_searcher\app.rs" -Raw
# 删除 app.rs 中的 pub enum IndexMsg 定义（只保留 indexer 中的）
$app2 = $app2 -replace '(?ms)^pub enum IndexMsg \{.*?\n\}', ''
$app2 = $app2 -replace 'use super::indexer::\*;', 'use super::indexer::*;'
$app2 | Set-Content -Path "src\doc_searcher\app.rs" -Encoding UTF8

# 在 app.rs 顶部添加缺失的 trait 导入
$app2 = Get-Content -Path "src\doc_searcher\app.rs" -Raw
$imports = @'
use notify::Watcher;
use tantivy::schema::Value;
use calamine::Reader;
use chrono::TimeZone;
'@
$app2 = $imports + "`n" + $app2
$app2 | Set-Content -Path "src\doc_searcher\app.rs" -Encoding UTF8

# 在 indexer.rs 顶部添加缺失的 trait 导入
$indexer = Get-Content -Path "src\doc_searcher\indexer.rs" -Raw
$imports2 = @'
use calamine::Reader;
use chrono::TimeZone;
'@
$indexer = $imports2 + "`n" + $indexer
$indexer | Set-Content -Path "src\doc_searcher\indexer.rs" -Encoding UTF8

Write-Host "`n✅ 自动修复完成！请手动检查以下问题：" -ForegroundColor Green
Write-Host "  - 确保 doc_searcher/app.rs 中 render 方法在 impl DocSearcherApp 块内"
Write-Host "  - 删除 indexer.rs 中未被使用的 import（警告可忽略）"
Write-Host "  - 运行 cargo build 查看剩余错误"