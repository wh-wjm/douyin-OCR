use crate::ocr::{
    DEFAULT_MODEL_CDN_BASE_URL, OcrClient, OcrClientConfig, OcrModelTier, OcrTextBlock,
};
use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::UNIX_EPOCH;

const MODEL_DOWNLOAD_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ExportProgress {
    pub current: usize,
    pub total: usize,
    pub image_path: PathBuf,
    pub cache_hit: bool,
}

#[derive(Debug, Clone)]
pub struct ExportSummary {
    pub live_csv_path: PathBuf,
    pub video_csv_path: PathBuf,
    pub image_count: usize,
    pub live_row_count: usize,
    pub video_row_count: usize,
}

#[derive(Debug, Clone)]
pub enum ExportEvent {
    ModelDownload(ModelDownloadProgress),
    Image(ExportProgress),
}

#[derive(Debug, Clone)]
pub struct ModelDownloadProgress {
    pub model_tier: OcrModelTier,
    pub file_name: String,
    pub url: String,
    pub path: PathBuf,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_second: f64,
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub image_dir: PathBuf,
    pub model_dir: PathBuf,
    pub model_tier: OcrModelTier,
    pub model_cdn_base_url: String,
    pub download_missing_models: bool,
    pub control: Option<Arc<ExportControl>>,
}

impl ExportOptions {
    pub fn new(image_dir: impl Into<PathBuf>) -> Self {
        Self {
            image_dir: image_dir.into(),
            model_dir: PathBuf::from("models"),
            model_tier: OcrModelTier::Medium,
            model_cdn_base_url: DEFAULT_MODEL_CDN_BASE_URL.to_owned(),
            download_missing_models: true,
            control: None,
        }
    }

    pub fn with_model_dir(mut self, model_dir: impl Into<PathBuf>) -> Self {
        self.model_dir = model_dir.into();
        self
    }

    pub fn with_model_tier(mut self, model_tier: OcrModelTier) -> Self {
        self.model_tier = model_tier;
        self
    }

    pub fn with_control(mut self, control: Arc<ExportControl>) -> Self {
        self.control = Some(control);
        self
    }
}

#[derive(Debug, Default)]
pub struct ExportControl {
    pause_requested: AtomicBool,
    stop_requested: AtomicBool,
}

impl ExportControl {
    pub fn request_pause(&self) {
        self.pause_requested.store(true, AtomicOrdering::SeqCst);
    }

    pub fn resume(&self) {
        self.pause_requested.store(false, AtomicOrdering::SeqCst);
    }

    pub fn request_stop(&self) {
        self.stop_requested.store(true, AtomicOrdering::SeqCst);
        self.resume();
    }

