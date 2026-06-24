import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";

type ModelTier = "tiny" | "small" | "medium";

type ExportSummary = {
  liveCsvPath: string;
  videoCsvPath: string;
  imageCount: number;
  liveRowCount: number;
  videoRowCount: number;
};

type ExportEvent =
  | {
      kind: "modelDownload";
      modelTier: ModelTier;
      fileName: string;
      downloadedBytes: number;
      totalBytes: number | null;
      bytesPerSecond: number;
      finished: boolean;
    }
  | {
      kind: "image";
      current: number;
      total: number;
      imagePath: string;
      fileName: string;
      cacheHit: boolean;
    }
  | {
      kind: "complete";
      summary: ExportSummary;
    }
  | {
      kind: "error";
      message: string;
    }
  | {
      kind: "state";
      running: boolean;
      paused: boolean;
    };

type Project = {
  name: string;
  license: string;
  repo: string;
};

const projects: Project[] = [
  { name: "PaddleOCR", license: "Apache-2.0", repo: "https://github.com/PaddlePaddle/PaddleOCR" },
  { name: "ocr-rs", license: "Apache-2.0", repo: "https://github.com/zibo-chen/rust-paddle-ocr" },
  { name: "MNN", license: "Apache-2.0", repo: "https://github.com/alibaba/MNN" },
  { name: "Tauri", license: "Apache-2.0 或 MIT", repo: "https://github.com/tauri-apps/tauri" },
  { name: "Vite", license: "MIT", repo: "https://github.com/vitejs/vite" },
  { name: "TypeScript", license: "Apache-2.0", repo: "https://github.com/microsoft/TypeScript" },
  { name: "rfd", license: "MIT", repo: "https://github.com/PolyMeilex/rfd" },
  { name: "image", license: "MIT 或 Apache-2.0", repo: "https://github.com/image-rs/image" },
  { name: "anyhow", license: "MIT 或 Apache-2.0", repo: "https://github.com/dtolnay/anyhow" },
  { name: "ureq", license: "MIT 或 Apache-2.0", repo: "https://github.com/algesten/ureq" },
  { name: "Rust", license: "MIT 或 Apache-2.0", repo: "https://github.com/rust-lang/rust" },
];

const state = {
  folderPath: "",
  modelTier: "medium" as ModelTier,
  running: false,
  paused: false,
  showAbout: false,
  statusText: "请选择包含数字命名图片的目录",
  downloadText: "",
  progress: 0,
  logs: [] as string[],
};

const appRoot = document.querySelector<HTMLDivElement>("#app");

if (!appRoot) {
  throw new Error("missing #app");
}

const app = appRoot;
let renderQueued = false;

function render() {
  renderQueued = false;
  app.innerHTML = state.showAbout ? renderAbout() : renderMain();
  bindMainEvents();
  bindAboutEvents();
}

function scheduleRender() {
  if (renderQueued) {
    return;
  }

  renderQueued = true;
  requestAnimationFrame(render);
}

function renderMain() {
  const logs = state.logs.length > 0
    ? state.logs.map((line) => `<div class="log-line">${escapeHtml(line)}</div>`).join("")
    : `<div class="empty-log">等待开始导出</div>`;

  return `
    <main class="app-shell">
      <header class="topbar">
        <div>
          <h1>抖音登记OCR</h1>
          <p>批量识别抖音数据截图，整理直播与视频 CSV。</p>
        </div>
        <button class="ghost-button" data-action="about">关于</button>
      </header>

      <section class="control-band">
        <label class="field grow">
          <span>图片目录</span>
          <div class="path-row">
            <input readonly value="${escapeAttribute(state.folderPath)}" placeholder="选择包含数字命名图片的目录" />
            <button data-action="browse" ${state.running ? "disabled" : ""}>选择目录</button>
          </div>
        </label>

        <label class="field model-field">
          <span>模型</span>
          <select data-action="model" ${state.running ? "disabled" : ""}>
            ${renderModelOption("tiny")}
            ${renderModelOption("small")}
            ${renderModelOption("medium")}
          </select>
        </label>
      </section>

      <section class="action-row">
        <button class="primary-button" data-action="start" ${!state.folderPath || state.running ? "disabled" : ""}>开始导出</button>
        <button data-action="${state.paused ? "resume" : "pause"}" ${!state.running ? "disabled" : ""}>${state.paused ? "继续" : "暂停"}</button>
        <button data-action="stop" ${!state.running ? "disabled" : ""}>中止</button>
      </section>

      <section class="status-area">
        <div class="status-line">${escapeHtml(state.statusText)}</div>
        ${state.downloadText ? `<div class="download-line">${escapeHtml(state.downloadText)}</div>` : ""}
        <div class="progress-track">
          <div class="progress-fill" style="width: ${Math.round(state.progress * 100)}%"></div>
        </div>
      </section>

      <section class="log-panel">
        ${logs}
      </section>
    </main>
  `;
}

