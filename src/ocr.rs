use anyhow::Result;
use image::DynamicImage;
use ocr_rs::{OcrEngine, OcrEngineConfig, OcrResult_};
use std::path::{Path, PathBuf};

pub const DEFAULT_DET_MODEL_PATH: &str = "models/PP-OCRv6_medium_det.mnn";
pub const DEFAULT_REC_MODEL_PATH: &str = "models/PP-OCRv6_medium_rec.mnn";
pub const DEFAULT_CHARSET_PATH: &str = "models/ppocr_keys_v6_medium.txt";

pub const DEFAULT_MODEL_CDN_BASE_URL: &str = "https://assets.checkpoint321.com/wjm/models/";

const MERGE_MAX_HORIZONTAL_GAP: i32 = 24;
const MERGE_MAX_CENTER_Y_DELTA_RATIO: f32 = 0.45;
const MERGE_MIN_VERTICAL_OVERLAP_RATIO: f32 = 0.55;
const METRIC_VALUE_MAX_TOP_DELTA: i32 = 72;
const METRIC_VALUE_MAX_LEFT_DELTA: i32 = 36;
const METRIC_VALUE_MIN_X_OVERLAP_RATIO: f32 = 0.45;
const METRIC_VALUE_MAX_TOP_DELTA_RATIO: f32 = 1.05;
const SPECIAL_VALUE_MAX_HORIZONTAL_GAP: i32 = 180;
const SPECIAL_VALUE_MAX_CENTER_Y_DELTA_RATIO: f32 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrModelTier {
    Tiny,
    Small,
    Medium,
}

impl OcrModelTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Small => "small",
            Self::Medium => "medium",
        }
    }

    pub fn from_index(index: i32) -> Self {
        match index {
            0 => Self::Tiny,
            1 => Self::Small,
            _ => Self::Medium,
        }
    }

    pub fn det_file_name(self) -> String {
        format!("PP-OCRv6_{}_det.mnn", self.as_str())
    }

    pub fn rec_file_name(self) -> String {
        format!("PP-OCRv6_{}_rec.mnn", self.as_str())
    }

    pub fn charset_file_name(self) -> String {
        format!("ppocr_keys_v6_{}.txt", self.as_str())
    }
}

impl Default for OcrModelTier {
    fn default() -> Self {
        Self::Medium
    }
}

#[derive(Debug, Clone)]
pub struct OcrClientConfig {
    pub det_model_path: PathBuf,
    pub rec_model_path: PathBuf,
    pub charset_path: PathBuf,
    pub engine_config: OcrEngineConfig,
}

impl Default for OcrClientConfig {
    fn default() -> Self {
        Self::for_tier_in_dir(OcrModelTier::Medium, "models")
    }
}