    pub fn reset(&self) {
        self.pause_requested.store(false, AtomicOrdering::SeqCst);
        self.stop_requested.store(false, AtomicOrdering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.pause_requested.load(AtomicOrdering::SeqCst)
    }

    pub fn is_stopped(&self) -> bool {
        self.stop_requested.load(AtomicOrdering::SeqCst)
    }
}

#[derive(Debug, Default)]
struct LiveRow {
    file_name: String,
    start_time: String,
    duration_seconds: String,
    pv: String,
    total_viewers: String,
    acu: String,
}

#[derive(Debug, Default)]
struct VideoRow {
    file_name: String,
    play_count: String,
    like_count: String,
    comment_count: String,
}

pub fn export_csv(
    image_dir: impl AsRef<Path>,
    on_progress: impl FnMut(ExportProgress),
) -> Result<ExportSummary> {
    export_csv_with_options(
        ExportOptions::new(image_dir.as_ref().to_path_buf()),
        on_progress,
    )
}

pub fn export_csv_with_options(
    options: ExportOptions,
    mut on_progress: impl FnMut(ExportProgress),
) -> Result<ExportSummary> {
    export_csv_with_events(options, |event| {
        if let ExportEvent::Image(progress) = event {
            on_progress(progress);
        }
    })
}

pub fn export_csv_with_events(
    options: ExportOptions,
    mut on_event: impl FnMut(ExportEvent),
) -> Result<ExportSummary> {
    let image_dir = options.image_dir.as_path();
    anyhow::ensure!(
        image_dir.is_dir(),
        "image_dir is not a directory: {}",
        image_dir.display()
    );

    if let Some(control) = &options.control {
        control.reset();
    }

    let ocr_config = OcrClientConfig::for_tier_in_dir(options.model_tier, &options.model_dir);
    if options.download_missing_models {
        ensure_models_available(
            &ocr_config,
            options.model_tier,
            &options.model_cdn_base_url,
            &mut on_event,
        )?;
    }

    let image_paths = collect_numeric_image_paths(&image_dir)?;
    let cache_dir = image_dir.join(".ocr-cache");
    fs::create_dir_all(&cache_dir)?;
    let mut client = None::<OcrClient>;

    let mut live_rows = Vec::new();
    let mut video_rows = Vec::new();

    let total = image_paths.len();

    for (index, image_path) in image_paths.into_iter().enumerate() {
        wait_for_resume_or_stop(options.control.as_deref())?;

        let file_name = image_path
            .file_stem()
            .and_then(|name| name.to_str())
            .context("image file name is not valid UTF-8")?
            .to_owned();

        let (blocks, cache_hit) = recognize_with_cache(
            &image_path,
            &cache_dir,
            options.model_tier,
            &ocr_config,
            &mut client,
        )?;
        on_event(ExportEvent::Image(ExportProgress {
            current: index + 1,
            total,
            image_path: image_path.clone(),
            cache_hit,
        }));
        if is_live_page(&blocks) {
            live_rows.push(extract_live_row(&file_name, &blocks));
        }
        if is_video_page(&blocks) {
            video_rows.push(extract_video_row(&file_name, &blocks));
        }
    }

    let live_csv_path = image_dir.join("直播.csv");
    let video_csv_path = image_dir.join("视频.csv");

    write_live_csv(&live_csv_path, &live_rows)?;
    write_video_csv(&video_csv_path, &video_rows)?;

    Ok(ExportSummary {
        live_csv_path,
        video_csv_path,
        image_count: total,
        live_row_count: live_rows.len(),
        video_row_count: video_rows.len(),
    })
}

fn wait_for_resume_or_stop(control: Option<&ExportControl>) -> Result<()> {
    let Some(control) = control else {
        return Ok(());
    };

    while control.is_paused() {
        if control.is_stopped() {
            anyhow::bail!("导出已中止");
        }
        thread::sleep(Duration::from_millis(120));
    }

    if control.is_stopped() {
        anyhow::bail!("导出已中止");
    }

    Ok(())
}

fn ensure_models_available(
    config: &OcrClientConfig,
    tier: OcrModelTier,
    cdn_base_url: &str,
    on_event: &mut impl FnMut(ExportEvent),
) -> Result<()> {
    let model_files = [
        (&config.det_model_path, tier.det_file_name()),
        (&config.rec_model_path, tier.rec_file_name()),
        (&config.charset_path, tier.charset_file_name()),
    ];

    for (path, file_name) in model_files {
        if path.exists() {
            continue;
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let base_url = cdn_base_url.trim_end_matches('/');
        let url = format!("{base_url}/{file_name}");
        download_file(&url, path, tier, &file_name, on_event)
            .with_context(|| format!("failed to download OCR model from {url}"))?;
    }

    Ok(())
}

fn download_file(
    url: &str,
    path: &Path,
    tier: OcrModelTier,
    file_name: &str,
    on_event: &mut impl FnMut(ExportEvent),
) -> Result<()> {
    let mut response = ureq::get(url).call()?;
    let total_bytes = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    report_model_download(
        on_event,
        tier,
        file_name,
        url,
        path,
        0,
        total_bytes,
        0.0,
        false,
    );

    let temp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("download")
    ));
    let mut file = fs::File::create(&temp_path)?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MODEL_DOWNLOAD_LIMIT_BYTES)
        .reader();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded_bytes = 0_u64;
    let started_at = Instant::now();
    let mut last_reported_at = started_at;

    loop {
        let read_len = reader.read(&mut buffer)?;
        if read_len == 0 {
            break;
        }

        std::io::Write::write_all(&mut file, &buffer[..read_len])?;
        downloaded_bytes += read_len as u64;

        let now = Instant::now();
        if now.duration_since(last_reported_at) >= Duration::from_millis(200) {
            report_model_download(
                on_event,
                tier,
                file_name,
                url,
                path,
                downloaded_bytes,
                total_bytes,
                download_speed(downloaded_bytes, started_at),
                false,
            );
            last_reported_at = now;
        }
    }

    anyhow::ensure!(downloaded_bytes > 0, "downloaded file is empty: {url}");
    drop(file);
    fs::rename(&temp_path, path)?;

    report_model_download(
        on_event,
        tier,
        file_name,
        url,
        path,
        downloaded_bytes,
        total_bytes,
        download_speed(downloaded_bytes, started_at),
        true,
    );

    Ok(())
}