function renderAbout() {
  return `
    <main class="app-shell">
      <header class="topbar">
        <div>
          <h1>关于</h1>
          <p>抖音登记OCR 2.0，基于 Tauri v2 重构。</p>
        </div>
        <button class="ghost-button" data-action="back">返回</button>
      </header>

      <button class="developer-panel" data-url="https://github.com/isTrih">
        <strong>开发者：三氢</strong>
        <span>https://github.com/isTrih</span>
      </button>

      <section class="project-list">
        <div class="list-heading">
          <span>名称</span>
          <span>协议</span>
          <span>仓库</span>
        </div>
        ${projects.map(renderProject).join("")}
      </section>
    </main>
  `;
}

function renderProject(project: Project) {
  return `
    <button class="project-row" data-url="${escapeAttribute(project.repo)}">
      <span>${escapeHtml(project.name)}</span>
      <span>${escapeHtml(project.license)}</span>
      <span>${escapeHtml(project.repo)}</span>
    </button>
  `;
}

function renderModelOption(model: ModelTier) {
  return `<option value="${model}" ${state.modelTier === model ? "selected" : ""}>${model}</option>`;
}

function bindMainEvents() {
  app.querySelector<HTMLElement>('[data-action="about"]')?.addEventListener("click", () => {
    state.showAbout = true;
    render();
  });

  app.querySelector<HTMLElement>('[data-action="browse"]')?.addEventListener("click", selectFolder);
  app.querySelector<HTMLElement>('[data-action="start"]')?.addEventListener("click", startExport);
  app.querySelector<HTMLElement>('[data-action="pause"]')?.addEventListener("click", pauseExport);
  app.querySelector<HTMLElement>('[data-action="resume"]')?.addEventListener("click", resumeExport);
  app.querySelector<HTMLElement>('[data-action="stop"]')?.addEventListener("click", stopExport);

  app.querySelector<HTMLSelectElement>('[data-action="model"]')?.addEventListener("change", (event) => {
    state.modelTier = (event.currentTarget as HTMLSelectElement).value as ModelTier;
  });
}

function bindAboutEvents() {
  app.querySelector<HTMLElement>('[data-action="back"]')?.addEventListener("click", () => {
    state.showAbout = false;
    render();
  });

  app.querySelectorAll<HTMLElement>("[data-url]").forEach((element) => {
    element.addEventListener("click", async () => {
      const url = element.dataset.url;
      if (url) {
        try {
          await openUrl(url);
        } catch (error) {
          state.showAbout = false;
          state.statusText = `打开链接失败：${formatError(error)}`;
          appendLog(`打开链接失败：${url}`);
          render();
        }
      }
    });
  });
}

async function selectFolder() {
  const selected = await open({ directory: true, multiple: false });
  if (typeof selected !== "string") {
    return;
  }

  state.folderPath = selected;
  state.statusText = "目录已选择，点击开始导出";
  state.downloadText = "";
  state.progress = 0;
  state.logs = [];
  render();
}

async function startExport() {
  state.running = true;
  state.paused = false;
  state.statusText = "正在导出...";
  state.downloadText = "";
  state.progress = 0;
  state.logs = [];
  render();

  try {
    await invoke("start_export", {
      imageDir: state.folderPath,
      modelTier: state.modelTier,
    });
  } catch (error) {
    state.running = false;
    state.statusText = `导出启动失败：${formatError(error)}`;
    render();
  }
}

async function pauseExport() {
  try {
    await invoke("pause_export");
    state.paused = true;
    state.statusText = "已暂停，会在当前图片处理完成后停在下一张前";
  } catch (error) {
    state.statusText = `暂停失败：${formatError(error)}`;
  }
  render();
}

async function resumeExport() {
  try {
    await invoke("resume_export");
    state.paused = false;
    state.statusText = "继续导出...";
  } catch (error) {
    state.statusText = `继续失败：${formatError(error)}`;
  }
  render();
}

async function stopExport() {
  try {
    await invoke("stop_export");
    state.paused = false;
    state.statusText = "正在中止，会在当前图片处理完成后停止";
  } catch (error) {
    state.statusText = `中止失败：${formatError(error)}`;
  }
  render();
}

function handleExportEvent(event: ExportEvent) {
  switch (event.kind) {
    case "modelDownload":
      handleModelDownload(event);
      break;
    case "image":
      state.downloadText = "";
      state.progress = event.total > 0 ? event.current / event.total : 0;
      state.statusText = `正在处理 ${event.current}/${event.total}`;
      appendLog(`${event.current}/${event.total} ${event.fileName}${event.cacheHit ? "（缓存）" : ""}`);
      break;
    case "complete":
      state.running = false;
      state.paused = false;
      state.progress = 1;
      state.downloadText = "";
      state.statusText = `导出完成：${event.summary.imageCount} 张图片，直播 ${event.summary.liveRowCount} 行，视频 ${event.summary.videoRowCount} 行`;
      appendLog(`直播CSV：${event.summary.liveCsvPath}`);
      appendLog(`视频CSV：${event.summary.videoCsvPath}`);
      break;
    case "error":
      state.running = false;
      state.paused = false;
      state.downloadText = "";
      state.statusText = `导出失败：${event.message}`;
      appendLog(`错误：${event.message}`);
      break;
    case "state":
      state.running = event.running;
      state.paused = event.paused;
      break;
  }

  scheduleRender();
}

