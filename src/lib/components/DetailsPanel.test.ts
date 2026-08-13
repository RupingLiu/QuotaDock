import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type Event } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DetailsPanel from "$lib/components/DetailsPanel.svelte";
import type {
  AppState,
  QuotaSnapshot,
  RefreshProvidersResult,
  UpdateStatus,
} from "$lib/types/usage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const snapshot: QuotaSnapshot = {
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
};

const appState: AppState = {
  version: 6,
  revision: 1,
  providers: {
    codex: {
      configured: true,
      latestSnapshot: { provider: "codex", data: snapshot },
      lastAttemptAt: snapshot.capturedAt,
      health: "fresh",
      errorCategory: null,
    },
    deepseek: {
      configured: false,
      latestSnapshot: null,
      lastAttemptAt: null,
      health: "not-configured",
      errorCategory: null,
    },
    kimi: {
      configured: false,
      latestSnapshot: null,
      lastAttemptAt: null,
      health: "not-configured",
      errorCategory: null,
    },
  },
  latestSnapshot: snapshot,
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
    floatingProviderIds: ["codex"],
  },
  recoveryNotice: null,
};

function connectedState(): AppState {
  return {
    ...appState,
    providers: {
      ...appState.providers,
      deepseek: {
        configured: true,
        latestSnapshot: {
          provider: "deepseek",
          data: {
            id: "deepseek-1",
            capturedAt: "unix:1781769600",
            isAvailable: false,
            balances: [
              { currency: "USD", totalBalance: "10.001", grantedBalance: "10.001", toppedUpBalance: "0.000" },
              { currency: "CNY", totalBalance: "0.00", grantedBalance: "0.00", toppedUpBalance: "0.00" },
            ],
          },
        },
        lastAttemptAt: "unix:1781769600",
        health: "fresh",
        errorCategory: null,
      },
      kimi: {
        configured: true,
        latestSnapshot: {
          provider: "kimi",
          data: {
            id: "kimi-1",
            capturedAt: "unix:1781769600",
            total: {
              name: "总使用量",
              window: null,
              used: "522",
              limit: "1000",
              resetAt: "2030-08-26T00:00:00Z",
            },
            limits: [
              { name: "Code", window: { duration: 5, unit: "hour" }, used: "0", limit: "100", resetAt: "2030-08-14T07:19:00Z" },
              { name: "Code", window: { duration: 7, unit: "day" }, used: "157", limit: "10000", resetAt: "2030-08-17T09:19:00Z" },
            ],
          },
        },
        lastAttemptAt: "unix:1781769600",
        health: "fresh",
        errorCategory: null,
      },
    },
    settings: { ...appState.settings, floatingProviderIds: ["codex", "deepseek"] },
  };
}

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
let providerRefreshResponse: RefreshProvidersResult | null;

