import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DetailsPanel from "$lib/components/DetailsPanel.svelte";
import type { AppState } from "$lib/types/usage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

const appState: AppState = {
  version: 3,
  latestSnapshot: {
    id: "app-server-1",
    source: "codex-app-server",
    capturedAt: "unix:1781769600",
    fiveHour: {
      remainingPercent: 72,
      resetAt: "unix:1781773200",
      resetCountdownSeconds: null,
    },
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
      fiveHourRemainingPercent: 84,
      weeklyRemainingPercent: 51,
    },
    {
      capturedAt: "unix:1781769600",
      fiveHourRemainingPercent: 72,
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
    expect(screen.getByText("72%")).toBeTruthy();
    expect(screen.getByText("46%")).toBeTruthy();
    expect(screen.getByText("2 个采样点")).toBeTruthy();
    expect(container.querySelectorAll(".series").length).toBe(2);
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
});
