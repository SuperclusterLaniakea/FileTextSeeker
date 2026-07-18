# FileTextSeeker — 超级检索工具

**FileTextSeeker**（超级检索工具）是一个基于 Rust + egui 构建的桌面多功能文件处理工具集，集文件搜索、文档全文检索、文件清单生成、代码合并于一体。

---

## ✨ 功能特性

### 📁 文件搜索（File Search）
- 基于 Everything 协议的快速文件搜索引擎
- 高性能索引：支持多线程并行扫描目录
- 支持按文件名、扩展名、路径、大小、日期等多维度搜索
- 支持通配符、正则表达式搜索
- 支持排除路径和排除模式
- 文件列表快速浏览与筛选
- 支持 HTTP / ETP / FTP 服务器远程访问
- 文件重命名、文件历史记录
- 系统托盘运行，支持开机自启
- 可导出文件列表

### 📄 文档检索（Doc Search）
- 基于 **Tantivy** 全文检索引擎，支持中文分词（jieba-rs）
- 支持格式：**PDF**、**DOCX**、**TXT**、**XLSX/XLS**、**PPTX**
- 数据库管理：支持多数据库创建、切换、删除
- 增量索引与对比更新
- 支持高级查询语法：`AND / OR / NOT`、通配符 `*` / `?`、短语匹配 `"..."`、`+` 必须 / `-` 排除
- 搜索结果按相关度、文件名、日期、词频排序
- 关键词高亮预览与定位跳转
- 支持导出搜索结果为 CSV
- 可配置 PDF 阅读器命令，支持页码跳转

### 📋 文件清单生成器（File List Generator）
- 选择源文件夹，自动扫描所有文件扩展名
- 按扩展名筛选要包含的文件
- 支持递归遍历子文件夹
- 生成文件清单：序号、文件名、扩展名、路径、创建时间、修改时间、文件大小
- 导出格式：**CSV** 或 **XLSX**（Excel）

### 🔧 代码合并工具（Code Merger）
- 选择源码目录，自动识别 30+ 种编程语言和配置文件
- 支持的扩展名：`rs`, `c`, `cpp`, `h`, `hpp`, `py`, `java`, `go`, `js`, `ts`, `rb`, `php`, `swift`, `kt`, `cs`, `lua`, `toml`, `json`, `yaml`, `xml`, `md` 等
- 按文件名/路径筛选过滤
- 全选 / 取消全选 / 反选文件
- 将选中的代码文件合并为一个文本文件，保留文件分隔和路径信息

---

## 🖼️ 界面截图

> 程序主窗口包含四个标签页，顶部导航栏可快速切换功能模块。

---

## 🚀 快速开始

### 环境要求
- Rust 1.70+
- Windows 10+（推荐，支持托盘功能）

### 编译运行

```bash
# 克隆项目
git clone <repo-url>
cd FileTextSeeker

# 构建
cargo build --release

# 运行
cargo run --release

# 以最小化到托盘方式启动
cargo run --release -- --minimized
```

### 构建说明

项目使用 `embed-resource` 在 Windows 上嵌入图标和版本信息，构建脚本会自动处理。

---

## 📦 项目结构

```
FileTextSeeker/
├── Cargo.toml              # 项目配置与依赖
├── build.rs                # 构建脚本（Windows 资源嵌入）
├── icon.ico                # 程序图标
├── resource.rc             # Windows 资源文件
├── src/
│   ├── main.rs             # 入口：窗口创建、图标加载
│   ├── merged_app.rs       # 主应用：标签页导航与布局
│   ├── file_seeker/        # 📁 文件搜索模块
│   │   ├── engine/         #   索引引擎（indexer, searcher, sorter, database）
│   │   ├── gui/            #   界面组件（app, search_panel, results_panel, options_panel）
│   │   ├── cli/            #   命令行接口
│   │   ├── config.rs       #   配置文件
│   │   ├── types.rs        #   数据类型定义
│   │   ├── tray/           #   系统托盘
│   │   ├── watcher/        #   文件变更监控
│   │   ├── http_server/    #   HTTP 远程访问
│   │   ├── etp/            #   ETP 协议
│   │   ├── ftp/            #   FTP 协议
│   │   ├── sdk/            #   Everything SDK 兼容
│   │   ├── rename/         #   文件重命名
│   │   ├── history/        #   运行历史
│   │   ├── file_list/      #   文件列表管理
│   │   └── autostart.rs    #   开机自启
│   ├── doc_searcher/       # 📄 文档检索模块
│   │   ├── app.rs          #   界面与主逻辑
│   │   ├── indexer.rs      #   索引引擎（Tantivy + 文件解析）
│   │   └── mod.rs          #   模块声明
│   ├── file_lister_gui/    # 📋 文件清单生成器模块
│   │   └── mod.rs          #   完整实现
│   └── code_merger/        # 🔧 代码合并工具模块
│       └── mod.rs          #   完整实现
└── target/                 # 编译输出
```

---

## 🛠️ 技术栈

| 类别 | 技术 |
|------|------|
| 语言 | Rust (edition 2021) |
| GUI 框架 | eframe / egui 0.27 |
| 全文检索引擎 | Tantivy 0.22 |
| 中文分词 | jieba-rs |
| 文档解析 | pdf-extract, docx-rs, calamine, zip + quick-xml |
| 文件变更监控 | notify + notify-debouncer-full |
| 序列化 | serde / serde_json / bincode |
| 嵌入式数据库 | sled |
| 文件遍历 | walkdir / jwalk |
| 办公文件输出 | csv, rust_xlsxwriter |

---

## 📄 许可证

本项目仅供学习参考使用。

---

## 🙏 致谢

- [file-seeker](https://github.com) — 文件搜索参考项目
- [doc-searcher](https://github.com) — 文档检索参考项目
- [file-lister-gui](https://github.com) — 文件清单生成器参考项目
- [CodeMerger](https://github.com) — 代码合并工具参考项目