fn report_model_download(
    on_event: &mut impl FnMut(ExportEvent),
    model_tier: OcrModelTier,
    file_name: &str,
    url: &str,
    path: &Path,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    bytes_per_second: f64,
    finished: bool,
) {
    on_event(ExportEvent::ModelDownload(ModelDownloadProgress {
        model_tier,
        file_name: file_name.to_owned(),
        url: url.to_owned(),
        path: path.to_path_buf(),
        downloaded_bytes,
        total_bytes,
        bytes_per_second,
        finished,
    }));
}

fn download_speed(downloaded_bytes: u64, started_at: Instant) -> f64 {
    let elapsed = started_at.elapsed().as_secs_f64();
    if elapsed <= f64::EPSILON {
        0.0
    } else {
        downloaded_bytes as f64 / elapsed
    }
}

fn collect_numeric_image_paths(image_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    for entry in fs::read_dir(image_dir)? {
        let path = entry?.path();
        if !path.is_file() || !is_supported_image(&path) {
            continue;
        }

        let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        if file_stem.chars().all(|ch| ch.is_ascii_digit()) {
            paths.push(path);
        }
    }

    paths.sort_by(|a, b| compare_numeric_file_stem(a, b));

    Ok(paths)
}

fn recognize_with_cache(
    image_path: &Path,
    cache_dir: &Path,
    model_tier: OcrModelTier,
    ocr_config: &OcrClientConfig,
    client: &mut Option<OcrClient>,
) -> Result<(Vec<OcrTextBlock>, bool)> {
    if let Some(blocks) = read_ocr_cache(image_path, cache_dir, model_tier)? {
        return Ok((blocks, true));
    }

    let client = match client {
        Some(client) => client,
        None => client.insert(OcrClient::new(ocr_config.clone())?),
    };

    let blocks = client.recognize_path(image_path)?;
    write_ocr_cache(image_path, cache_dir, model_tier, &blocks)?;

    Ok((blocks, false))
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "bmp"
            )
        })
}

fn compare_numeric_file_stem(a: &Path, b: &Path) -> Ordering {
    let a_num = file_stem_number(a);
    let b_num = file_stem_number(b);

    a_num.cmp(&b_num).then_with(|| a.cmp(b))
}

fn file_stem_number(path: &Path) -> u64 {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.parse::<u64>().ok())
        .unwrap_or(u64::MAX)
}

fn cache_path_for_image(image_path: &Path, cache_dir: &Path) -> Result<PathBuf> {
    let file_name = image_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("image file name is not valid UTF-8")?;
    Ok(cache_dir.join(format!("{file_name}.tsv")))
}

fn image_cache_signature(image_path: &Path) -> Result<(u64, u64)> {
    let metadata = fs::metadata(image_path)?;
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok((metadata.len(), modified))
}

fn read_ocr_cache(
    image_path: &Path,
    cache_dir: &Path,
    model_tier: OcrModelTier,
) -> Result<Option<Vec<OcrTextBlock>>> {
    let cache_path = cache_path_for_image(image_path, cache_dir)?;
    if !cache_path.exists() {
        return Ok(None);
    }

    let cache = fs::read_to_string(cache_path)?;
    let mut lines = cache.lines();
    let Some(header) = lines.next() else {
        return Ok(None);
    };

    let (image_size, image_modified) = image_cache_signature(image_path)?;
    let expected_header = format!(
        "v2\t{}\t{image_size}\t{image_modified}",
        model_tier.as_str()
    );
    if header != expected_header {
        return Ok(None);
    }

    let mut blocks = Vec::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 7 {
            return Ok(None);
        }

        blocks.push(OcrTextBlock {
            text: unescape_cache_field(fields[0]),
            confidence: fields[1].parse().ok().unwrap_or_default(),
            left: fields[2].parse().ok().unwrap_or_default(),
            top: fields[3].parse().ok().unwrap_or_default(),
            width: fields[4].parse().ok().unwrap_or_default(),
            height: fields[5].parse().ok().unwrap_or_default(),
            is_special: fields[6] == "1",
        });
    }

    Ok(Some(blocks))
}

