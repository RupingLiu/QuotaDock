import { describe, expect, it } from "vitest";
import { formatCapturedAt, formatPercent, formatReset, progressValue } from "$lib/utils/format";

describe("format utilities", () => {
  it("formats unknown and known percentages", () => {
    expect(formatPercent(null)).toBe("--");
    expect(formatPercent(72)).toBe("72%");
  });

  it("clamps progress values", () => {
    expect(progressValue(null)).toBe(0);
    expect(progressValue(120)).toBe(100);
    expect(progressValue(-1)).toBe(0);
  });

  it("formats countdown reset text in Chinese", () => {
    expect(
      formatReset({
        remainingPercent: 72,
        resetAt: null,
        resetCountdownSeconds: 8100,
      }),
    ).toBe("2小时15分钟后");
  });

  it("formats Codex English reset dates in Chinese", () => {
    expect(
      formatReset({
        remainingPercent: 31,
        resetAt: "07:00 on 25 Jun",
        resetCountdownSeconds: null,
      }),
    ).toBe("6月25日 07:00");
  });

  it("formats ISO reset dates in Chinese", () => {
    expect(
      formatReset({
        remainingPercent: 46,
        resetAt: "2026-06-23T09:00:00Z",
        resetCountdownSeconds: null,
      }),
    ).toContain("6月23日");
  });

  it("formats unix capture times", () => {
    expect(formatCapturedAt("unix:1781769600")).not.toBe("尚未更新");
  });
});
