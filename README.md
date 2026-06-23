# 抖音登记OCR

抖音登记OCR 是一个用于批量识别抖音数据截图并导出登记表格的桌面工具。它会读取目录中以数字命名的图片，自动识别直播数据和视频数据，并分别生成 `直播.csv` 与 `视频.csv`。

2.0 版本使用 Tauri v2 重构桌面程序，保留原有 Rust OCR 与导出核心。普通使用者可以直接使用桌面程序；开发者和自动化助手可以复用 Rust 库、命令行示例和 OCR 调试程序。

## 当前能力

- 读取目录中以数字命名的图片，例如 `191.jpg`、`192.png`、`212.png`。
- 使用 `ocr-rs` 与 PaddleOCR 的 MNN 模型进行文字识别。
- 自动合并截图中位置相近的字段和值，例如指标名称和它下方或右侧的数值。
- 自动区分直播页和视频页。
- 导出直播数据到 `直播.csv`。
- 导出视频数据到 `视频.csv`。
- 没有识别到的字段写为 `<NULL>`。
- OCR 结果会缓存在图片目录下的 `.ocr-cache`，重复导出时会优先复用缓存。
- 支持 `tiny`、`small`、`medium` 三种模型，默认使用 `medium`。
- 本地缺少模型时，会从 `https://assets.checkpoint321.com/wjm/models/` 自动下载，并在界面显示下载进度、下载速度和当前下载文件。
- 桌面界面支持选择目录、选择模型、暂停、继续、中止、查看关于页和打开开源项目链接。
- 程序启用了单例运行，重复打开时会聚焦已有窗口。

## 导出字段

直播导出文件名为 `直播.csv`，字段如下：

| 字段 | 含义 |
| --- | --- |
| 文件名 | 图片文件名，不包含后缀 |
| 开播时间 | 从截图中提取并整理成带空格的日期时间 |
| 直播时长 | 转换成纯秒数 |
| PV | 对应曝光人数；如果带“万”，会转换为整数 |
| 累计观看人数 | 对应进房人数 |
| ACU | 对应平均在线人数 |

视频导出文件名为 `视频.csv`，字段如下：

| 字段 | 含义 |
| --- | --- |
| 文件名 | 图片文件名，不包含后缀 |
| 播放量 | 带特殊标记合并出来的播放量优先 |
| 点赞量 | 截图中的点赞量 |
| 评论量 | 截图中的评论量 |

## 使用桌面程序

从发布页面下载对应系统的安装包。

- macOS：下载 `douyin-ocr-macos-arm64-app-unsigned.zip`，解压后运行 `抖音登记OCR.app`。
- Windows：下载 `douyin-ocr-windows-x64-setup-unsigned.exe`，双击安装。

首次运行时，如果本机没有所选模型，程序会自动下载模型。下载完成后会继续处理图片。模型会存放在用户目录下的应用数据目录中，不会写入程序安装目录。

程序目前发布的是未签名版本。Windows 可能会出现 SmartScreen 提醒，macOS 可能需要在系统设置的“隐私与安全性”中手动允许运行。

## 使用命令行示例

在项目根目录执行：

```bash
cargo run --example export_csv -- 图片目录
```

示例会读取目录中的数字命名图片，并在同一目录生成 `直播.csv` 与 `视频.csv`。

## 开发环境

需要安装：

- Rust 稳定版
- Bun
- Tauri v2 所需的系统依赖

安装前端依赖：

```bash
bun install
```

启动 Tauri 开发版：

```bash
bun run tauri:dev
```

构建 Tauri 桌面程序：

```bash
bun run tauri:build
```

只构建前端：

```bash
bun run build
```

验证 Rust 核心：

```bash
cargo test
```

运行 OCR 调试程序：

```bash
cargo run --bin test_model
```

`test_model` 会读取 `models/test1.jpg` 和 `models/test2.png`，输出 OCR 合并后的文本块，适合调试识别与合并规则。

## 模型说明

项目不会把大型 `.mnn` 模型提交进源码仓库。程序运行时会按所选档位查找以下文件：

