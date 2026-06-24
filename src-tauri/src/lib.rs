use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use whwjm_ocr::{
    ExportControl, ExportEvent, ExportOptions, ExportSummary, ModelDownloadProgress,
    OcrEngineConfig, OcrModelTier, export_csv_with_events,
};

#[derive(Default)]
struct AppState {
    control: Mutex<Option<Arc<ExportControl>>>,
    running: AtomicBool,
    paused: AtomicBool,
}

#[derive(Serialize, Clone)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum UiExportEvent {
    ModelDownload {
        model_tier: String,
        file_name: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        bytes_per_second: f64,
        finished: bool,
    },
    Image {
        current: usize,
        total: usize,
        image_path: String,
        file_name: String,
        cache_hit: bool,
    },
    Complete {
        summary: UiExportSummary,
    },
    Error {
        message: String,
    },
    State {
        running: bool,
        paused: bool,
    },
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UiExportSummary {
    live_csv_path: String,
    video_csv_path: String,
    image_count: usize,
    live_row_count: usize,
    video_row_count: usize,
}

#[tauri::command]
fn start_export(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    image_dir: String,
    model_tier: String,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("已有导出任务正在运行".to_owned());
    }

    let tier = parse_model_tier(&model_tier)?;
    let control = Arc::new(ExportControl::default());
    {
        let mut current_control = state.control.lock().map_err(|error| error.to_string())?;
        *current_control = Some(control.clone());
    }
    state.paused.store(false, Ordering::SeqCst);

    emit_event(
        &app,
        UiExportEvent::State {
            running: true,
            paused: false,
        },
    );

    let app_state = state.inner().clone();
    std::thread::spawn(move || {
        let model_dir = model_dir(&app);
        let options = ExportOptions::new(PathBuf::from(image_dir))
            .with_model_dir(model_dir)
            .with_model_tier(tier)
            .with_ocr_engine_config(desktop_ocr_engine_config())
            .with_control(control);

        let result = export_csv_with_events(options, |event| {
            emit_event(&app, map_export_event(event));
        });

        match result {
            Ok(summary) => emit_event(
                &app,
                UiExportEvent::Complete {
                    summary: map_summary(summary),
                },
            ),
            Err(error) => emit_event(
                &app,
                UiExportEvent::Error {
                    message: format!("{error:#}"),
                },
            ),
        }

        app_state.running.store(false, Ordering::SeqCst);
        app_state.paused.store(false, Ordering::SeqCst);
        if let Ok(mut current_control) = app_state.control.lock() {
            *current_control = None;
        }
        emit_event(
            &app,
            UiExportEvent::State {
                running: false,
                paused: false,
            },
        );
    });

    Ok(())
}

#[tauri::command]
fn pause_export(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let control = current_control(&state)?;
    control.request_pause();
    state.paused.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn resume_export(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let control = current_control(&state)?;
    control.resume();
    state.paused.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn stop_export(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let control = current_control(&state)?;
    control.request_stop();
    state.paused.store(false, Ordering::SeqCst);
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _args, _working_directory| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            start_export,
            pause_export,
            resume_export,
            stop_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

fn current_control(state: &State<'_, Arc<AppState>>) -> Result<Arc<ExportControl>, String> {
    state
        .control
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "当前没有正在运行的导出任务".to_owned())
}

fn parse_model_tier(value: &str) -> Result<OcrModelTier, String> {
    match value {
        "tiny" => Ok(OcrModelTier::Tiny),
        "small" => Ok(OcrModelTier::Small),
        "medium" => Ok(OcrModelTier::Medium),
        other => Err(format!("未知模型档位：{other}")),
    }
}

fn desktop_ocr_engine_config() -> OcrEngineConfig {
    OcrEngineConfig::fast()
        .with_threads(2)
        .with_parallel(false)
        .with_min_result_confidence(0.5)
}

fn map_export_event(event: ExportEvent) -> UiExportEvent {
    match event {
        ExportEvent::ModelDownload(progress) => map_model_download(progress),
        ExportEvent::Image(progress) => {
            let file_name = progress
                .image_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
                .to_owned();

            UiExportEvent::Image {
                current: progress.current,
                total: progress.total,
                image_path: progress.image_path.display().to_string(),
                file_name,
                cache_hit: progress.cache_hit,
            }
        }
    }
}

fn map_model_download(progress: ModelDownloadProgress) -> UiExportEvent {
    UiExportEvent::ModelDownload {
        model_tier: progress.model_tier.as_str().to_owned(),
        file_name: progress.file_name,
        downloaded_bytes: progress.downloaded_bytes,
        total_bytes: progress.total_bytes,
        bytes_per_second: progress.bytes_per_second,
        finished: progress.finished,
    }
}

fn map_summary(summary: ExportSummary) -> UiExportSummary {
    UiExportSummary {
        live_csv_path: summary.live_csv_path.display().to_string(),
        video_csv_path: summary.video_csv_path.display().to_string(),
        image_count: summary.image_count,
        live_row_count: summary.live_row_count,
        video_row_count: summary.video_row_count,
    }
}

fn emit_event(app: &AppHandle, event: UiExportEvent) {
    if let Err(error) = app.emit("export-event", event) {
        eprintln!("failed to emit export event: {error}");
    }
}

fn model_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| fallback_model_dir())
        .join("models")
}

fn fallback_model_dir() -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_export_event_fields_as_camel_case() {
        let model_download = serde_json::to_value(UiExportEvent::ModelDownload {
            model_tier: "medium".to_owned(),
            file_name: "PP-OCRv6_medium_det.mnn".to_owned(),
            downloaded_bytes: 1024,
            total_bytes: Some(2048),
            bytes_per_second: 512.0,
            finished: false,
        })
        .expect("model download event should serialize");

        assert_eq!(model_download["kind"], "modelDownload");
        assert_eq!(model_download["modelTier"], "medium");
        assert_eq!(model_download["fileName"], "PP-OCRv6_medium_det.mnn");
        assert_eq!(model_download["downloadedBytes"], 1024);
        assert_eq!(model_download["totalBytes"], 2048);
        assert_eq!(model_download["bytesPerSecond"], 512.0);
        assert_eq!(model_download.get("model_tier"), None);
        assert_eq!(model_download.get("file_name"), None);

        let image = serde_json::to_value(UiExportEvent::Image {
            current: 1,
            total: 2,
            image_path: "/tmp/1.png".to_owned(),
            file_name: "1.png".to_owned(),
            cache_hit: true,
        })
        .expect("image event should serialize");

        assert_eq!(
            image,
            json!({
                "kind": "image",
                "current": 1,
                "total": 2,
                "imagePath": "/tmp/1.png",
                "fileName": "1.png",
                "cacheHit": true
            })
        );

        let complete = serde_json::to_value(UiExportEvent::Complete {
            summary: UiExportSummary {
                live_csv_path: "/tmp/直播.csv".to_owned(),
                video_csv_path: "/tmp/视频.csv".to_owned(),
                image_count: 3,
                live_row_count: 2,
                video_row_count: 1,
            },
        })
        .expect("complete event should serialize");

        assert_eq!(complete["summary"]["liveCsvPath"], "/tmp/直播.csv");
        assert_eq!(complete["summary"]["videoCsvPath"], "/tmp/视频.csv");
        assert_eq!(complete["summary"]["imageCount"], 3);
        assert_eq!(complete["summary"]["liveRowCount"], 2);
        assert_eq!(complete["summary"]["videoRowCount"], 1);
        assert_eq!(complete["summary"].get("live_csv_path"), None);
    }
}
