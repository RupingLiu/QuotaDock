import type {
  ProviderErrorCategory,
  ProviderHealth,
  QuotaReading,
  SnapshotSource,
  StorageStatus,
} from "$lib/types/usage";

export type ResetFormatOptions = {
  capturedAt?: string | null;
  nowMs?: number;
};

export function formatPercent(value: number | null): string {
  return typeof value === "number" ? `${value}%` : "--";
}

export function progressValue(value: number | null): number {
  if (typeof value !== "number") {
    return 0;
  }
  return Math.max(0, Math.min(100, value));
}

export function formatBalance(amount: string | null | undefined, currency: string): string {
  if (amount === null || amount === undefined || amount === "") return "--";
  const prefix = currency === "CNY" ? "¥" : currency === "USD" ? "$" : `${currency} `;
  return `${prefix}${amount}`;
}

export function providerHealthLabel(health: ProviderHealth): string {
  switch (health) {
    case "fresh":
      return "已更新";
    case "refreshing":
      return "读取中";
    case "stale":
      return "数据陈旧";
    case "error":
      return "刷新失败";
    case "not-configured":
      return "未配置";
    default:
      return "等待更新";
  }
}

export function providerErrorLabel(category: ProviderErrorCategory | null): string | null {
  switch (category) {
    case "busy":
      return "已有查询正在进行";
    case "unauthorized":
      return "API Key 无效或无权访问";
    case "insufficient-balance":
      return "账户余额不足或已停用";
    case "rate-limited":
      return "请求过于频繁，请稍后重试";
    case "timeout":
      return "查询超时";
    case "network":
      return "网络连接失败";
    case "server":
      return "供应商服务暂时不可用";
    case "invalid-response":
      return "供应商返回了无法识别的数据";
    case "credential-store":
      return "Windows 凭据存储不可用";
    case "not-configured":
      return "尚未配置 API Key";
    default:
      return null;
  }
}

export function formatReset(
  reading: QuotaReading,
  options: ResetFormatOptions = {},
): string {
  if (reading.resetAt) {
    return formatDateTimeOrRaw(reading.resetAt);
  }
  if (typeof reading.resetCountdownSeconds === "number") {
    const remainingSeconds = liveCountdownSeconds(
      reading.resetCountdownSeconds,
      options.capturedAt,
      options.nowMs,
    );
    if (remainingSeconds === 0) {
      return "待刷新";
    }
    return `${formatDuration(remainingSeconds)}后`;
  }
  return "--";
}

export function liveCountdownSeconds(
  capturedCountdownSeconds: number,
  capturedAt?: string | null,
  nowMs = Date.now(),
): number {
  const safeCountdown = Math.max(0, Math.floor(capturedCountdownSeconds));
  const capturedAtMs = capturedAtToEpochMs(capturedAt);
  if (capturedAtMs === null || nowMs <= capturedAtMs) {
    return safeCountdown;
  }

  const elapsedSeconds = Math.floor((nowMs - capturedAtMs) / 1000);
  return Math.max(0, safeCountdown - elapsedSeconds);
}

export function formatCapturedAt(value: string | null | undefined): string {
  if (!value) {
    return "尚未更新";
  }
  if (value.startsWith("unix:")) {
    const seconds = Number(value.slice(5));
    if (Number.isFinite(seconds)) {
      return formatDate(new Date(seconds * 1000));
    }
  }
  return formatDateTimeOrRaw(value);
}

export function capturedAtToEpochMs(
  capturedAt: string | null | undefined,
): number | null {
  if (!capturedAt) {
    return null;
  }
  if (capturedAt.startsWith("unix:")) {
    const rawSeconds = capturedAt.slice("unix:".length).trim();
    if (!/^\d+(?:\.\d+)?$/.test(rawSeconds)) {
      return null;
    }
    const seconds = Number(rawSeconds);
    return Number.isFinite(seconds) ? seconds * 1000 : null;
  }

  const parsed = Date.parse(capturedAt);
  return Number.isNaN(parsed) ? null : parsed;
}

export function sourceLabel(source: SnapshotSource | null | undefined): string {
  if (source === "codex-app-server") {
    return "Codex App Server";
  }
  if (source === "codex-cli") {
    return "Codex CLI";
  }
  if (source === "pasted-status") {
    return "本地数据";
  }
  return "未连接";
}

export function storageLabel(status: StorageStatus | null | undefined): string {
  switch (status) {
    case "ready":
      return "本地已保存";
    case "missing":
      return "等待首次更新";
    case "recovered":
      return "已恢复存储";
    case "unsupported-version":
      return "已重建存储";
    default:
      return "加载中";
  }
}

function formatDateTimeOrRaw(value: string): string {
  if (value.startsWith("unix:")) {
    const epochMs = capturedAtToEpochMs(value);
    return epochMs === null ? value : formatDate(new Date(epochMs));
  }
  const codexReset = formatCodexResetDate(value);
  if (codexReset) {
    return codexReset;
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return formatDate(date);
}

function formatDate(date: Date): string {
  return `${date.getMonth() + 1}月${date.getDate()}日 ${formatTime(
    date.getHours(),
    date.getMinutes(),
  )}`;
}

function formatCodexResetDate(value: string): string | null {
  const match = value
    .trim()
    .match(/^(\d{1,2}):(\d{2})\s+on\s+(\d{1,2})\s+([a-z]{3,9})$/i);
  if (!match) {
    return null;
  }

  const [, hour, minute, day, monthName] = match;
  const month = monthNumber(monthName);
  if (!month) {
    return null;
  }

  return `${month}月${Number(day)}日 ${formatTime(Number(hour), Number(minute))}`;
}

function monthNumber(value: string): number | null {
  const month = value.slice(0, 3).toLowerCase();
  const index = [
    "jan",
    "feb",
    "mar",
    "apr",
    "may",
    "jun",
    "jul",
    "aug",
    "sep",
    "oct",
    "nov",
    "dec",
  ].indexOf(month);
  return index >= 0 ? index + 1 : null;
}

function formatTime(hour: number, minute: number): string {
  return `${hour.toString().padStart(2, "0")}:${minute
    .toString()
    .padStart(2, "0")}`;
}

function formatDuration(seconds: number): string {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const days = Math.floor(safeSeconds / 86_400);
  const hours = Math.floor((safeSeconds % 86_400) / 3_600);
  const minutes = Math.floor((safeSeconds % 3_600) / 60);

  if (days > 0) {
    return hours > 0 ? `${days}天${hours}小时` : `${days}天`;
  }
  if (hours > 0) {
    return minutes > 0 ? `${hours}小时${minutes}分钟` : `${hours}小时`;
  }
  if (safeSeconds > 0 && minutes === 0) {
    return "<1分钟";
  }
  return `${minutes}分钟`;
}
