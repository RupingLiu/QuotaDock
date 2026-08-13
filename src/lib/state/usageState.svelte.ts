import type { QuotaDockApi } from "$lib/api/tauri";
import { tauriApi } from "$lib/api/tauri";
import type {
  AppState,
  QuotaSnapshot,
  RefreshProvidersResult,
  RefreshUsageResult,
} from "$lib/types/usage";
import { capturedAtToEpochMs } from "$lib/utils/format";

export const FOREGROUND_REFRESH_MAX_AGE_MS = 2 * 60 * 1000;
export const FUTURE_TIMESTAMP_TOLERANCE_MS = 5 * 60 * 1000;

export class UsageState {
  appState = $state<AppState | null>(null);
  loading = $state(false);
  refreshing = $state(false);
  errorMessage = $state<string | null>(null);
  noticeMessage = $state<string | null>(null);
  providerAnnouncement = $state<string | null>(null);
  private lastForegroundRefreshStartedAt = 0;

  constructor(private readonly api: QuotaDockApi = tauriApi) {}

  get activeSnapshot(): QuotaSnapshot | null {
    return this.appState?.latestSnapshot ?? null;
  }

  async load(): Promise<void> {
    await this.capture(async () => {
      this.loading = true;
      const appState = await this.api.getAppState();
      if (this.applyAppState(appState)) {
        this.noticeMessage = appState.statusMessage;
      }
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

  async refreshProviders(): Promise<void> {
    await this.capture(async () => {
      this.refreshing = true;
      try {
        this.applyProviderResult(await this.api.refreshProviders());
      } catch (error) {
        this.providerAnnouncement = error instanceof Error ? error.message : String(error);
        throw error;
      }
    }).finally(() => {
      this.refreshing = false;
    });
  }

  async refreshIfStale(maxAgeMs = FOREGROUND_REFRESH_MAX_AGE_MS): Promise<void> {
    const nowMs = Date.now();
    if (
      !shouldRefreshOnForeground(
        this.appState,
        nowMs,
        maxAgeMs,
        this.lastForegroundRefreshStartedAt,
        this.loading || this.refreshing,
      )
    ) {
      return;
    }

    this.lastForegroundRefreshStartedAt = nowMs;
    await this.refreshProviders();
  }

  applyRefreshResult(result: RefreshUsageResult): void {
    if (!this.applyAppState(result.appState)) return;
    if (result.updated) {
      this.errorMessage = null;
      this.noticeMessage = result.message;
    } else {
      this.errorMessage = result.message;
      this.noticeMessage = null;
    }
  }

  applyProviderResult(result: RefreshProvidersResult): void {
    if (!this.applyAppState(result.appState)) return;
    this.providerAnnouncement =
      result.providerResults.length === 1
        ? result.providerResults[0]?.message ?? result.message
        : result.message;
    const codex = result.providerResults.find(
      (providerResult) => providerResult.providerId === "codex",
    );
    if (!codex) return;
    if (codex.outcome === "failed") {
      this.errorMessage = codex.message;
      this.noticeMessage = result.anyUpdated ? result.message : null;
    } else {
      this.errorMessage = null;
      this.noticeMessage = result.message;
    }
  }

  applyAppState(next: AppState): boolean {
    if (this.appState && next.revision < this.appState.revision) return false;
    this.appState = next;
    return true;
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
  state: AppState | QuotaSnapshot | null,
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
  if (state && "providers" in state) {
    return (["codex", "deepseek", "kimi"] as const).some((providerId) => {
      const provider = state.providers[providerId];
      if (!provider.configured) return false;
      const snapshot = provider.latestSnapshot;
      return !snapshot || isCapturedAtStale(snapshot.data.capturedAt, nowMs, maxAgeMs);
    });
  }
  return isSnapshotStale(state, nowMs, maxAgeMs);
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
  const ageMs = nowMs - capturedAtMs;
  return ageMs > maxAgeMs || ageMs < -FUTURE_TIMESTAMP_TOLERANCE_MS;
}

function isCapturedAtStale(
  capturedAt: string | null | undefined,
  nowMs: number,
  maxAgeMs: number,
): boolean {
  const capturedAtMs = capturedAtToEpochMs(capturedAt);
  if (capturedAtMs === null) return true;
  const ageMs = nowMs - capturedAtMs;
  return ageMs > maxAgeMs || ageMs < -FUTURE_TIMESTAMP_TOLERANCE_MS;
}

export { capturedAtToEpochMs };
