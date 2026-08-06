import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type Event } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DetailsPanel from "$lib/components/DetailsPanel.svelte";
import type { AppState, UpdateStatus } from "$lib/types/usage";

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

const idleUpdateStatus: UpdateStatus = {
  currentVersion: "0.5.4",
  phase: "idle",
  message: "尚未检查软件更新。",
  technicalDetail: null,
  availableVersion: null,
  progressPercent: null,
  checkedAt: null,
};

let initialUpdateStatus: UpdateStatus;
let initialUpdateStatusPromise: Promise<UpdateStatus> | null;
let checkedUpdateStatus: UpdateStatus;
let checkedUpdateStatusPromise: Promise<UpdateStatus> | null;
let installedUpdateStatus: UpdateStatus;
let updateEventListener: ((event: Event<UpdateStatus>) => void) | null;

beforeEach(() => {
  initialUpdateStatus = idleUpdateStatus;
  initialUpdateStatusPromise = null;
  checkedUpdateStatusPromise = null;
  updateEventListener = null;
  checkedUpdateStatus = {
    ...idleUpdateStatus,
    phase: "error",
    message: "暂时无法连接更新服务，请检查网络或代理后重试。",
    technicalDetail:
      "获取签名更新清单失败：https://github.com/example/releases/latest/download/latest.json",
    checkedAt: "unix:1781769600",
  };
  installedUpdateStatus = {
    ...idleUpdateStatus,
    phase: "installing",
    message: "正在安装更新，应用即将重启。",
    availableVersion: "next",
    progressPercent: 100,
    checkedAt: "unix:1781769600",
  };
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  vi.mocked(listen).mockImplementation((_event, listener) => {
    updateEventListener = listener as (event: Event<UpdateStatus>) => void;
    return Promise.resolve(() => {});
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
      return initialUpdateStatusPromise ?? Promise.resolve(initialUpdateStatus);
    }
    if (command === "check_for_updates") {
      return checkedUpdateStatusPromise ?? Promise.resolve(checkedUpdateStatus);
    }
    if (command === "install_downloaded_update") {
      return Promise.resolve(installedUpdateStatus);
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

  it("shows background download progress as a polite live status", async () => {
    checkedUpdateStatus = {
      ...idleUpdateStatus,
      phase: "downloading",
      message: "正在后台下载并验证更新…",
      availableVersion: "next",
      progressPercent: 42,
      checkedAt: "unix:1781769600",
    };
    render(DetailsPanel, { props: { appState } });

    await fireEvent.click(await screen.findByRole("button", { name: "检查更新" }));

    expect(invoke).toHaveBeenCalledWith("check_for_updates");
    const progress = await screen.findByRole("progressbar", { name: "更新下载进度" });
    expect(progress.getAttribute("aria-valuenow")).toBe("42");
    expect(screen.getByRole("button", { name: "下载中 42%" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
  });

  it("installs only after a downloaded update is ready", async () => {
    initialUpdateStatus = {
      ...idleUpdateStatus,
      phase: "ready",
      message: "新版本已下载并通过签名校验，可以立即安装。",
      availableVersion: "next",
      progressPercent: 100,
      checkedAt: "unix:1781769600",
    };
    render(DetailsPanel, { props: { appState } });

    const installButton = await screen.findByRole("button", { name: "立即安装" });
    expect(screen.getByText("可安装")).toBeTruthy();
    expect(screen.getByRole("status").getAttribute("aria-live")).toBe("polite");
    await fireEvent.click(installButton);

    expect(invoke).toHaveBeenCalledWith("install_downloaded_update");
    expect(invoke).not.toHaveBeenCalledWith("check_for_updates");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "正在安装…" })).toHaveProperty(
        "disabled",
        true,
      );
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("does not let an older startup snapshot overwrite a newer ready event", async () => {
    const staleSnapshot = {
      ...idleUpdateStatus,
      phase: "downloading" as const,
      message: "正在后台下载并验证更新…",
      availableVersion: "next",
      progressPercent: 75,
    };
    let resolveSnapshot: (status: UpdateStatus) => void = () => {};
    initialUpdateStatusPromise = new Promise((resolve) => {
      resolveSnapshot = resolve;
    });
    render(DetailsPanel, { props: { appState } });

    await waitFor(() => expect(updateEventListener).not.toBeNull());
    updateEventListener?.({
      event: "quotadock:update-status",
      id: 1,
      payload: {
        ...idleUpdateStatus,
        phase: "ready",
        message: "新版本已下载并通过签名校验，可以立即安装。",
        availableVersion: "next",
        progressPercent: 100,
      },
    });
    resolveSnapshot(staleSnapshot);

    expect(await screen.findByRole("button", { name: "立即安装" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "下载中 75%" })).toBeNull();
  });

  it("does not let an older check response overwrite a newer ready event", async () => {
    const staleResponse = {
      ...idleUpdateStatus,
      phase: "downloading" as const,
      message: "正在后台下载并验证更新…",
      availableVersion: "next",
      progressPercent: 75,
    };
    let resolveCheck: (status: UpdateStatus) => void = () => {};
    checkedUpdateStatusPromise = new Promise((resolve) => {
      resolveCheck = resolve;
    });
    render(DetailsPanel, { props: { appState } });

    await waitFor(() => expect(updateEventListener).not.toBeNull());
    await fireEvent.click(await screen.findByRole("button", { name: "检查更新" }));
    updateEventListener?.({
      event: "quotadock:update-status",
      id: 2,
      payload: {
        ...idleUpdateStatus,
        phase: "ready",
        message: "新版本已下载并通过签名校验，可以立即安装。",
        availableVersion: "next",
        progressPercent: 100,
      },
    });
    resolveCheck(staleResponse);

    expect(await screen.findByRole("button", { name: "立即安装" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "下载中 75%" })).toBeNull();
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
