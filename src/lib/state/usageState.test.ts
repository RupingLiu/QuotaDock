import { describe, expect, it, vi } from "vitest";
import type { QuotaDockApi } from "$lib/api/tauri";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";
import {
  capturedAtToEpochMs,
  isSnapshotStale,
  shouldRefreshOnForeground,
  UsageState,
} from "./usageState.svelte";

const snapshot: QuotaSnapshot = {
  id: "snap-1",
  source: "codex-cli",
  capturedAt: "unix:1000",
  weekly: {
    remainingPercent: 46,
    resetAt: null,
    resetCountdownSeconds: null,
  },
  planType: null,
  creditsBalance: null,
  resetCreditsAvailable: null,
  rawText: "",
  statusMessage: "updated",
  warnings: [],
};

function appStateAt(revision: number, statusMessage: string): AppState {
  return {
    version: 5,
    revision,
    providers: {
      codex: {
        configured: true,
        latestSnapshot: { provider: "codex", data: snapshot },
        lastAttemptAt: snapshot.capturedAt,
        health: "fresh",
        errorCategory: null,
      },
      deepseek: {
        configured: false,
        latestSnapshot: null,
        lastAttemptAt: null,
        health: "not-configured",
        errorCategory: null,
      },
      kimi: {
        configured: false,
        latestSnapshot: null,
        lastAttemptAt: null,
        health: "not-configured",
        errorCategory: null,
      },
    },
    latestSnapshot: snapshot,
    storageStatus: "ready",
    storagePath: null,
    backupPath: null,
    statusMessage,
    history: [],
    settings: {
      automaticUpdateChecks: true,
      lowQuotaNotifications: false,
      floatingProviderIds: ["codex"],
    },
    recoveryNotice: null,
  };
}