impl OcrClientConfig {
    pub fn for_tier_in_dir(tier: OcrModelTier, model_dir: impl AsRef<Path>) -> Self {
        let model_dir = model_dir.as_ref();

        Self {
            det_model_path: model_dir.join(tier.det_file_name()),
            rec_model_path: model_dir.join(tier.rec_file_name()),
            charset_path: model_dir.join(tier.charset_file_name()),
            engine_config: OcrEngineConfig::fast().with_min_result_confidence(0.5),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OcrTextBlock {
    pub text: String,
    pub confidence: f32,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub is_special: bool,
}

impl OcrTextBlock {
    fn from_merged(result: MergedOcrResult) -> Self {
        let width = result.width();
        let height = result.height();

        Self {
            text: result.text,
            confidence: result.confidence,
            left: result.left,
            top: result.top,
            width,
            height,
            is_special: result.is_special,
        }
    }
}

pub struct OcrClient {
    engine: OcrEngine,
}

impl OcrClient {
    pub fn new(config: OcrClientConfig) -> Result<Self> {
        ensure_exists(&config.det_model_path)?;
        ensure_exists(&config.rec_model_path)?;
        ensure_exists(&config.charset_path)?;

        let engine = OcrEngine::new(
            &config.det_model_path,
            &config.rec_model_path,
            &config.charset_path,
            Some(config.engine_config),
        )?;

        Ok(Self { engine })
    }

    pub fn new_default() -> Result<Self> {
        Self::new(OcrClientConfig::default())
    }

    pub fn recognize_path(&self, image_path: impl AsRef<Path>) -> Result<Vec<OcrTextBlock>> {
        let image_path = image_path.as_ref();
        ensure_exists(image_path)?;

        let image = image::open(image_path)?;
        self.recognize_image(&image)
    }

    pub fn recognize_buffer(&self, buffer: &[u8]) -> Result<Vec<OcrTextBlock>> {
        let image = image::load_from_memory(buffer)?;
        self.recognize_image(&image)
    }

    pub fn recognize_image(&self, image: &DynamicImage) -> Result<Vec<OcrTextBlock>> {
        let results = self.engine.recognize(image)?;
        Ok(postprocess_results(results)
            .into_iter()
            .map(OcrTextBlock::from_merged)
            .collect())
    }
}

#[derive(Debug, Clone)]
struct MergedOcrResult {
    text: String,
    confidence: f32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    is_special: bool,
}

impl MergedOcrResult {
    fn from_ocr_result(result: OcrResult_) -> Self {
        let rect = result.bbox.rect;

        Self {
            text: normalize_joined_text(&result.text),
            confidence: result.confidence,
            left: rect.left(),
            top: rect.top(),
            right: rect.right(),
            bottom: rect.bottom(),
            is_special: false,
        }
    }

    fn width(&self) -> i32 {
        self.right - self.left + 1
    }

    fn height(&self) -> i32 {
        self.bottom - self.top + 1
    }

    fn center_y(&self) -> f32 {
        (self.top + self.bottom) as f32 / 2.0
    }

    fn x_overlap_ratio(&self, next: &Self) -> f32 {
        let overlap_left = self.left.max(next.left);
        let overlap_right = self.right.min(next.right);
        let overlap = (overlap_right - overlap_left + 1).max(0) as f32;
        let min_width = self.width().min(next.width()) as f32;

        overlap / min_width
    }

    fn merge_with(&mut self, next: &Self) {
        let self_area = (self.width() * self.height()) as f32;
        let next_area = (next.width() * next.height()) as f32;

        self.text.push_str(&next.text);
        self.text = normalize_joined_text(&self.text);
        self.confidence = ((self.confidence * self_area) + (next.confidence * next_area))
            / (self_area + next_area);
        self.left = self.left.min(next.left);
        self.top = self.top.min(next.top);
        self.right = self.right.max(next.right);
        self.bottom = self.bottom.max(next.bottom);
    }

    fn merge_special_with(&mut self, next: &Self) {
        self.merge_with(next);
        self.is_special = true;
    }
}

fn ensure_exists(path: &Path) -> Result<()> {
    anyhow::ensure!(
        path.exists(),
        "missing required file: {}\n\
         ocr-rs 2.3.x uses MNN runtime models. Put the matching .mnn model and charset files under models/.",
        path.display()
    );
    Ok(())
}

fn postprocess_results(results: Vec<OcrResult_>) -> Vec<MergedOcrResult> {
    merge_special_horizontal_values(merge_nearby_metric_values(merge_close_horizontal_results(
        results,
    )))
}

fn normalize_joined_text(text: &str) -> String {
    let chars = text.trim().chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(text.len());

    for (index, ch) in chars.iter().enumerate() {
        if ch.is_whitespace() {
            let prev = index.checked_sub(1).and_then(|prev| chars.get(prev));
            let next = chars.get(index + 1);

            if next.is_some_and(|next| matches!(next, '%' | '％' | '+' | '-' | '−'))
                || prev.is_some_and(|prev| matches!(prev, '+' | '-' | '−'))
            {
                continue;
            }

            if !normalized.ends_with(' ') {
                normalized.push(' ');
            }

            continue;
        }

        normalized.push(*ch);
    }

    normalized
}

fn is_metric_value_text(text: &str) -> bool {
    let text = text.trim();

    !text.starts_with("较近")
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

fn should_merge_horizontal(current: &MergedOcrResult, next: &MergedOcrResult) -> bool {
    let horizontal_gap = next.left - current.right - 1;
    if !(0..=MERGE_MAX_HORIZONTAL_GAP).contains(&horizontal_gap) {
        return false;
    }

    let center_delta = (current.center_y() - next.center_y()).abs();
    let max_center_delta =
        current.height().max(next.height()) as f32 * MERGE_MAX_CENTER_Y_DELTA_RATIO;
    if center_delta > max_center_delta {
        return false;
    }

    let overlap_top = current.top.max(next.top);
    let overlap_bottom = current.bottom.min(next.bottom);
    let vertical_overlap = (overlap_bottom - overlap_top + 1).max(0) as f32;
    let min_height = current.height().min(next.height()) as f32;

    vertical_overlap / min_height >= MERGE_MIN_VERTICAL_OVERLAP_RATIO
}

fn merge_close_horizontal_results(results: Vec<OcrResult_>) -> Vec<MergedOcrResult> {
    let mut items: Vec<_> = results
        .into_iter()
        .map(MergedOcrResult::from_ocr_result)
        .collect();

    sort_reading_order(&mut items);

    let mut merged = Vec::<MergedOcrResult>::new();

    for item in items {
        if let Some(current) = merged.last_mut()
            && should_merge_horizontal(current, &item)
        {
            current.merge_with(&item);
            continue;
        }

        merged.push(item);
    }

    merged
}

fn should_merge_metric_value(label: &MergedOcrResult, value: &MergedOcrResult) -> bool {
    if !is_metric_value_text(&value.text) {
        return false;
    }

    let top_delta = value.top - label.top;
    if !(0..=METRIC_VALUE_MAX_TOP_DELTA).contains(&top_delta) {
        return false;
    }

    let max_relative_delta =
        label.height().max(value.height()) as f32 * METRIC_VALUE_MAX_TOP_DELTA_RATIO;
    if top_delta as f32 > max_relative_delta {
        return false;
    }

    (label.left - value.left).abs() <= METRIC_VALUE_MAX_LEFT_DELTA
        || label.x_overlap_ratio(value) >= METRIC_VALUE_MIN_X_OVERLAP_RATIO
}

fn merge_nearby_metric_values(results: Vec<MergedOcrResult>) -> Vec<MergedOcrResult> {
    let mut items = results;
    items.sort_by(|a, b| a.top.cmp(&b.top).then(a.left.cmp(&b.left)));

    let mut merged = Vec::with_capacity(items.len());
    let mut used = vec![false; items.len()];

    for index in 0..items.len() {
        if used[index] {
            continue;
        }

        let mut item = items[index].clone();

        if !is_metric_value_text(&item.text) {
            let value_index = ((index + 1)..items.len())
                .filter(|candidate_index| !used[*candidate_index])
                .filter(|candidate_index| {
                    should_merge_metric_value(&item, &items[*candidate_index])
                })
                .min_by_key(|candidate_index| {
                    (
                        items[*candidate_index].top - item.top,
                        (items[*candidate_index].left - item.left).abs(),
                    )
                });

            if let Some(value_index) = value_index {
                item.merge_with(&items[value_index]);
                used[value_index] = true;
            }
        }

        used[index] = true;
        merged.push(item);
    }

    sort_reading_order(&mut merged);

    merged
}

fn should_merge_special_horizontal(label: &MergedOcrResult, value: &MergedOcrResult) -> bool {
    if label.is_special || value.is_special || is_metric_value_text(&label.text) {
        return false;
    }

    if !is_metric_value_text(&value.text) {
        return false;
    }

    let horizontal_gap = value.left - label.right - 1;
    if !(0..=SPECIAL_VALUE_MAX_HORIZONTAL_GAP).contains(&horizontal_gap) {
        return false;
    }

    let center_delta = (label.center_y() - value.center_y()).abs();
    let max_center_delta =
        label.height().max(value.height()) as f32 * SPECIAL_VALUE_MAX_CENTER_Y_DELTA_RATIO;

    center_delta <= max_center_delta
}

fn merge_special_horizontal_values(results: Vec<MergedOcrResult>) -> Vec<MergedOcrResult> {
    let mut items = results;
    items.sort_by(|a, b| a.top.cmp(&b.top).then(a.left.cmp(&b.left)));

    let mut merged = Vec::with_capacity(items.len());
    let mut used = vec![false; items.len()];

    for index in 0..items.len() {
        if used[index] {
            continue;
        }

        let mut item = items[index].clone();

        let value_index = ((index + 1)..items.len())
            .filter(|candidate_index| !used[*candidate_index])
            .filter(|candidate_index| {
                should_merge_special_horizontal(&item, &items[*candidate_index])
            })
            .min_by_key(|candidate_index| {
                (
                    (items[*candidate_index].center_y() - item.center_y()).abs() as i32,
                    items[*candidate_index].left - item.right,
                )
            });

        if let Some(value_index) = value_index {
            item.merge_special_with(&items[value_index]);
            used[value_index] = true;
        }

        used[index] = true;
        merged.push(item);
    }

    sort_reading_order(&mut merged);

    merged
}

fn sort_reading_order(items: &mut [MergedOcrResult]) {
    items.sort_by_key(|item| (item.top, item.left, item.bottom, item.right));

    let mut rows = Vec::<Vec<MergedOcrResult>>::new();

    for item in items.iter().cloned() {
        if let Some(row) = rows.last_mut() {
            let row_center_y =
                row.iter().map(MergedOcrResult::center_y).sum::<f32>() / row.len() as f32;
            let row_height = row.iter().map(MergedOcrResult::height).max().unwrap_or(1);
            let max_delta = row_height.max(item.height()) as f32 * MERGE_MAX_CENTER_Y_DELTA_RATIO;

            if (item.center_y() - row_center_y).abs() <= max_delta {
                row.push(item);
                continue;
            }
        }

        rows.push(vec![item]);
    }

    let mut write_index = 0;
    for row in &mut rows {
        row.sort_by_key(|item| (item.left, item.top, item.right, item.bottom));

        for item in row.drain(..) {
            items[write_index] = item;
            write_index += 1;
        }
    }
}
