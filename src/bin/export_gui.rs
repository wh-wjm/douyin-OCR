use anyhow::Result;
use slint::{ComponentHandle, SharedString};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use whwjm_ocr::{
    ExportControl, ExportEvent, ExportOptions, ModelDownloadProgress, OcrModelTier,
    export_csv_with_events,
};

slint::slint! {
    import {
        Button, ComboBox, HorizontalBox, LineEdit, ProgressIndicator, ScrollView, VerticalBox
    } from "std-widgets.slint";

    component ProjectRow inherits Rectangle {
        in property <string> project_name;
        in property <string> license;
        in property <string> repo;
        callback open_url(url: string);

        height: 56px;
        border-color: #e2e8f0;
        border-width: 1px;
        border-radius: 6px;
        background: touch.has-hover ? #f8fbff : #ffffff;

        HorizontalBox {
            padding-left: 12px;
            padding-right: 12px;
            spacing: 12px;

            Text {
                text: root.project_name;
                width: 130px;
                vertical-alignment: center;
                font-weight: 700;
                color: #172033;
                overflow: elide;
            }

            Text {
                text: root.license;
                width: 155px;
                vertical-alignment: center;
                color: #3b4a62;
                overflow: elide;
            }

            Text {
                text: root.repo;
                vertical-alignment: center;
                color: #0a66c2;
                wrap: word-wrap;
            }
        }

        touch := TouchArea {
            mouse-cursor: pointer;
            clicked => {
                root.open_url(root.repo);
            }
        }
    }

    export component ExportApp inherits Window {
        title: "抖音登记OCR";
        icon: @image-url("../../ocr-logo.png");
        width: 860px;
        height: 680px;
        default-font-family: "PingFang SC, Microsoft YaHei, Noto Sans CJK SC, sans-serif";

        in-out property <string> folder_path;
        in-out property <string> status_text: "请选择包含数字命名图片的目录";
        in-out property <string> download_text;
        in-out property <string> progress_text;
        in-out property <bool> is_running;
        in-out property <bool> is_paused;
        in-out property <bool> show_about;
        in-out property <int> model_index: 2;
        in-out property <float> progress_value;

        callback browse_folder();
        callback start_export();
        callback pause_export();
        callback resume_export();
        callback stop_export();
        callback open_about();
        callback close_about();
        callback open_link(url: string);

        if !root.show_about : VerticalBox {
            padding: 16px;
            spacing: 12px;

            HorizontalBox {
                spacing: 8px;

                Text {
                    text: "抖音登记OCR";
                    font-size: 24px;
                    font-weight: 700;
                    horizontal-stretch: 1;
                    vertical-alignment: center;
                }

                Button {
                    text: "关于";
                    enabled: !root.is_running;
                    clicked => {
                        root.open_about();
                    }
                }
            }

            Text {
                text: "选择图片目录后导出直播.csv 和视频.csv；OCR 结果会缓存到目录下的 .ocr-cache。首次缺少模型时会从 CDN 下载对应档位。";
                color: #555;
                wrap: word-wrap;
            }

            HorizontalBox {
                spacing: 8px;

                LineEdit {
                    text <=> root.folder_path;
                    enabled: !root.is_running;
                    placeholder-text: "图片目录";
                }

                Button {
                    text: "选择目录";
                    enabled: !root.is_running;
                    clicked => {
                        root.browse_folder();
                    }
                }
            }

            HorizontalBox {
                spacing: 8px;

                Text {
                    text: "模型";
                    width: 38px;
                    vertical-alignment: center;
                    color: #333;
                }

                ComboBox {
                    model: ["tiny", "small", "medium"];
                    current-index <=> root.model_index;
                    enabled: !root.is_running;
                    width: 150px;
                }

                Button {
                    text: root.is_running ? "导出中..." : "开始导出";
                    enabled: !root.is_running && root.folder_path != "";
                    clicked => {
                        root.start_export();
                    }
                }

                Button {
                    text: root.is_paused ? "继续" : "暂停";
                    enabled: root.is_running;
                    clicked => {
                        if root.is_paused {
                            root.resume_export();
                        } else {
                            root.pause_export();
                        }
                    }
                }

                Button {
                    text: "中止";
                    enabled: root.is_running;
                    clicked => {
                        root.stop_export();
                    }
                }
            }

            Text {
                text: root.status_text;
                color: root.is_running ? #0a66c2 : #333;
                wrap: word-wrap;
            }

            if root.download_text != "" : Text {
                text: root.download_text;
                color: #0a66c2;
                wrap: word-wrap;
            }

            ProgressIndicator {
                progress: root.progress_value;
                indeterminate: root.is_running && root.progress_value <= 0;
                height: 6px;
            }

            Rectangle {
                border-color: #ddd;
                border-width: 1px;
                background: #fafafa;
                border-radius: 6px;
                vertical-stretch: 1;

                ScrollView {
                    x: 10px;
                    y: 10px;
                    width: parent.width - 20px;
                    height: parent.height - 20px;

                    Text {
                        width: parent.width;
                        text: root.progress_text;
                        color: #333;
                        wrap: word-wrap;
                    }
                }
            }
        }

        if root.show_about : VerticalBox {
            padding: 16px;
            spacing: 12px;

            HorizontalBox {
                spacing: 8px;

                Text {
                    text: "关于";
                    font-size: 24px;
                    font-weight: 700;
                    horizontal-stretch: 1;
                    vertical-alignment: center;
                }

                Button {
                    text: "返回";
                    clicked => {
                        root.close_about();
                    }
                }
            }

            Rectangle {
                height: 96px;
                border-color: #d7dee8;
                border-width: 1px;
                border-radius: 6px;
                background: #ffffff;

                VerticalBox {
                    padding: 12px;
                    spacing: 4px;

                    Text { text: "开发者：三氢@芜湖玩家盟"; font-weight: 700; color: #172033; }
                    Text { text: "三氢 GitHub：https://github.com/isTrih"; color: #0a66c2; }
                    Text { text: "芜湖玩家盟官网：https://www.topgamers.com.cn/"; color: #0a66c2; }
                }

                TouchArea {
                    mouse-cursor: pointer;
                    clicked => {
                        root.open_link("https://github.com/isTrih");
                    }
                }
            }

            Text {
                text: "开源项目";
                font-size: 18px;
                font-weight: 700;
                color: #172033;
            }

            ScrollView {
                vertical-stretch: 1;

                VerticalBox {
                    spacing: 8px;

                    ProjectRow { project_name: "PaddleOCR"; license: "Apache-2.0"; repo: "https://github.com/PaddlePaddle/PaddleOCR"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "ocr-rs"; license: "Apache-2.0"; repo: "https://github.com/zibo-chen/rust-paddle-ocr"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "MNN"; license: "Apache-2.0"; repo: "https://github.com/alibaba/MNN"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "Slint"; license: "GPL-3.0 / Slint License"; repo: "https://github.com/slint-ui/slint"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "rfd"; license: "MIT"; repo: "https://github.com/PolyMeilex/rfd"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "image"; license: "MIT OR Apache-2.0"; repo: "https://github.com/image-rs/image"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "anyhow"; license: "MIT OR Apache-2.0"; repo: "https://github.com/dtolnay/anyhow"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "ureq"; license: "MIT OR Apache-2.0"; repo: "https://github.com/algesten/ureq"; open_url(url) => { root.open_link(url); } }
                    ProjectRow { project_name: "Rust"; license: "MIT OR Apache-2.0"; repo: "https://github.com/rust-lang/rust"; open_url(url) => { root.open_link(url); } }
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let app = ExportApp::new()?;
    let export_control = Arc::new(ExportControl::default());

    let app_weak = app.as_weak();
    app.on_browse_folder(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder()
            && let Some(app) = app_weak.upgrade()
        {
            app.set_folder_path(folder.display().to_string().into());
            app.set_status_text("目录已选择，点击开始导出".into());
            app.set_download_text(SharedString::default());
            app.set_progress_text(SharedString::default());
        }
    });

    let app_weak = app.as_weak();
    app.on_open_about(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_show_about(true);
        }
    });

    let app_weak = app.as_weak();
    app.on_close_about(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_show_about(false);
        }
    });

    let app_weak = app.as_weak();
    app.on_open_link(move |url| {
        let url = url.to_string();
        if let Err(error) = webbrowser::open(&url)
            && let Some(app) = app_weak.upgrade()
        {
            app.set_status_text(format!("打开链接失败：{error}").into());
        }
    });

    let pause_control = export_control.clone();
    let app_weak = app.as_weak();
    app.on_pause_export(move || {
        pause_control.request_pause();
        if let Some(app) = app_weak.upgrade() {
            app.set_is_paused(true);
            app.set_status_text("已暂停，会在当前图片处理完成后停在下一张前".into());
        }
    });

    let resume_control = export_control.clone();
    let app_weak = app.as_weak();
    app.on_resume_export(move || {
        resume_control.resume();
        if let Some(app) = app_weak.upgrade() {
            app.set_is_paused(false);
            app.set_status_text("继续导出...".into());
        }
    });

    let stop_control = export_control.clone();
    let app_weak = app.as_weak();
    app.on_stop_export(move || {
        stop_control.request_stop();
        if let Some(app) = app_weak.upgrade() {
            app.set_is_paused(false);
            app.set_status_text("正在中止，会在当前图片处理完成后停止".into());
        }
    });

    let app_weak = app.as_weak();
    let start_control = export_control.clone();
    app.on_start_export(move || {
        let Some(app) = app_weak.upgrade() else {
            return;
        };

        let folder = app.get_folder_path().to_string();
        if folder.trim().is_empty() {
            app.set_status_text("请先选择图片目录".into());
            return;
        }

        app.set_is_running(true);
        app.set_is_paused(false);
        app.set_status_text("正在导出...".into());
        app.set_download_text(SharedString::default());
        app.set_progress_text(SharedString::default());
        app.set_progress_value(0.0);

        let worker_weak = app.as_weak();
        let model_tier = OcrModelTier::from_index(app.get_model_index());
        let control = start_control.clone();
        thread::spawn(move || {
            let folder = PathBuf::from(folder);
            let options = ExportOptions::new(&folder)
                .with_model_dir(default_model_dir())
                .with_model_tier(model_tier)
                .with_control(control);

            let result = export_csv_with_events(options, |event| {
                handle_export_event(worker_weak.clone(), event);
            });

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = worker_weak.upgrade() {
                    app.set_is_running(false);
                    app.set_is_paused(false);

                    match result {
                        Ok(summary) => {
                            app.set_progress_value(1.0);
                            app.set_download_text(SharedString::default());
                            app.set_status_text(
                                format!(
                                    "导出完成：{} 张图片，直播 {} 行，视频 {} 行",
                                    summary.image_count,
                                    summary.live_row_count,
                                    summary.video_row_count
                                )
                                .into(),
                            );
                            append_progress_text(
                                &app,
                                format!("直播CSV：{}", summary.live_csv_path.display()),
                            );
                            append_progress_text(
                                &app,
                                format!("视频CSV：{}", summary.video_csv_path.display()),
                            );
                        }
                        Err(error) => {
                            app.set_progress_value(0.0);
                            app.set_status_text(format!("导出失败：{error:#}").into());
                        }
                    }
                }
            });
        });
    });

    app.run()?;

    Ok(())
}

