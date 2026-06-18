import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import UsageSummary from "$lib/components/UsageSummary.svelte";
import type { Settings, UsageSnapshot } from "$lib/types/usage";

afterEach(() => cleanup());

const settings: Settings = {
  staleAfterMinutes: 60,
  notifyBelowPercent: [20, 10],
  clipboardMonitoring: false,
  notificationsEnabled: true,
};

const snapshot: UsageSnapshot = {
  id: "snapshot-1",
  source: "pasted-status",
  parsedAt: "2026-06-18T08:00:00.000Z",
  remainingPercent: 18,
  resetAt: "2026-06-18T12:00:00.000Z",
  resetCountdownSeconds: 14400,
  creditsBalance: 7.25,
  model: "gpt-5-codex",
  contextWindow: "200k",
  confidence: "fresh",
  rawText: "status",
  manualFields: [],
  warnings: [],
  notes: "",
};

describe("UsageSummary", () => {
  it("renders usage numbers from a fresh snapshot", () => {
    render(UsageSummary, {
      props: {
        snapshot,
        settings,
        now: new Date("2026-06-18T08:20:00.000Z"),
      },
    });

    expect(screen.getByTestId("usage-summary").textContent).toContain("18%");
    expect(screen.getByTestId("usage-summary").textContent).toContain("7.25");
    expect(screen.getByTestId("confidence-badge").textContent).toContain("Fresh");
  });

  it("shows manual fields and parse warnings", () => {
    render(UsageSummary, {
      props: {
        snapshot: {
          ...snapshot,
          source: "manual",
          confidence: "manual",
          manualFields: ["remaining-percent", "notes"],
          warnings: [{ code: "missing-reset", message: "Reset time was not found." }],
        },
        settings,
      },
    });

    expect(screen.getByTestId("manual-marker").textContent).toContain("remaining-percent");
    expect(screen.getByTestId("parse-warnings").textContent).toContain("missing-reset");
    expect(screen.getByTestId("confidence-badge").textContent).toContain("Manual");
  });

  it("keeps unavailable and stale states visible", () => {
    const { rerender } = render(UsageSummary, { props: { snapshot: null, settings } });
    expect(screen.getByTestId("confidence-badge").textContent).toContain("Unavailable");

    rerender({
      snapshot,
      settings,
      now: new Date("2026-06-18T09:30:00.000Z"),
    });

    expect(screen.getByTestId("confidence-badge").textContent).toContain("Stale");
  });
});