beforeEach(() => {
  initialUpdateStatus = idleUpdateStatus;
  initialUpdateStatusPromise = null;
  checkedUpdateStatusPromise = null;
  updateEventListener = null;
  providerRefreshResponse = null;
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
    if (command === "get_provider_credential_status") {
      return Promise.resolve([
        { providerId: "deepseek", availability: "not-configured" },
        { providerId: "kimi", availability: "not-configured" },
      ]);
    }
    if (command === "get_app_state") return Promise.resolve(appState);
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
    if (command === "refresh_provider" && providerRefreshResponse) {
      return Promise.resolve(providerRefreshResponse);
    }
    return Promise.resolve();
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
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
    expect(container.querySelectorAll(".quota-card")).toHaveLength(3);
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

    expect(invoke).toHaveBeenCalledWith("open_provider_portal", {
      provider: "codex",
    });
  });

  it("shows every DeepSeek currency and all Kimi Coding Plan windows", () => {
    render(DetailsPanel, { props: { appState: connectedState() } });

    expect(screen.getByText("USD 充值余额")).toBeTruthy();
    expect(screen.getByText("$0.000")).toBeTruthy();
    expect(screen.getByText("CNY 充值余额")).toBeTruthy();
    expect(screen.getByText("¥0.00")).toBeTruthy();
    expect(screen.getByText(/账户余额接口：不可用/)).toBeTruthy();
    expect(screen.getByText("总使用量")).toBeTruthy();
    expect(screen.getByText("Code · 5 小时")).toBeTruthy();
    expect(screen.getByText("Code · 7 天")).toBeTruthy();
    expect(screen.getByText("剩余 47.8%")).toBeTruthy();
    expect(screen.getByText("剩余 100%")).toBeTruthy();
    expect(screen.getByText("剩余 98.43%")).toBeTruthy();
    expect(screen.getByText(/官方接口不保证返回 Kimi \/ Code 分段/)).toBeTruthy();
  });

  it("keeps full-precision DeepSeek totals accessible without ellipsis", () => {
    const current = connectedState();
    const deepseek = current.providers.deepseek.latestSnapshot;
    if (deepseek?.provider !== "deepseek") throw new Error("fixture mismatch");
    deepseek.data.balances[0] = {
      currency: "USD",
      totalBalance: "12345678901234567890.123456789",
      grantedBalance: "98765432109876543210.000000001",
      toppedUpBalance: "0.000000000",
    };
    const { container } = render(DetailsPanel, { props: { appState: current } });
    const detail = container.querySelector<HTMLElement>(".balance-detail")!;
    expect(detail.textContent).toContain("$12345678901234567890.123456789");
    expect(detail.textContent).toContain("$98765432109876543210.000000001");
    expect(detail.classList.contains("balance-detail")).toBe(true);
    expect(detail.title).toContain("$12345678901234567890.123456789");
    expect(detail.title).toContain("$98765432109876543210.000000001");
  });

  it("marks all configured cards busy during refresh-all without showing fresh", () => {
    const { container } = render(DetailsPanel, {
      props: { appState: connectedState(), refreshing: true },
    });
    for (const provider of ["codex", "deepseek", "kimi"]) {
      const card = container.querySelector(`[data-provider="${provider}"]`)!;
      expect(card.getAttribute("aria-busy")).toBe("true");
      const badge = card.querySelector(".provider-status")!;
      expect(badge.getAttribute("data-health")).toBe("refreshing");
      expect(badge.textContent).toBe("读取中");
    }
  });

  it("derives matching stale data-health after the low-frequency clock crosses the threshold", async () => {
    vi.useFakeTimers();
    const current = connectedState();
    const capturedAtMs = 1_781_769_600 * 1000;
    vi.setSystemTime(capturedAtMs + 9 * 60_000 + 45_000);
    const view = render(DetailsPanel, { props: { appState: current } });
    const badge = view.container.querySelector('[data-provider="deepseek"] .provider-status')!;
    expect(badge.getAttribute("data-health")).toBe("fresh");
    expect(badge.textContent).toBe("已更新");
    await vi.advanceTimersByTimeAsync(30_000);
    expect(badge.getAttribute("data-health")).toBe("stale");
    expect(badge.textContent).toBe("数据陈旧");
    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it("announces a DeepSeek refresh without clearing the visible Codex error", async () => {
    const next = connectedState();
    next.revision = 2;
    providerRefreshResponse = {
      appState: next,
      providerResults: [{
        providerId: "deepseek",
        outcome: "updated",
        message: "DeepSeek 余额已更新。",
        errorCategory: null,
      }],
      anyUpdated: true,
      message: "DeepSeek 余额已更新。",
    };
    const onStateChange = vi.fn();
    const { container } = render(DetailsPanel, {
      props: {
        appState: connectedState(),
        errorMessage: "Codex 刷新失败。",
        onStateChange,
      },
    });
    await fireEvent.click(screen.getByRole("button", { name: "刷新 DeepSeek" }));
    await waitFor(() => expect(onStateChange).toHaveBeenCalledWith(next));
    expect(container.querySelector(".status-strip")?.textContent).toContain("Codex 刷新失败。");
    expect(screen.getByTestId("provider-announcement").textContent).toContain("DeepSeek 余额已更新。");
  });

  it("deduplicates provider announcements already exposed by the visible status", async () => {
    const view = render(DetailsPanel, {
      props: {
        appState: connectedState(),
        errorMessage: "Codex 刷新失败。",
        providerAnnouncement: "Codex 刷新失败。",
      },
    });
    expect(view.container.querySelector(".status-strip")?.textContent).toContain("Codex 刷新失败。");
    expect(screen.getByTestId("provider-announcement").textContent).toBe("");

    await view.rerender({
      appState: connectedState(),
      errorMessage: null,
      noticeMessage: "全部供应商额度已更新。",
      providerAnnouncement: "全部供应商额度已更新。",
    });
    expect(view.container.querySelector(".status-strip")?.textContent).toContain("全部供应商额度已更新。");
    expect(screen.getByTestId("provider-announcement").textContent).toBe("");

    await view.rerender({
      appState: connectedState(),
      noticeMessage: "全部供应商额度已更新。",
      providerAnnouncement: "DeepSeek 余额已更新。",
    });
    expect(screen.getByTestId("provider-announcement").textContent).toBe("DeepSeek 余额已更新。");
  });

  it("persists the complete canonical floating selection and prevents removing the last item", async () => {
    render(DetailsPanel, { props: { appState: connectedState() } });

    await fireEvent.click(
      screen.getByRole("checkbox", { name: "将 Kimi 加入悬浮条轮播" }),
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { floatingProviderIds: ["codex", "deepseek", "kimi"] },
    });

    cleanup();
    render(DetailsPanel, { props: { appState } });
    expect(
      screen.getByRole("checkbox", { name: "将 Codex 加入悬浮条轮播" }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByRole("checkbox", { name: "将 DeepSeek 加入悬浮条轮播" }),
    ).toHaveProperty("disabled", true);
  });

  it("uses password-only credential fields, clears the submitted secret, and never refills it", async () => {
    const current = connectedState();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_app_state") return Promise.resolve(current);
      if (command === "get_provider_credential_status") return Promise.resolve([]);
      if (command === "get_diagnostics") return Promise.resolve({
        appVersion: "0.5.4", codexPath: null, codexVersion: null,
        latestSource: null, latestSuccessAt: null, storagePath: null,
        storageStatus: "ready", startupEnabled: false, signedUpdatesEnabled: true,
      });
      if (command === "get_update_status") return Promise.resolve(idleUpdateStatus);
      return Promise.resolve({ providerId: "deepseek", availability: "configured" });
    });
    const { container } = render(DetailsPanel, { props: { appState: current } });
    const input = container.querySelector<HTMLInputElement>("#deepseek-key")!;
    expect(input.type).toBe("password");
    expect(input.autocomplete).toBe("off");
    expect(input.getAttribute("spellcheck")).toBe("false");
    expect(input.getAttribute("autocapitalize")).toBe("none");
    expect(screen.getByRole("button", { name: "替换 DeepSeek API Key" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "替换 Kimi Code API Key" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "删除 Kimi Code API Key" })).toBeTruthy();
    await fireEvent.input(input, { target: { value: "sk-local-test-value" } });
    await fireEvent.click(screen.getByRole("button", { name: "替换 DeepSeek API Key" }));
    await waitFor(() => expect(input.value).toBe(""));
    expect(container.textContent).not.toContain("sk-local-test-value");
    expect(invoke).toHaveBeenCalledWith("set_provider_credential", {
      provider: "deepseek",
      secret: "sk-local-test-value",
    });
  });

  it("confirms credential deletion and uses only fixed provider portal commands", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(DetailsPanel, { props: { appState: connectedState() } });
    await fireEvent.click(screen.getByRole("button", { name: "删除 DeepSeek API Key" }));
    expect(confirm).toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("delete_provider_credential", {
      provider: "deepseek",
    });
    await fireEvent.click(screen.getByRole("button", { name: /打开 Kimi Code 官方页面/ }));
    expect(invoke).toHaveBeenCalledWith("open_provider_portal", { provider: "kimi" });
    confirm.mockRestore();
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
    expect(document.querySelector(".update-copy")?.getAttribute("aria-live")).toBe("polite");
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
    expect(document.querySelector(".update-copy")?.getAttribute("aria-live")).toBe("polite");
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
