import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-06-18T08:00:00Z"));
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
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
  planType: null,
  creditsBalance: null,
  resetCreditsAvailable: null,
  rawText: "status",
  statusMessage: "已更新 5 小时与 1 周额度。",
  warnings: [],
};

const appState: AppState = {
  version: 3,
  latestSnapshot: snapshot,
  storageStatus: "ready",
  storagePath: null,
  backupPath: null,
  statusMessage: "已更新 5 小时与 1 周额度。",
  history: [],
  settings: {
    automaticUpdateChecks: true,
    lowQuotaNotifications: false,
  },
  recoveryNotice: null,
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
    expect(screen.getByText("重置 2小时15分钟后")).toBeTruthy();
  });

  it("updates the visible countdown while the bar remains open", async () => {
    render(QuotaDashboard, { props: { appState } });

    expect(screen.getByTestId("five-hour-reset").textContent).toContain(
      "2h15m",
    );
    await vi.advanceTimersByTimeAsync(60_000);
    await tick();
    expect(screen.getByTestId("five-hour-reset").textContent).toContain(
      "2h14m",
    );
  });

  it("keeps the compact bar focused while exposing a keyboard menu", () => {
    render(QuotaDashboard, { props: { appState } });

    expect(
      screen.getByRole("button", { name: "打开 QuotaDock 菜单" }),
    ).toBeTruthy();
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

  it("opens the native menu from the keyboard-accessible menu button", async () => {
    render(QuotaDashboard, { props: { appState } });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });

    await fireEvent.click(
      screen.getByRole("button", { name: "打开 QuotaDock 菜单" }),
    );

    expect(invoke).toHaveBeenCalledWith("show_dashboard_context_menu", {
      x: 0,
      y: 0,
    });
  });

  it("surfaces native menu failures and points to the tray fallback", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("menu unavailable"));
    render(QuotaDashboard, { props: { appState } });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });

    await fireEvent.click(
      screen.getByRole("button", { name: "打开 QuotaDock 菜单" }),
    );
    await Promise.resolve();
    await tick();

    expect(screen.getByText("菜单失败")).toBeTruthy();
    expect(screen.getByText(/菜单打开失败，请使用系统托盘菜单/)).toBeTruthy();
  });

  it("makes refresh failures visible while retaining the last snapshot", () => {
    const { container } = render(QuotaDashboard, {
      props: {
        appState,
        errorMessage: "Codex CLI 查询失败。",
      },
    });

    expect(container.querySelector(".mini-status")?.getAttribute("data-state")).toBe(
      "error",
    );
    expect(screen.getByText(/刷新失败，当前显示上次成功数据/)).toBeTruthy();
    expect(screen.getByTestId("five-hour-value").textContent).toContain("72%");
  });

  it("describes a first-load failure without claiming that old values exist", () => {
    render(QuotaDashboard, {
      props: {
        appState: null,
        errorMessage: "Codex CLI 查询失败。",
      },
    });

    expect(screen.getByText(/刷新失败，尚无可显示的额度数据/)).toBeTruthy();
  });

  it("surfaces storage recovery even when no snapshot remains", () => {
    const recoveredState: AppState = {
      ...appState,
      latestSnapshot: null,
      storageStatus: "recovered",
      statusMessage: "状态文件损坏，已恢复默认状态。",
    };
    const { container } = render(QuotaDashboard, {
      props: { appState: recoveredState },
    });

    expect(container.querySelector(".mini-status")?.getAttribute("data-state")).toBe(
      "warning",
    );
    expect(screen.getByText(/数据不完整或存储已恢复/)).toBeTruthy();
  });

  it("marks the 20 percent boundary as low usage", () => {
    const lowState: AppState = {
      ...appState,
      latestSnapshot: {
        ...snapshot,
        fiveHour: {
          ...snapshot.fiveHour,
          remainingPercent: 20,
        },
      },
    };
    const { container } = render(QuotaDashboard, {
      props: { appState: lowState },
    });

    expect(container.querySelector(".quota-row.low")).toBeTruthy();
    expect(screen.getByText("低额度")).toBeTruthy();
  });
});