| 档位 | 检测模型 | 识别模型 | 字符表 |
| --- | --- | --- | --- |
| tiny | `PP-OCRv6_tiny_det.mnn` | `PP-OCRv6_tiny_rec.mnn` | `ppocr_keys_v6_tiny.txt` |
| small | `PP-OCRv6_small_det.mnn` | `PP-OCRv6_small_rec.mnn` | `ppocr_keys_v6_small.txt` |
| medium | `PP-OCRv6_medium_det.mnn` | `PP-OCRv6_medium_rec.mnn` | `ppocr_keys_v6_medium.txt` |

模型下载地址前缀为：

```text
https://assets.checkpoint321.com/wjm/models/
```

## 发布

推送形如 `v2.0.0` 的标签会触发发布流程。当前自动构建以下未签名产物：

- macOS Apple 芯片版本，产物为包含 `抖音登记OCR.app` 的 zip。
- Windows x64 版本，产物为 NSIS 安装器。

后续不再构建 macOS x64 版本。

## 项目结构

| 路径 | 说明 |
| --- | --- |
| `src/ocr.rs` | OCR 客户端、模型档位、识别结果合并规则 |
| `src/export.rs` | 目录扫描、缓存、字段提取、CSV 导出、模型下载进度 |
| `src/lib.rs` | 可复用 Rust 库入口 |
| `examples/export_csv.rs` | 命令行导出示例 |
| `src/bin/test_model.rs` | OCR 调试程序 |
| `src/bin/export_gui.rs` | 旧 Slint 界面，保留为可选特性，不参与 2.0 默认构建 |
| `app/` | Tauri 前端源码 |
| `src-tauri/` | Tauri v2 桌面后端、配置、权限与打包信息 |
| `models/` | 测试图片和小字符表，大型模型不提交 |
| `.github/workflows/release.yml` | 自动构建和发布配置 |

## 给大语言模型和自动化助手的阅读提示

这个项目的核心行为在 `src/export.rs` 和 `src/ocr.rs`。如果要改识别结果如何合并，优先查看 `src/ocr.rs` 中的后处理逻辑。如果要改导出的字段、直播页与视频页判断、缺失值策略、时长转换或缓存逻辑，优先查看 `src/export.rs`。

桌面程序 2.0 的入口在 `src-tauri/src/lib.rs` 和 `app/src/main.ts`。Rust 后端负责调用导出核心、处理暂停继续中止、发送进度事件；前端负责目录选择、模型选择、进度展示、日志、关于页和链接打开。

不要把大型 `.mnn` 模型提交到仓库。它们已经在 `.gitignore` 中排除，并且程序可以从 CDN 下载。

如果要发布新版本，先确认 `bun run build`、`cargo test` 和 Tauri 构建检查通过，然后提交代码，创建并推送新的版本标签。

## 旧 Slint 界面

旧界面仍保留在 `src/bin/export_gui.rs`，但依赖已改为可选特性。需要调试旧界面时执行：

```bash
cargo run --features slint-gui --bin export_gui
```

2.0 发布流程不再使用旧 Slint 界面。

## 开源项目

本项目使用或参考了以下开源项目：

| 名称 | 协议 | 仓库 |
| --- | --- | --- |
| PaddleOCR | Apache-2.0 | https://github.com/PaddlePaddle/PaddleOCR |
| ocr-rs | Apache-2.0 | https://github.com/zibo-chen/rust-paddle-ocr |
| MNN | Apache-2.0 | https://github.com/alibaba/MNN |
| Tauri | Apache-2.0 或 MIT | https://github.com/tauri-apps/tauri |
| Vite | MIT | https://github.com/vitejs/vite |
| TypeScript | Apache-2.0 | https://github.com/microsoft/TypeScript |
| Slint | GPL-3.0 或 Slint 授权 | https://github.com/slint-ui/slint |
| rfd | MIT | https://github.com/PolyMeilex/rfd |
| image | MIT 或 Apache-2.0 | https://github.com/image-rs/image |
| anyhow | MIT 或 Apache-2.0 | https://github.com/dtolnay/anyhow |
| ureq | MIT 或 Apache-2.0 | https://github.com/algesten/ureq |
| webbrowser | MIT 或 Apache-2.0 | https://github.com/amodm/webbrowser-rs |
| Rust | MIT 或 Apache-2.0 | https://github.com/rust-lang/rust |

## 开发者

开发者：三氢

主页：https://github.com/isTrih
