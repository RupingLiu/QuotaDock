import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { tick } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
import type { AppState, ProviderId, QuotaSnapshot } from "$lib/types/usage";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve()) }));

const codex: QuotaSnapshot = {
  id: "codex-1",
  source: "codex-app-server",
  capturedAt: "2026-06-18T08:00:00Z",
  weekly: { remainingPercent: 46, resetAt: "2026-06-23T09:00:00Z", resetCountdownSeconds: null },
  planType: null,
  creditsBalance: null,
  resetCreditsAvailable: null,
  rawText: "",
  statusMessage: "Codex 已更新。",
  warnings: [],
};

function state(selected: ProviderId[] = ["codex"]): AppState {
  return {
    version: 6,
    revision: 3,
    providers: {
      codex: { configured: true, latestSnapshot: { provider: "codex", data: codex }, lastAttemptAt: codex.capturedAt, health: "fresh", errorCategory: null },
      deepseek: {
        configured: true,
        latestSnapshot: { provider: "deepseek", data: { id: "ds-1", capturedAt: codex.capturedAt, isAvailable: true, balances: [
          { currency: "USD", totalBalance: "2.00", grantedBalance: "0.00", toppedUpBalance: "2.00" },
          { currency: "CNY", totalBalance: "100.00", grantedBalance: "0.00", toppedUpBalance: "100.00" },
        ] } },
        lastAttemptAt: codex.capturedAt,
        health: "fresh",
        errorCategory: null,
      },
      kimi: {
        configured: true,
        latestSnapshot: { provider: "kimi", data: {
          id: "kimi-1",
          capturedAt: codex.capturedAt,
          total: { name: "总使用量", window: null, used: "52", limit: "100", resetAt: "2026-08-26T00:00:00Z" },
          limits: [
            { name: "Code", window: { duration: 5, unit: "hour" }, used: "0", limit: "100", resetAt: "2026-08-14T07:19:00Z" },
            { name: "Code", window: { duration: 7, unit: "day" }, used: "1", limit: "64", resetAt: "2026-08-17T09:19:00Z" },
          ],
        } },
        lastAttemptAt: codex.capturedAt,
        health: "fresh",
        errorCategory: null,
      },
    },
    latestSnapshot: codex,
    storageStatus: "ready",
    storagePath: null,
    backupPath: null,
    statusMessage: "额度已更新。",
    history: [],
    settings: { automaticUpdateChecks: true, lowQuotaNotifications: false, floatingProviderIds: selected },
    recoveryNotice: null,
  };
}

