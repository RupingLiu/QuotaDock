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

    expect(screen.getByText("Codex 额度")).toBeTruthy();
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
        noticeMessage: "当前无法自动查询，请确认 Codex CLI 可用后重试。",
      },
    });

    expect(screen.getByTestId("status-message").textContent).toContain("确认 Codex CLI 可用");
  });

  it("keeps only the automatic query action", async () => {
    const onRefresh = vi.fn();
    render(QuotaDashboard, {
      props: {
        appState: null,
        onRefresh,
      },
    });

    await fireEvent.click(screen.getByLabelText("自动查询"));

    expect(onRefresh).toHaveBeenCalled();
    expect(screen.queryByText("粘贴 /status 更新")).toBeNull();
    expect(screen.queryByText("保存解析结果")).toBeNull();
    expect(screen.queryByText("清空")).toBeNull();
    expect(screen.queryByLabelText("粘贴 /status 内容")).toBeNull();
  });
});

