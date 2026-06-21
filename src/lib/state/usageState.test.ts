import { describe, expect, it } from "vitest";
import type { QuotaSnapshot } from "$lib/types/usage";
import {
  capturedAtToEpochMs,
  isSnapshotStale,
  shouldRefreshOnForeground,
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
});
