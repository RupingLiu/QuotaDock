import { describe, expect, it } from "vitest";
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
  fiveHour: {
    remainingPercent: 72,
    resetAt: null,
    resetCountdownSeconds: 3600,
  },
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
    const appState: AppState = {
      version: 3,
      latestSnapshot: snapshot,
      storageStatus: "ready",
      storagePath: null,
      backupPath: null,
      statusMessage: "上次更新成功。",
      history: [],
      settings: {
        automaticUpdateChecks: true,
        lowQuotaNotifications: false,
      },
      recoveryNotice: null,
    };

    state.applyRefreshResult({
      appState,
      updated: false,
      message: "Codex CLI 查询失败。",
    });

    expect(state.appState).toEqual(appState);
    expect(state.errorMessage).toBe("Codex CLI 查询失败。");
    expect(state.noticeMessage).toBeNull();
  });
});
