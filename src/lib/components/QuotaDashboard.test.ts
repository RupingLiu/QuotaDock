import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
});

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
    const { container } = render(QuotaDashboard, { props: { appState } });

    expect(screen.queryByText("剩余用量")).toBeNull();
    expect(screen.getByText("5小时额度")).toBeTruthy();
    expect(screen.getByText("1周额度")).toBeTruthy();
    expect(screen.getByTestId("five-hour-value").textContent).toContain("72%");
    expect(screen.getByTestId("weekly-value").textContent).toContain("46%");
    expect(container.querySelector(".panel-chevron")).toBeNull();
    expect(
      container
        .querySelector(".mini-status")
        ?.getAttribute("data-tauri-drag-region"),
    ).toBe("deep");
    expect(
      container.querySelector(".mini-status [data-tauri-drag-region]"),
    ).toBeNull();
  });

  it("shows unknown values as double dashes", () => {
    render(QuotaDashboard, { props: { appState: null } });

    expect(screen.getByTestId("five-hour-value").textContent).toContain("--");
    expect(screen.getByTestId("weekly-value").textContent).toContain("--");
  });

  it("shows reset timing for both quota windows", () => {
    render(QuotaDashboard, { props: { appState } });

    expect(screen.getByTestId("five-hour-reset").textContent).toContain(
      "2h15m",
    );
    expect(screen.getByTestId("weekly-reset").textContent).toContain("6/23");
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

  it("opens the native menu from the dashboard context menu", async () => {
    const { container } = render(QuotaDashboard, { props: { appState } });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });

    await fireEvent.contextMenu(container.querySelector(".float-shell")!, {
      clientX: 122,
      clientY: 34,
    });

    expect(invoke).toHaveBeenCalledWith("show_dashboard_context_menu", {
      x: 122,
      y: 34,
    });
  });
});

