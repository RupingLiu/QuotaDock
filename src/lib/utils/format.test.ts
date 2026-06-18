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

  it("formats unix capture times", () => {
    expect(formatCapturedAt("unix:1781769600")).not.toBe("尚未更新");
  });
});
