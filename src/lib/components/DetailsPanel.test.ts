import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DetailsPanel from "$lib/components/DetailsPanel.svelte";
import type { AppState } from "$lib/types/usage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const appState: AppState = {
  version: 4,
  latestSnapshot: {
    id: "app-server-1",
    source: "codex-app-server",
    capturedAt: "unix:1781769600",
    weekly: {
      remainingPercent: 46,
      resetAt: "unix:1782190800",
      resetCountdownSeconds: null,
    },
    planType: "plus",
    creditsBalance: "12",
    resetCreditsAvailable: 1,
    rawText: "",
    statusMessage: "已通过 Codex App Server 更新额度。",
    warnings: [],
  },
  storageStatus: "ready",
  storagePath: "C:\\QuotaDock\\usage-state.json",
  backupPath: null,
  statusMessage: "已通过 Codex App Server 更新额度。",
  history: [
    {
      capturedAt: "unix:1781760000",
      weeklyRemainingPercent: 51,
    },
    {
      capturedAt: "unix:1781769600",
      weeklyRemainingPercent: 46,
    },
  ],
  settings: {
    automaticUpdateChecks: true,
    lowQuotaNotifications: false,
  },
  recoveryNotice: null,
};

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_diagnostics") {
      return Promise.resolve({
        appVersion: "0.5.0",
        codexPath: "C:\\Tools\\codex.exe",
        codexVersion: "codex-cli 0.145.0",
        latestSource: "codex-app-server",
        latestSuccessAt: "unix:1781769600",
        storagePath: "C:\\QuotaDock\\usage-state.json",
        storageStatus: "ready",
        startupEnabled: false,
        signedUpdatesEnabled: true,
      });
    }
    if (command === "update_settings") {
      return Promise.resolve({
        ...appState,
        settings: {
          ...appState.settings,
          automaticUpdateChecks: false,
        },
      });
    }
    if (command === "get_update_status") {
      return Promise.resolve({
        currentVersion: "0.5.3",
        phase: "idle",
        message: "尚未检查软件更新。",
        technicalDetail: null,
        availableVersion: null,
        progressPercent: null,
        checkedAt: null,
      });
    }
    if (command === "check_for_updates") {
      return Promise.resolve({
        currentVersion: "0.5.3",
        phase: "error",
        message: "暂时无法连接更新服务，请检查网络或代理后重试。",
        technicalDetail:
          "获取签名更新清单失败：https://github.com/example/releases/latest/download/latest.json",
        availableVersion: null,
        progressPercent: null,
        checkedAt: "unix:1781769600",
      });
    }
    return Promise.resolve();
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
});

describe("DetailsPanel", () => {
  it("shows quota, structured source, history and signed-update diagnostics", async () => {
    const { container } = render(DetailsPanel, { props: { appState } });

    expect(screen.getByText("额度详情")).toBeTruthy();
    expect(screen.getByText("Codex App Server")).toBeTruthy();
    expect(screen.getByText("46%")).toBeTruthy();
    expect(screen.getByText("2 个采样点")).toBeTruthy();
    expect(screen.getByText("1 周额度趋势")).toBeTruthy();
    expect(container.querySelectorAll(".quota-card")).toHaveLength(1);
    expect(container.querySelectorAll(".series")).toHaveLength(1);
    expect(container.textContent).not.toContain(["5", "小时"].join(""));
    await waitFor(() => {
      expect(screen.getByText("0.5.0")).toBeTruthy();
    });
    expect(screen.getByText("签名更新")).toBeTruthy();
  });

  it("persists update preferences through the native command", async () => {
    render(DetailsPanel, { props: { appState } });

    await fireEvent.click(screen.getByRole("checkbox", { name: /自动检查更新/ }));

    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { automaticUpdateChecks: false },
    });
  });

  it("opens the fixed official usage page through the backend command", async () => {
    render(DetailsPanel, { props: { appState } });

    await fireEvent.click(
      screen.getByRole("button", { name: /打开 Codex 官方 Usage Dashboard/ }),
    );

    expect(invoke).toHaveBeenCalledWith("open_official_usage");
  });

  it("offers retry, browser download and collapsed technical details after an update failure", async () => {
    const { container } = render(DetailsPanel, { props: { appState } });

    const checkButton = await screen.findByRole("button", { name: "检查更新" });
    await fireEvent.click(checkButton);

    await waitFor(() => {
      expect(screen.getByText(/暂时无法连接更新服务/)).toBeTruthy();
    });
    expect(screen.getByRole("button", { name: "重新检查" })).toBeTruthy();
    const downloadButton = screen.getByRole("button", { name: "浏览器下载" });
    expect(downloadButton).toBeTruthy();

    const technicalDetails = container.querySelector(".update-recovery details");
    expect(technicalDetails?.hasAttribute("open")).toBe(false);
    expect(container.querySelector(".update-message")?.textContent).not.toContain("http");

    await fireEvent.click(downloadButton);
    expect(invoke).toHaveBeenCalledWith("open_latest_release");
  });
});
