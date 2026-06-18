export function formatPercent(value: number | null): string {
  return value === null ? "No value" : `${value}%`;
}

export function formatCredits(value: number | null): string {
  if (value === null) {
    return "No value";
  }
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 0,
    maximumFractionDigits: 2,
  }).format(value);
}

export function formatDateTime(value: string | null): string {
  if (!value) {
    return "No reset";
  }
  const date = value.startsWith("unix:")
    ? new Date(Number(value.slice("unix:".length)) * 1000)
    : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString([], {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null) {
    return "No countdown";
  }
  const safeSeconds = Math.max(0, seconds);
  const hours = Math.floor(safeSeconds / 3600);
  const minutes = Math.floor((safeSeconds % 3600) / 60);
  if (hours === 0) {
    return `${minutes}m`;
  }
  return `${hours}h ${minutes}m`;
}
