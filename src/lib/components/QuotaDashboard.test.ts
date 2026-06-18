import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";

afterEach(() => cleanup());

const snapshot: QuotaSnapshot = {
  id: "snapshot-1",
  source: "pasted-status",
  capturedAt: "2026-06-18T08:00:00Z",
  fiveHour: {
    remainingPercent: 72,
    resetAt: null,
    resetCountdownSeconds: 8100,
  },
  weekly: {
    remainingPercent: 46,
    resetAt: "2026-06-23T09:00:00Z",
    resetCountdownSeconds: null,
  },
  rawText: "status",
  statusMessage: "已更新 5 小时与 1 周额度。",
  warnings: [],
};

const appState: AppState = {
  version: 2,
  latestSnapshot: snapshot,
  storageStatus: "ready",
  storagePath: null,
  backupPath: null,
  statusMessage: "已更新 5 小时与 1 周额度。",
};

describe("QuotaDashboard", () => {
  it("renders the Chinese data deck with both quota windows", () => {
    render(QuotaDashboard, { props: { appState } });

    expect(screen.getByText("Codex 额度监控舱")).toBeTruthy();
    expect(screen.getByText("5小时额度")).toBeTruthy();
    expect(screen.getByText("1周额度")).toBeTruthy();
    expect(screen.getByTestId("five-hour-value").textContent).toContain("72%");
    expect(screen.getByTestId("weekly-value").textContent).toContain("46%");
  });

  it("shows unknown values as double dashes", () => {
    render(QuotaDashboard, { props: { appState: null } });

    expect(screen.getByTestId("five-hour-value").textContent).toContain("--");
    expect(screen.getByTestId("weekly-value").textContent).toContain("--");
  });

  it("surfaces automatic query unavailable messages", () => {
    render(QuotaDashboard, {
      props: {
        appState: null,
        noticeMessage: "当前 Codex CLI 未提供额度查询，请粘贴 /status。",
      },
    });

    expect(screen.getByTestId("status-message").textContent).toContain("请粘贴 /status");
  });

  it("emits paste text and parse actions", async () => {
    const onPasteInput = vi.fn();
    const onParse = vi.fn();
    render(QuotaDashboard, {
      props: {
        appState: null,
        pasteText: "5h remaining: 72%",
        onPasteInput,
        onParse,
      },
    });

    await fireEvent.input(screen.getByLabelText("粘贴 /status 内容"), {
      target: { value: "1w remaining: 46%" },
    });
    await fireEvent.click(screen.getByText("粘贴 /status 更新"));

    expect(onPasteInput).toHaveBeenCalledWith("1w remaining: 46%");
    expect(onParse).toHaveBeenCalled();
  });
});