fn handle_export_event(app_weak: slint::Weak<ExportApp>, event: ExportEvent) {
    match event {
        ExportEvent::ModelDownload(progress) => {
            update_model_download(app_weak, progress);
        }
        ExportEvent::Image(progress) => {
            let file_name = progress
                .image_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
                .to_owned();
            let line = format!(
                "{}/{} {}{}",
                progress.current,
                progress.total,
                file_name,
                if progress.cache_hit {
                    " (cache hit)"
                } else {
                    ""
                }
            );
            let progress_value = if progress.total == 0 {
                0.0
            } else {
                progress.current as f32 / progress.total as f32
            };

            append_progress(app_weak, line, progress_value);
        }
    }
}

fn update_model_download(app_weak: slint::Weak<ExportApp>, progress: ModelDownloadProgress) {
    let progress_value = download_progress_value(&progress);
    let text = format_model_download_text(&progress);
    let completed_line = progress.finished.then(|| {
        format!(
            "模型下载完成：{} ({})",
            progress.file_name,
            format_bytes(progress.downloaded_bytes)
        )
    });

    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_status_text(
                format!(
                    "本地没有 {} 模型，正在下载...",
                    progress.model_tier.as_str()
                )
                .into(),
            );
            app.set_download_text(text.into());
            app.set_progress_value(progress_value);

            if let Some(line) = completed_line {
                append_progress_text(&app, line);
            }
        }
    });
}

