import { describe, expect, it } from "vitest";
import { evaluateSnapshotFreshness } from "$lib/utils/freshness";
import type { Settings, UsageSnapshot } from "$lib/types/usage";

const settings: Settings = {
  staleAfterMinutes: 60,
  notifyBelowPercent: [20, 10],
  clipboardMonitoring: false,
};

const baseSnapshot: UsageSnapshot = {
  id: "snapshot-1",
  source: "pasted-status",
  parsedAt: "2026-06-18T08:00:00.000Z",
  remainingPercent: 42,
  resetAt: "2026-06-18T12:00:00.000Z",
  resetCountdownSeconds: 14400,
  creditsBalance: null,
  model: "gpt-5",
  contextWindow: null,
  confidence: "fresh",
  rawText: "status",
  manualFields: [],
  warnings: [],
  notes: "",
};

describe("evaluateSnapshotFreshness", () => {
  it("reports unavailable when no snapshot exists", () => {
    expect(evaluateSnapshotFreshness(null, settings).state).toBe("unavailable");
  });

  it("keeps recent parsed snapshots fresh", () => {
    const result = evaluateSnapshotFreshness(
      baseSnapshot,
      settings,
      new Date("2026-06-18T08:30:00.000Z"),
    );

    expect(result.state).toBe("fresh");
    expect(result.ageMinutes).toBe(30);
  });

  it("marks old parsed snapshots stale", () => {
    const result = evaluateSnapshotFreshness(
      baseSnapshot,
      settings,
      new Date("2026-06-18T09:10:00.000Z"),
    );

    expect(result.state).toBe("stale");
    expect(result.isStale).toBe(true);
  });

  it("parses runtime unix timestamps from Rust commands", () => {
    const result = evaluateSnapshotFreshness(
      { ...baseSnapshot, parsedAt: "unix:1781769600" },
      settings,
      new Date("2026-06-18T08:30:00.000Z"),
    );

    expect(result.state).toBe("fresh");
    expect(result.ageMinutes).toBe(30);
  });

  it("preserves partial and manual states instead of pretending they are live data", () => {
    const partial = evaluateSnapshotFreshness(
      { ...baseSnapshot, confidence: "partial" },
      settings,
      new Date("2026-06-18T08:10:00.000Z"),
    );
    const manual = evaluateSnapshotFreshness(
      { ...baseSnapshot, confidence: "manual", source: "manual" },
      settings,
      new Date("2026-06-18T10:10:00.000Z"),
    );

    expect(partial.state).toBe("partial");
    expect(manual.state).toBe("manual");
  });
});
