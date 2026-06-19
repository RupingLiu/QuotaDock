import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
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

  it("shows the weekly refresh time instead of the captured time", () => {
    render(QuotaDashboard, { props: { appState } });

    const status = screen.getByTestId("status-message").textContent ?? "";
    expect(status).toContain("周更");
    expect(status).toContain("06/23");
    expect(status).not.toContain("06/18");
  });

  it("does not render a status-bar refresh button", () => {
    render(QuotaDashboard, { props: { appState } });

    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByLabelText("自动查询")).toBeNull();
    expect(screen.queryByText("粘贴 /status 更新")).toBeNull();
    expect(screen.queryByText("保存解析结果")).toBeNull();
    expect(screen.queryByText("清空")).toBeNull();
    expect(screen.queryByLabelText("粘贴 /status 内容")).toBeNull();
  });
});