fn append_progress(app_weak: slint::Weak<ExportApp>, line: String, progress_value: f32) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_download_text(SharedString::default());
            app.set_progress_value(progress_value);
            append_progress_text(&app, line);
        }
    });
}

fn append_progress_text(app: &ExportApp, line: String) {
    let mut progress = app.get_progress_text().to_string();
    if !progress.is_empty() {
        progress.push('\n');
    }
    progress.push_str(&line);
    app.set_progress_text(progress.into());
}

fn default_model_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("whwjm-ocr").join("models");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("whwjm-ocr")
                .join("models");
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("whwjm-ocr")
            .join("models");
    }

    PathBuf::from("models")
}

fn download_progress_value(progress: &ModelDownloadProgress) -> f32 {
    progress
        .total_bytes
        .filter(|total| *total > 0)
        .map(|total| progress.downloaded_bytes as f32 / total as f32)
        .unwrap_or(0.0)
}

fn format_model_download_text(progress: &ModelDownloadProgress) -> String {
    let total = progress
        .total_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "未知大小".to_owned());
    let percent = progress
        .total_bytes
        .filter(|total| *total > 0)
        .map(|total| {
            format!(
                "{:.1}%",
                progress.downloaded_bytes as f64 * 100.0 / total as f64
            )
        })
        .unwrap_or_else(|| "--".to_owned());

    format!(
        "本地没有 {} 模型，正在下载 {}：{} / {}，{}，{}",
        progress.model_tier.as_str(),
        progress.file_name,
        format_bytes(progress.downloaded_bytes),
        total,
        percent,
        format_speed(progress.bytes_per_second)
    )
}

fn format_speed(bytes_per_second: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_second.max(0.0) as u64))
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.2} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{} B", bytes as u64)
    }
}