function handleModelDownload(event: Extract<ExportEvent, { kind: "modelDownload" }>) {
  state.statusText = `本地没有 ${event.modelTier} 模型，正在下载...`;
  state.progress = event.totalBytes ? clampProgress(event.downloadedBytes / event.totalBytes) : 0;
  const total = event.totalBytes ? formatBytes(event.totalBytes) : "未知大小";
  const percent = event.totalBytes ? `${((event.downloadedBytes * 100) / event.totalBytes).toFixed(1)}%` : "--";
  const speed = event.bytesPerSecond > 0 ? `${formatBytes(event.bytesPerSecond)}/s` : "计算中";
  state.downloadText = `正在下载 ${event.fileName}：${formatBytes(event.downloadedBytes)} / ${total}，${percent}，${speed}`;

  if (event.finished) {
    appendLog(`模型下载完成：${event.fileName}`);
  }
}

function appendLog(line: string) {
  state.logs = [...state.logs, line].slice(-200);
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${Math.max(0, Math.round(bytes))} B`;
}

function clampProgress(value: number) {
  if (!Number.isFinite(value)) {
    return 0;
  }

  return Math.min(1, Math.max(0, value));
}

function normalizeExportEvent(value: unknown): ExportEvent | null {
  const record = asRecord(value);
  if (!record) {
    return null;
  }

  const kind = stringValue(record.kind, "");
  switch (kind) {
    case "modelDownload":
      return {
        kind,
        modelTier: modelTierValue(read(record, "modelTier", "model_tier"), state.modelTier),
        fileName: stringValue(read(record, "fileName", "file_name"), "模型文件"),
        downloadedBytes: numberValue(read(record, "downloadedBytes", "downloaded_bytes"), 0),
        totalBytes: nullableNumberValue(read(record, "totalBytes", "total_bytes")),
        bytesPerSecond: numberValue(read(record, "bytesPerSecond", "bytes_per_second"), 0),
        finished: booleanValue(record.finished, false),
      };
    case "image":
      return {
        kind,
        current: numberValue(record.current, 0),
        total: numberValue(record.total, 0),
        imagePath: stringValue(read(record, "imagePath", "image_path"), ""),
        fileName: stringValue(read(record, "fileName", "file_name"), "未知图片"),
        cacheHit: booleanValue(read(record, "cacheHit", "cache_hit"), false),
      };
    case "complete": {
      const summary = asRecord(record.summary);
      if (!summary) {
        return null;
      }

      return {
        kind,
        summary: {
          liveCsvPath: stringValue(read(summary, "liveCsvPath", "live_csv_path"), ""),
          videoCsvPath: stringValue(read(summary, "videoCsvPath", "video_csv_path"), ""),
          imageCount: numberValue(read(summary, "imageCount", "image_count"), 0),
          liveRowCount: numberValue(read(summary, "liveRowCount", "live_row_count"), 0),
          videoRowCount: numberValue(read(summary, "videoRowCount", "video_row_count"), 0),
        },
      };
    }
    case "error":
      return {
        kind,
        message: stringValue(record.message, "未知错误"),
      };
    case "state":
      return {
        kind,
        running: booleanValue(record.running, false),
        paused: booleanValue(record.paused, false),
      };
    default:
      return null;
  }
}

function read(record: Record<string, unknown>, camelKey: string, snakeKey: string) {
  return record[camelKey] ?? record[snakeKey];
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" ? value as Record<string, unknown> : null;
}

function stringValue(value: unknown, fallback: string) {
  return typeof value === "string" && value.length > 0 ? value : fallback;
}

function numberValue(value: unknown, fallback: number) {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function nullableNumberValue(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function booleanValue(value: unknown, fallback: boolean) {
  return typeof value === "boolean" ? value : fallback;
}

function modelTierValue(value: unknown, fallback: ModelTier): ModelTier {
  return value === "tiny" || value === "small" || value === "medium" ? value : fallback;
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function escapeHtml(value: string) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function escapeAttribute(value: string) {
  return escapeHtml(value);
}

await listen<unknown>("export-event", (event) => {
  const normalized = normalizeExportEvent(event.payload);
  if (normalized) {
    handleExportEvent(normalized);
    return;
  }

  appendLog("收到无法识别的导出事件");
  scheduleRender();
});

render();