describe("foreground refresh freshness", () => {
  it("parses unix and ISO capturedAt values", () => {
    expect(capturedAtToEpochMs("unix:1000")).toBe(1_000_000);
    expect(capturedAtToEpochMs("2026-06-18T08:00:00Z")).toBe(
      Date.parse("2026-06-18T08:00:00Z"),
    );
    expect(capturedAtToEpochMs("not-a-date")).toBeNull();
  });

  it("treats missing or old snapshots as stale", () => {
    expect(isSnapshotStale(null, 1_000_000, 120_000)).toBe(true);
    expect(isSnapshotStale(snapshot, 1_130_001, 120_000)).toBe(true);
  });

  it("keeps fresh snapshots quiet", () => {
    expect(isSnapshotStale(snapshot, 1_060_000, 120_000)).toBe(false);
  });

  it("treats implausibly future-dated snapshots as stale", () => {
    expect(isSnapshotStale(snapshot, 600_000, 120_000)).toBe(true);
  });

  it("does not refresh while already busy", () => {
    expect(shouldRefreshOnForeground(snapshot, 1_130_001, 120_000, 0, true)).toBe(
      false,
    );
  });

  it("throttles repeated foreground refresh attempts", () => {
    expect(shouldRefreshOnForeground(snapshot, 1_130_001, 120_000, 1_100_000)).toBe(
      false,
    );
  });

  it("keeps the last snapshot but exposes unsuccessful refreshes as errors", () => {
    const state = new UsageState();
    const appState = appStateAt(1, "上次更新成功。");

    state.applyRefreshResult({
      appState,
      updated: false,
      message: "Codex CLI 查询失败。",
    });

    expect(state.appState).toEqual(appState);
    expect(state.errorMessage).toBe("Codex CLI 查询失败。");
    expect(state.noticeMessage).toBeNull();
  });

  it("checks every configured provider and ignores unconfigured providers", () => {
    const state = appStateAt(1, "fresh");
    expect(shouldRefreshOnForeground(state, 1_060_000, 120_000)).toBe(false);
    state.providers.deepseek.configured = true;
    state.providers.deepseek.health = "idle";
    expect(shouldRefreshOnForeground(state, 1_060_000, 120_000)).toBe(true);
  });

  it("foreground refresh invokes the provider-aware refresh command", async () => {
    const current = appStateAt(1, "old");
    const refreshProviders = vi.fn().mockResolvedValue({
      appState: appStateAt(2, "updated"),
      providerResults: [{ providerId: "codex", outcome: "updated", message: "updated", errorCategory: null }],
      anyUpdated: true,
      message: "updated",
    });
    const refreshUsage = vi.fn();
    const usage = new UsageState({ refreshProviders, refreshUsage } as unknown as QuotaDockApi);
    usage.applyAppState(current);
    const now = vi.spyOn(Date, "now").mockReturnValue(1_130_001);
    await usage.refreshIfStale(120_000);
    expect(refreshProviders).toHaveBeenCalledTimes(1);
    expect(refreshUsage).not.toHaveBeenCalled();
    now.mockRestore();
  });

  it("rejects out-of-order legacy and provider events by revision", () => {
    const state = new UsageState();
    const current = {
      version: 5,
      revision: 4,
      providers: {
        codex: {
          configured: true,
          latestSnapshot: { provider: "codex" as const, data: snapshot },
          lastAttemptAt: snapshot.capturedAt,
          health: "fresh" as const,
          errorCategory: null,
        },
        deepseek: {
          configured: false,
          latestSnapshot: null,
          lastAttemptAt: null,
          health: "not-configured" as const,
          errorCategory: null,
        },
        kimi: {
          configured: false,
          latestSnapshot: null,
          lastAttemptAt: null,
          health: "not-configured" as const,
          errorCategory: null,
        },
      },
      latestSnapshot: snapshot,
      storageStatus: "ready" as const,
      storagePath: null,
      backupPath: null,
      statusMessage: "current",
      history: [],
      settings: {
        automaticUpdateChecks: true,
        lowQuotaNotifications: false,
        floatingProviderIds: ["codex" as const],
      },
      recoveryNotice: null,
    };
    state.applyAppState(current);

    state.applyRefreshResult({
      appState: { ...current, revision: 2, statusMessage: "old legacy" },
      updated: true,
      message: "old legacy",
    });
    state.applyProviderResult({
      appState: { ...current, revision: 3, statusMessage: "old provider" },
      providerResults: [
        {
          providerId: "deepseek",
          outcome: "updated",
          message: "old provider",
          errorCategory: null,
        },
      ],
      anyUpdated: true,
      message: "old provider",
    });

    expect(state.appState?.revision).toBe(4);
    expect(state.appState?.statusMessage).toBe("current");
  });

  it("does not let a deferred load overwrite a newer event or its notice", async () => {
    let resolveLoad!: (value: AppState) => void;
    const deferredLoad = new Promise<AppState>((resolve) => {
      resolveLoad = resolve;
    });
    const state = new UsageState({
      getAppState: () => deferredLoad,
    } as QuotaDockApi);

    const load = state.load();
    state.applyProviderResult({
      appState: appStateAt(2, "newer Codex state"),
      providerResults: [
        {
          providerId: "codex",
          outcome: "failed",
          message: "Codex failed while loading",
          errorCategory: "network",
        },
      ],
      anyUpdated: false,
      message: "Codex failed while loading",
    });
    resolveLoad(appStateAt(1, "stale load notice"));
    await load;

    expect(state.appState?.revision).toBe(2);
    expect(state.appState?.statusMessage).toBe("newer Codex state");
    expect(state.errorMessage).toBe("Codex failed while loading");
    expect(state.noticeMessage).toBeNull();
  });

  it("keeps the Codex global error when a newer non-Codex success arrives", () => {
    const state = new UsageState();
    state.applyProviderResult({
      appState: appStateAt(5, "Codex failed state"),
      providerResults: [
        {
          providerId: "codex",
          outcome: "failed",
          message: "Codex refresh failed",
          errorCategory: "timeout",
        },
      ],
      anyUpdated: false,
      message: "Codex refresh failed",
    });
    state.applyProviderResult({
      appState: appStateAt(6, "DeepSeek updated state"),
      providerResults: [
        {
          providerId: "deepseek",
          outcome: "updated",
          message: "DeepSeek updated",
          errorCategory: null,
        },
      ],
      anyUpdated: true,
      message: "DeepSeek updated",
    });

    expect(state.appState?.revision).toBe(6);
    expect(state.errorMessage).toBe("Codex refresh failed");
    expect(state.noticeMessage).toBeNull();
    expect(state.providerAnnouncement).toBe("DeepSeek updated");
  });

  it("applies state-only detail responses without mutating legacy messages", () => {
    const state = new UsageState();
    state.applyProviderResult({
      appState: appStateAt(1, "Codex failed"),
      providerResults: [{
        providerId: "codex",
        outcome: "failed",
        message: "Codex refresh failed",
        errorCategory: "network",
      }],
      anyUpdated: false,
      message: "Codex refresh failed",
    });
    const applied = state.applyAppState(appStateAt(2, "DeepSeek command state"));
    expect(applied).toBe(true);
    expect(state.errorMessage).toBe("Codex refresh failed");
    expect(state.noticeMessage).toBeNull();
    expect(state.providerAnnouncement).toBe("Codex refresh failed");
  });

  it("announces provider failures independently from the Codex legacy error", () => {
    const state = new UsageState();
    state.errorMessage = "Codex previous failure";
    state.applyProviderResult({
      appState: appStateAt(3, "Kimi failed state"),
      providerResults: [{
        providerId: "kimi",
        outcome: "failed",
        message: "Kimi 查询超时。",
        errorCategory: "timeout",
      }],
      anyUpdated: false,
      message: "Kimi 查询超时。",
    });
    expect(state.providerAnnouncement).toBe("Kimi 查询超时。");
    expect(state.errorMessage).toBe("Codex previous failure");
  });
});