let reducedMotion = false;
let motionListener: ((event: MediaQueryListEvent) => void) | null;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-06-18T08:00:00Z"));
  reducedMotion = false;
  motionListener = null;
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => ({
      matches: reducedMotion,
      media: "(prefers-reduced-motion: reduce)",
      onchange: null,
      addEventListener: (_name: string, listener: (event: MediaQueryListEvent) => void) => { motionListener = listener; },
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

function currentProvider(): string | null {
  return document.querySelector(".mini-status")?.getAttribute("data-provider") ?? null;
}

describe("QuotaDashboard provider rotation", () => {
  it("reads Codex from providers and keeps the compact drag shell", () => {
    const { container } = render(QuotaDashboard, { props: { appState: state() } });
    expect(currentProvider()).toBe("codex");
    expect(screen.getByTestId("provider-value").textContent).toBe("46%");
    expect(screen.getByRole("button", { name: /当前显示 Codex/ })).toHaveProperty("disabled", true);
    expect(container.querySelector(".mini-status")?.getAttribute("data-tauri-drag-region")).toBe("deep");
  });

  it("rotates in canonical order at 8 and 16 seconds, preferring DeepSeek CNY", async () => {
    render(QuotaDashboard, { props: { appState: state(["kimi", "codex", "deepseek"]) } });
    expect(currentProvider()).toBe("codex");
    await vi.advanceTimersByTimeAsync(7_999);
    expect(currentProvider()).toBe("codex");
    await vi.advanceTimersByTimeAsync(1);
    expect(currentProvider()).toBe("deepseek");
    expect(screen.getByTestId("provider-value").textContent).toBe("¥100.00");
    await vi.advanceTimersByTimeAsync(8_000);
    expect(currentProvider()).toBe("kimi");
    expect(screen.getByTestId("provider-value").textContent).toBe("总 48%");
    expect(document.querySelector(".secondary")?.textContent).toBe("7d 98.44%");
  });

  it("manual switching restarts a complete eight second interval", async () => {
    render(QuotaDashboard, { props: { appState: state(["codex", "deepseek", "kimi"]) } });
    await vi.advanceTimersByTimeAsync(4_000);
    await fireEvent.click(screen.getByRole("button", { name: /当前显示 Codex/ }));
    expect(currentProvider()).toBe("deepseek");
    await vi.advanceTimersByTimeAsync(7_999);
    expect(currentProvider()).toBe("deepseek");
    await vi.advanceTimersByTimeAsync(1);
    expect(currentProvider()).toBe("kimi");
  });

  it("keeps rotating when state revisions update before the deadline", async () => {
    const view = render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    for (let revision = 4; revision <= 8; revision += 1) {
      await vi.advanceTimersByTimeAsync(1_000);
      await view.rerender({ appState: { ...state(["codex", "deepseek"]), revision } });
    }
    await vi.advanceTimersByTimeAsync(3_000);
    expect(currentProvider()).toBe("deepseek");
  });

  it("opens the fixed DeepSeek top-up page from the compact secondary action", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    await fireEvent.click(screen.getByRole("button", { name: /当前显示 Codex/ }));
    await fireEvent.mouseDown(screen.getByRole("button", { name: "打开 DeepSeek 官方充值页面" }));
    await fireEvent.click(screen.getByRole("button", { name: "打开 DeepSeek 官方充值页面" }));
    expect(invoke).toHaveBeenCalledWith("open_provider_portal", { provider: "deepseek" });
    await vi.advanceTimersByTimeAsync(8_000);
    expect(currentProvider()).toBe("codex");
  });

  it("pauses for hover and restarts a full interval after leaving", async () => {
    const { container } = render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    const shell = container.querySelector(".mini-status")!;
    await vi.advanceTimersByTimeAsync(4_000);
    await fireEvent.pointerEnter(shell);
    await vi.advanceTimersByTimeAsync(12_000);
    expect(currentProvider()).toBe("codex");
    await fireEvent.pointerLeave(shell);
    await vi.advanceTimersByTimeAsync(7_999);
    expect(currentProvider()).toBe("codex");
    await vi.advanceTimersByTimeAsync(1);
    expect(currentProvider()).toBe("deepseek");
  });

  it("keeps focus pause while moving inside and resumes after focus leaves", async () => {
    render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    const provider = screen.getByRole("button", { name: /当前显示 Codex/ });
    const menu = screen.getByRole("button", { name: "打开 QuotaDock 菜单" });
    provider.focus();
    await fireEvent.focusIn(provider);
    await fireEvent.focusOut(provider, { relatedTarget: menu });
    menu.focus();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(currentProvider()).toBe("codex");
    await fireEvent.focusOut(menu, { relatedTarget: document.body });
    await vi.advanceTimersByTimeAsync(8_000);
    expect(currentProvider()).toBe("deepseek");
  });

  it("does not auto rotate under reduced motion but retains manual next", async () => {
    reducedMotion = true;
    render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    await vi.advanceTimersByTimeAsync(20_000);
    expect(currentProvider()).toBe("codex");
    await fireEvent.click(screen.getByRole("button", { name: /当前显示 Codex/ }));
    expect(currentProvider()).toBe("deepseek");
  });

  it("does not catch up while hidden and waits a full interval when visible again", async () => {
    render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(20_000);
    expect(currentProvider()).toBe("codex");
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(7_999);
    expect(currentProvider()).toBe("codex");
    await vi.advanceTimersByTimeAsync(1);
    expect(currentProvider()).toBe("deepseek");
  });

  it("stops when reduced motion becomes active and cleans timers on destroy", async () => {
    const view = render(QuotaDashboard, { props: { appState: state(["codex", "deepseek"]) } });
    reducedMotion = true;
    motionListener?.({ matches: true } as MediaQueryListEvent);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(currentProvider()).toBe("codex");
    view.unmount();
    await tick();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("filters unconfigured and duplicate choices and falls back when active is removed", async () => {
    const initial = state(["deepseek", "deepseek", "codex"]);
    const view = render(QuotaDashboard, { props: { appState: initial } });
    await fireEvent.click(screen.getByRole("button", { name: /当前显示 Codex/ }));
    expect(currentProvider()).toBe("deepseek");
    const next = state(["codex", "kimi"]);
    next.providers.kimi.configured = false;
    await view.rerender({ appState: next });
    expect(currentProvider()).toBe("codex");
  });

  it("does not change either live region during automatic provider rotation", async () => {
    render(QuotaDashboard, {
      props: {
        appState: state(["codex", "deepseek"]),
        errorMessage: "Codex 刷新失败。",
        providerAnnouncement: "Codex 刷新失败。",
      },
    });
    const legacyLive = screen.getByTestId("legacy-announcement");
    const providerLive = screen.getByTestId("provider-announcement");
    const legacyBefore = legacyLive.textContent;
    const providerBefore = providerLive.textContent;
    expect(legacyBefore).toBe("Codex 刷新失败。");
    expect(providerBefore).toBe("");
    await vi.advanceTimersByTimeAsync(8_000);
    expect(currentProvider()).toBe("deepseek");
    expect(legacyLive.textContent).toBe(legacyBefore);
    expect(providerLive.textContent).toBe(providerBefore);
  });

  it("marks the active configured provider busy during refresh-all", async () => {
    render(QuotaDashboard, {
      props: {
        appState: state(["deepseek"]),
        refreshing: true,
      },
    });
    await tick();
    expect(currentProvider()).toBe("deepseek");
    const shell = document.querySelector(".mini-status")!;
    expect(shell.getAttribute("data-state")).toBe("busy");
    expect(shell.getAttribute("aria-busy")).toBe("true");
    expect(document.querySelector(".freshness")?.textContent).toBe("读取");
  });

  it("announces real provider refreshes independently while automatic rotation stays silent", async () => {
    render(QuotaDashboard, {
      props: {
        appState: state(["codex", "deepseek"]),
        errorMessage: "Codex 刷新失败。",
        providerAnnouncement: "DeepSeek 余额已更新。",
      },
    });
    const providerLive = screen.getByTestId("provider-announcement");
    expect(providerLive.textContent).toBe("DeepSeek 余额已更新。");
    expect(screen.getByTestId("legacy-announcement").textContent).toBe("Codex 刷新失败。");
    await vi.advanceTimersByTimeAsync(8_000);
    expect(currentProvider()).toBe("deepseek");
    expect(providerLive.textContent).toBe("DeepSeek 余额已更新。");
    expect(screen.getByTestId("legacy-announcement").textContent).toBe("Codex 刷新失败。");
  });

  it("deduplicates matching Codex and refresh-all announcements", async () => {
    const view = render(QuotaDashboard, {
      props: {
        appState: state(),
        errorMessage: "Codex 刷新失败。",
        providerAnnouncement: "Codex 刷新失败。",
      },
    });
    expect(screen.getByTestId("legacy-announcement").textContent).toBe("Codex 刷新失败。");
    expect(screen.getByTestId("provider-announcement").textContent).toBe("");

    await view.rerender({
      appState: state(),
      errorMessage: null,
      noticeMessage: "全部供应商额度已更新。",
      providerAnnouncement: "全部供应商额度已更新。",
    });
    expect(screen.getByTestId("legacy-announcement").textContent).toBe("全部供应商额度已更新。");
    expect(screen.getByTestId("provider-announcement").textContent).toBe("");

    await view.rerender({
      appState: state(),
      noticeMessage: "全部供应商额度已更新。",
      providerAnnouncement: "Kimi 余额已更新。",
    });
    expect(screen.getByTestId("provider-announcement").textContent).toBe("Kimi 余额已更新。");
  });

  it("opens the fixed native context menu and reports a menu failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("unavailable"));
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    render(QuotaDashboard, { props: { appState: state() } });
    await fireEvent.click(screen.getByRole("button", { name: "打开 QuotaDock 菜单" }));
    await tick();
    expect(invoke).toHaveBeenCalledWith("show_dashboard_context_menu", { x: 0, y: 0 });
    expect(screen.getByTestId("legacy-announcement").textContent).toContain("菜单打开失败");
  });

  it("marks provider failure with retained values and stale captures without global pollution", () => {
    const failed = state();
    failed.providers.codex.health = "error";
    failed.providers.codex.errorCategory = "network";
    let view = render(QuotaDashboard, { props: { appState: failed } });
    expect(document.querySelector(".mini-status")?.getAttribute("data-state")).toBe("error");
    expect(screen.getByTestId("provider-value").textContent).toBe("46%");
    view.unmount();
    const old = state();
    old.providers.codex.latestSnapshot = { provider: "codex", data: { ...codex, capturedAt: "2026-06-17T08:00:00Z" } };
    view = render(QuotaDashboard, { props: { appState: old } });
    expect(document.querySelector(".mini-status")?.getAttribute("data-state")).toBe("stale");
  });
});
