import type { QuotaReading, SnapshotSource, StorageStatus } from "$lib/types/usage";

export function formatPercent(value: number | null): string {
  return typeof value === "number" ? `${value}%` : "--";
}

export function progressValue(value: number | null): number {
  if (typeof value !== "number") {
    return 0;
  }
  return Math.max(0, Math.min(100, value));
}

export function formatReset(reading: QuotaReading): string {
  if (reading.resetAt) {
    return formatDateTimeOrRaw(reading.resetAt);
  }
  if (typeof reading.resetCountdownSeconds === "number") {
    return `${formatDuration(reading.resetCountdownSeconds)}后`;
  }
  return "--";
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

export function sourceLabel(source: SnapshotSource | null | undefined): string {
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
  return `${minutes}分钟`;
}