fn write_ocr_cache(
    image_path: &Path,
    cache_dir: &Path,
    model_tier: OcrModelTier,
    blocks: &[OcrTextBlock],
) -> Result<()> {
    let cache_path = cache_path_for_image(image_path, cache_dir)?;
    let (image_size, image_modified) = image_cache_signature(image_path)?;
    let mut cache = format!(
        "v2\t{}\t{image_size}\t{image_modified}\n",
        model_tier.as_str()
    );

    for block in blocks {
        cache.push_str(&escape_cache_field(&block.text));
        cache.push('\t');
        cache.push_str(&block.confidence.to_string());
        cache.push('\t');
        cache.push_str(&block.left.to_string());
        cache.push('\t');
        cache.push_str(&block.top.to_string());
        cache.push('\t');
        cache.push_str(&block.width.to_string());
        cache.push('\t');
        cache.push_str(&block.height.to_string());
        cache.push('\t');
        cache.push_str(if block.is_special { "1" } else { "0" });
        cache.push('\n');
    }

    fs::write(cache_path, cache)?;
    Ok(())
}

fn escape_cache_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn unescape_cache_field(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next() {
            Some('\\') => output.push('\\'),
            Some('t') => output.push('\t'),
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }

    output
}

fn extract_live_row(file_name: &str, blocks: &[OcrTextBlock]) -> LiveRow {
    let text = joined_text(blocks);

    LiveRow {
        file_name: file_name.to_owned(),
        start_time: extract_start_time(&text).unwrap_or_else(null_value),
        duration_seconds: extract_duration_seconds(&text)
            .or_else(|| {
                extract_metric_value(blocks, &["开播时长", "直播时长"])
                    .and_then(|duration| find_duration_seconds(&duration))
            })
            .map(|seconds| seconds.to_string())
            .unwrap_or_else(null_value),
        pv: extract_metric_value(blocks, &["曝光人数", "PV"])
            .and_then(|value| parse_count_to_integer(&value))
            .map(|value| value.to_string())
            .unwrap_or_else(null_value),
        total_viewers: extract_metric_value(blocks, &["进房人数", "累计观看人数", "累计观看"])
            .map(|value| strip_number_text(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(null_value),
        acu: extract_metric_value(blocks, &["平均在线人数", "ACU"])
            .map(|value| strip_number_text(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(null_value),
    }
}

fn is_live_page(blocks: &[OcrTextBlock]) -> bool {
    let text = joined_text(blocks);

    text.contains("开播时间")
        || text.contains("开播时长")
        || text.contains("直播时长")
        || text.contains("单场数据")
}

fn is_video_page(blocks: &[OcrTextBlock]) -> bool {
    let text = joined_text(blocks);

    text.contains("作品数据详情")
        || text.contains("切换作品")
        || text.contains("作品诊断")
        || blocks
            .iter()
            .any(|block| block.is_special && block.text.starts_with("播放量"))
}

fn extract_video_row(file_name: &str, blocks: &[OcrTextBlock]) -> VideoRow {
    VideoRow {
        file_name: file_name.to_owned(),
        play_count: extract_special_metric_value(blocks, "播放量")
            .or_else(|| extract_metric_value(blocks, &["播放量"]))
            .map(|value| strip_number_text(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(null_value),
        like_count: extract_metric_value(blocks, &["点赞量"])
            .map(|value| strip_number_text(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(null_value),
        comment_count: extract_metric_value(blocks, &["评论量"])
            .map(|value| strip_number_text(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(null_value),
    }
}

fn null_value() -> String {
    "<NULL>".to_owned()
}

fn extract_start_time(text: &str) -> Option<String> {
    ["开播时间：", "开播时间:", "开播时间"]
        .iter()
        .filter_map(|label| text.split_once(label).map(|(_, value)| value))
        .find_map(find_datetime)
        .or_else(|| find_datetime(text))
}

fn find_datetime(text: &str) -> Option<String> {
    let bytes = text.as_bytes();

    for index in 0..bytes.len().saturating_sub(9) {
        if !is_date_start(bytes, index) {
            continue;
        }

        let mut time_start = index + 10;
        while bytes.get(time_start).is_some_and(u8::is_ascii_whitespace) {
            time_start += 1;
        }

        let hour_start = time_start;
        let mut hour_end = hour_start;
        while bytes.get(hour_end).is_some_and(u8::is_ascii_digit) && hour_end - hour_start < 2 {
            hour_end += 1;
        }

        if hour_end == hour_start || bytes.get(hour_end) != Some(&b':') {
            continue;
        }

        let minute_start = hour_end + 1;
        let second_start = minute_start + 3;
        if bytes.get(minute_start + 2) != Some(&b':')
            || !is_two_digits(bytes, minute_start)
            || !is_two_digits(bytes, second_start)
        {
            continue;
        }

        let date = &text[index..index + 10];
        let hour = &text[hour_start..hour_end];
        let minute = &text[minute_start..minute_start + 2];
        let second = &text[second_start..second_start + 2];
        let hour = hour.parse::<u8>().ok()?;

        return Some(format!("{date} {hour:02}:{minute}:{second}"));
    }

    None
}

fn is_date_start(bytes: &[u8], index: usize) -> bool {
    is_four_digits(bytes, index)
        && bytes.get(index + 4) == Some(&b'-')
        && is_two_digits(bytes, index + 5)
        && bytes.get(index + 7) == Some(&b'-')
        && is_two_digits(bytes, index + 8)
}

fn is_four_digits(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index..index + 4)
        .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
}

fn is_two_digits(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index..index + 2)
        .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
}

fn extract_duration_seconds(text: &str) -> Option<u64> {
    [
        "开播时长：",
        "开播时长:",
        "开播时长",
        "直播时长：",
        "直播时长:",
    ]
    .iter()
    .filter_map(|label| text.split_once(label).map(|(_, value)| value))
    .find_map(find_duration_seconds)
    .or_else(|| find_duration_seconds(text))
}

fn find_duration_seconds(text: &str) -> Option<u64> {
    text.char_indices()
        .filter(|(_, ch)| ch.is_ascii_digit())
        .find_map(|(index, _)| parse_duration_seconds(&text[index..]))
}

fn joined_text(blocks: &[OcrTextBlock]) -> String {
    blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_metric_value(blocks: &[OcrTextBlock], labels: &[&str]) -> Option<String> {
    labels.iter().find_map(|label| {
        extract_metric_value_from_text(blocks, label)
            .or_else(|| extract_metric_value_by_position(blocks, label))
    })
}

fn extract_special_metric_value(blocks: &[OcrTextBlock], label: &str) -> Option<String> {
    blocks.iter().find_map(|block| {
        if !block.is_special {
            return None;
        }

        block
            .text
            .strip_prefix(label)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn extract_metric_value_from_text(blocks: &[OcrTextBlock], label: &str) -> Option<String> {
    blocks.iter().find_map(|block| {
        block
            .text
            .split_once(label)
            .map(|(_, value)| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn extract_metric_value_by_position(blocks: &[OcrTextBlock], label: &str) -> Option<String> {
    blocks
        .iter()
        .filter(|block| block.text.contains(label))
        .find_map(|label_block| {
            find_value_below_label(blocks, label_block)
                .or_else(|| find_value_right_of_label(blocks, label_block))
        })
}

fn find_value_below_label(blocks: &[OcrTextBlock], label: &OcrTextBlock) -> Option<String> {
    blocks
        .iter()
        .filter(|block| !block.is_special)
        .filter(|block| is_plain_metric_value(&block.text) || is_empty_metric_value(&block.text))
        .filter(|block| block.top >= label.top)
        .filter(|block| block.top - label.top <= 90)
        .filter(|block| same_metric_column(label, block))
        .min_by_key(|block| {
            (
                block.top - label.top,
                (block.left - label.left).abs(),
                block.left,
            )
        })
        .map(|block| block.text.clone())
}

fn find_value_right_of_label(blocks: &[OcrTextBlock], label: &OcrTextBlock) -> Option<String> {
    let label_right = label.left + label.width - 1;

    blocks
        .iter()
        .filter(|block| !block.is_special)
        .filter(|block| is_plain_metric_value(&block.text) || is_empty_metric_value(&block.text))
        .filter(|block| block.left > label_right)
        .filter(|block| block.left - label_right <= 260)
        .filter(|block| same_metric_row(label, block))
        .min_by_key(|block| {
            (
                (block_center_y(block) - block_center_y(label)).abs() as i32,
                block.left - label_right,
                block.top,
            )
        })
        .map(|block| block.text.clone())
}

fn same_metric_column(label: &OcrTextBlock, value: &OcrTextBlock) -> bool {
    let label_right = label.left + label.width - 1;
    let value_right = value.left + value.width - 1;
    let overlap_left = label.left.max(value.left);
    let overlap_right = label_right.min(value_right);
    let overlap = (overlap_right - overlap_left + 1).max(0) as f32;
    let min_width = label.width.min(value.width).max(1) as f32;

    (label.left - value.left).abs() <= 48 || overlap / min_width >= 0.35
}

fn same_metric_row(label: &OcrTextBlock, value: &OcrTextBlock) -> bool {
    let label_center_y = block_center_y(label);
    let value_center_y = block_center_y(value);
    let max_delta = label.height.max(value.height) as f32 * 0.45;

    (label_center_y - value_center_y).abs() <= max_delta
}

fn block_center_y(block: &OcrTextBlock) -> f32 {
    (block.top + block.height / 2) as f32
}

fn is_plain_metric_value(text: &str) -> bool {
    let text = text.trim();

    !text.is_empty()
        && !text.starts_with("较近")
        && text.chars().any(|ch| ch.is_ascii_digit())
        && text.chars().all(|ch| {
            ch.is_ascii_digit()
                || ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | '.'
                        | '%'
                        | '％'
                        | '+'
                        | '-'
                        | '−'
                        | '万'
                        | '元'
                        | '分'
                        | '钟'
                        | '小'
                        | '时'
                        | '秒'
                )
        })
}

fn is_empty_metric_value(text: &str) -> bool {
    matches!(text.trim(), "-" | "一")
}

fn parse_duration_seconds(duration: &str) -> Option<u64> {
    let duration = duration.trim();
    let mut seconds = 0u64;
    let mut number = String::new();
    let mut has_unit = false;

    for ch in duration.chars() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }

        if !number.is_empty()
            && (ch.is_whitespace()
                || matches!(
                    ch,
                    '%' | '％' | '同' | '级' | '别' | '主' | '播' | '数' | '据'
                ))
        {
            break;
        }

        if number.is_empty() {
            if has_unit && !matches!(ch, '小' | '时' | '分' | '钟' | '秒') {
                break;
            }
            continue;
        }

        let value = number.parse::<u64>().ok()?;
        number.clear();

        match ch {
            '小' => {
                seconds += value * 3600;
                has_unit = true;
            }
            '分' => {
                seconds += value * 60;
                has_unit = true;
            }
            '秒' => {
                seconds += value;
                has_unit = true;
            }
            _ => {}
        }
    }

    has_unit.then_some(seconds)
}

fn parse_count_to_integer(value: &str) -> Option<u64> {
    let value = value.trim().replace(',', "");

    if let Some(number) = value.strip_suffix('万') {
        let parsed = number.parse::<f64>().ok()?;
        return Some((parsed * 10_000.0).round() as u64);
    }

    strip_number_text(&value).parse::<u64>().ok()
}

fn strip_number_text(value: &str) -> String {
    let stripped = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_digit() || matches!(ch, ',' | '.'))
        .collect::<String>()
        .replace(',', "");

    if stripped.is_empty() && is_empty_metric_value(value) {
        "-".to_owned()
    } else {
        stripped
    }
}

fn write_live_csv(path: &Path, rows: &[LiveRow]) -> Result<()> {
    let mut csv = String::from("文件名,开播时间,直播时长,PV,累计观看人数,ACU\n");

    for row in rows {
        push_csv_row(
            &mut csv,
            [
                row.file_name.as_str(),
                row.start_time.as_str(),
                row.duration_seconds.as_str(),
                row.pv.as_str(),
                row.total_viewers.as_str(),
                row.acu.as_str(),
            ],
        );
    }

    fs::write(path, csv)?;
    Ok(())
}

fn write_video_csv(path: &Path, rows: &[VideoRow]) -> Result<()> {
    let mut csv = String::from("文件名,播放量,点赞量,评论量\n");

    for row in rows {
        push_csv_row(
            &mut csv,
            [
                row.file_name.as_str(),
                row.play_count.as_str(),
                row.like_count.as_str(),
                row.comment_count.as_str(),
            ],
        );
    }

    fs::write(path, csv)?;
    Ok(())
}

fn push_csv_row<'a>(csv: &mut String, fields: impl IntoIterator<Item = &'a str>) {
    let mut first = true;

    for field in fields {
        if !first {
            csv.push(',');
        }

        first = false;
        csv.push_str(&escape_csv_field(field));
    }

    csv.push('\n');
}

fn escape_csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}
