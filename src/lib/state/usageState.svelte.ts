import type { QuotaDockApi } from "$lib/api/tauri";
import { tauriApi } from "$lib/api/tauri";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";

export const FOREGROUND_REFRESH_MAX_AGE_MS = 2 * 60 * 1000;

export class UsageState {
  appState = $state<AppState | null>(null);
  loading = $state(false);
  refreshing = $state(false);
  errorMessage = $state<string | null>(null);
  noticeMessage = $state<string | null>(null);
  private lastForegroundRefreshStartedAt = 0;

  constructor(private readonly api: QuotaDockApi = tauriApi) {}

  get activeSnapshot(): QuotaSnapshot | null {
    return this.appState?.latestSnapshot ?? null;
  }

  async load(): Promise<void> {
    await this.capture(async () => {
      this.loading = true;
      this.appState = await this.api.getAppState();
      this.noticeMessage = this.appState.statusMessage;
    }).finally(() => {
      this.loading = false;
    });
  }

  async refreshUsage(): Promise<void> {
    await this.capture(async () => {
      this.refreshing = true;
      const result = await this.api.refreshUsage();
      this.applyRefreshResult(result);
    }).finally(() => {
      this.refreshing = false;
    });
  }

  async refreshIfStale(maxAgeMs = FOREGROUND_REFRESH_MAX_AGE_MS): Promise<void> {
    const nowMs = Date.now();
    if (
      !shouldRefreshOnForeground(
        this.activeSnapshot,
        nowMs,
        maxAgeMs,
        this.lastForegroundRefreshStartedAt,
        this.loading || this.refreshing,
      )
    ) {
      return;
    }

    this.lastForegroundRefreshStartedAt = nowMs;
    await this.refreshUsage();
  }

  applyRefreshResult(result: { appState: AppState; message: string }): void {
    this.errorMessage = null;
    this.appState = result.appState;
    this.noticeMessage = result.message;
  }

  private async capture(work: () => Promise<void>): Promise<void> {
    this.errorMessage = null;
    try {
      await work();
    } catch (error) {
      this.errorMessage = error instanceof Error ? error.message : String(error);
    }
  }
}

export function createUsageState(api?: QuotaDockApi): UsageState {
  return new UsageState(api);
}

export function shouldRefreshOnForeground(
  snapshot: QuotaSnapshot | null,
  nowMs: number,
  maxAgeMs = FOREGROUND_REFRESH_MAX_AGE_MS,
  lastRefreshStartedAtMs = 0,
  busy = false,
): boolean {
  if (busy) {
    return false;
  }
  if (
    lastRefreshStartedAtMs > 0 &&
    nowMs - lastRefreshStartedAtMs < maxAgeMs
  ) {
    return false;
  }
  return isSnapshotStale(snapshot, nowMs, maxAgeMs);
}

export function isSnapshotStale(
  snapshot: QuotaSnapshot | null,
  nowMs: number,
  maxAgeMs = FOREGROUND_REFRESH_MAX_AGE_MS,
): boolean {
  if (!snapshot) {
    return true;
  }

  const capturedAtMs = capturedAtToEpochMs(snapshot.capturedAt);
  if (capturedAtMs === null) {
    return true;
  }
  return nowMs - capturedAtMs > maxAgeMs;
}

export function capturedAtToEpochMs(capturedAt: string): number | null {
  if (capturedAt.startsWith("unix:")) {
    const seconds = Number(capturedAt.slice("unix:".length));
    return Number.isFinite(seconds) ? seconds * 1000 : null;
  }

  const parsed = Date.parse(capturedAt);
  return Number.isNaN(parsed) ? null : parsed;
}
