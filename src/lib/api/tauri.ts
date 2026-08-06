import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppDiagnostics,
  AppState,
  QuotaSnapshot,
  RefreshUsageResult,
  SettingsPatch,
  UpdateStatus,
} from "$lib/types/usage";

export type QuotaDockApi = {
  getAppState(): Promise<AppState>;
  refreshUsage(): Promise<RefreshUsageResult>;
  showDashboardContextMenu(x: number, y: number): Promise<void>;
  updateSettings(patch: SettingsPatch): Promise<AppState>;
  acknowledgeRecovery(): Promise<AppState>;
  getDiagnostics(): Promise<AppDiagnostics>;
  setStartupEnabled(enabled: boolean): Promise<boolean>;
  hideDetails(): Promise<void>;
  openOfficialUsage(): Promise<void>;
  getUpdateStatus(): Promise<UpdateStatus>;
  checkForUpdates(): Promise<UpdateStatus>;
  openLatestRelease(): Promise<void>;
  onUpdateStatus(listener: (status: UpdateStatus) => void): Promise<UnlistenFn>;
};

export const tauriApi: QuotaDockApi = {
  getAppState: () =>
    hasTauriRuntime()
      ? invoke<AppState>("get_app_state")
      : Promise.resolve(defaultAppState("浏览器预览模式：请在桌面应用中查询额度。")),
  refreshUsage: () =>
    hasTauriRuntime()
      ? invoke<RefreshUsageResult>("refresh_usage")
      : Promise.resolve({
          appState: defaultAppState("浏览器预览模式无法调用 Codex CLI。"),
          updated: false,
          message: "浏览器预览模式无法调用 Codex CLI。",
        }),
  showDashboardContextMenu: (x, y) =>
    hasTauriRuntime()
      ? invoke<void>("show_dashboard_context_menu", { x, y })
      : Promise.resolve(),
  updateSettings: (patch) =>
    hasTauriRuntime()
      ? invoke<AppState>("update_settings", { patch })
      : Promise.resolve({
          ...defaultAppState("浏览器预览模式：设置不会写入系统。"),
          settings: {
            ...defaultAppState("").settings,
            ...patch,
          },
        }),
  acknowledgeRecovery: () =>
    hasTauriRuntime()
      ? invoke<AppState>("acknowledge_recovery")
      : Promise.resolve(defaultAppState("浏览器预览模式。")),
  getDiagnostics: () =>
    hasTauriRuntime()
      ? invoke<AppDiagnostics>("get_diagnostics")
      : Promise.resolve({
          appVersion: "preview",
          codexPath: null,
          codexVersion: null,
          latestSource: null,
          latestSuccessAt: null,
          storagePath: null,
          storageStatus: "missing",
          startupEnabled: false,
          signedUpdatesEnabled: true,
        }),
  setStartupEnabled: (enabled) =>
    hasTauriRuntime()
      ? invoke<boolean>("set_startup_enabled", { enabled })
      : Promise.resolve(enabled),
  hideDetails: () =>
    hasTauriRuntime() ? invoke<void>("hide_details") : Promise.resolve(),
  openOfficialUsage: () =>
    hasTauriRuntime() ? invoke<void>("open_official_usage") : Promise.resolve(),
  getUpdateStatus: () =>
    hasTauriRuntime()
      ? invoke<UpdateStatus>("get_update_status")
      : Promise.resolve(defaultUpdateStatus()),
  checkForUpdates: () =>
    hasTauriRuntime()
      ? invoke<UpdateStatus>("check_for_updates")
      : Promise.resolve(browserPreviewCheckedUpdateStatus()),
  openLatestRelease: () =>
    hasTauriRuntime() ? invoke<void>("open_latest_release") : Promise.resolve(),
  onUpdateStatus: (listener) =>
    hasTauriRuntime()
      ? listen<UpdateStatus>("quotadock:update-status", (event) => listener(event.payload))
      : Promise.resolve(() => {}),
};

function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function defaultAppState(statusMessage: string): AppState {
  const previewSnapshot = browserPreviewSnapshot();
  return {
    version: 3,
    latestSnapshot: previewSnapshot,
    storageStatus: "missing",
    storagePath: null,
    backupPath: null,
    statusMessage: previewSnapshot?.statusMessage ?? statusMessage,
    history: [],
    settings: {
      automaticUpdateChecks: true,
      lowQuotaNotifications: false,
    },
    recoveryNotice: null,
  };
}

function defaultUpdateStatus(): UpdateStatus {
  if (
    import.meta.env.DEV &&
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).get("fixture") === "update-error"
  ) {
    return {
      currentVersion: "0.5.2",
      phase: "error",
      message: "暂时无法连接更新服务，请检查网络或代理后重试。",
      technicalDetail:
        "获取签名更新清单失败：error sending request for url (https://github.com/RupingLiu/QuotaDock/releases/latest/download/latest.json)",
      availableVersion: null,
      progressPercent: null,
      checkedAt: `unix:${Math.floor(Date.now() / 1000)}`,
    };
  }
  return {
    currentVersion: "preview",
    phase: "idle",
    message: "尚未检查软件更新。",
    technicalDetail: null,
    availableVersion: null,
    progressPercent: null,
    checkedAt: null,
  };
}

function browserPreviewCheckedUpdateStatus(): UpdateStatus {
  const status = defaultUpdateStatus();
  if (status.phase === "error") return status;
  return {
    ...status,
    phase: "up-to-date",
    message: "浏览器预览模式未连接桌面更新服务。",
    checkedAt: `unix:${Math.floor(Date.now() / 1000)}`,
  };
}

function browserPreviewSnapshot(): QuotaSnapshot | null {
  if (
    !import.meta.env.DEV ||
    typeof window === "undefined" ||
    new URLSearchParams(window.location.search).get("fixture") !== "weekly-only"
  ) {
    return null;
  }

  const capturedAt = `unix:${Math.floor(Date.now() / 1000)}`;
  return {
    id: capturedAt,
    source: "codex-app-server",
    capturedAt,
    fiveHour: {
      remainingPercent: null,
      resetAt: null,
      resetCountdownSeconds: null,
    },
    weekly: {
      remainingPercent: 76,
      resetAt: null,
      resetCountdownSeconds: null,
    },
    planType: "preview",
    creditsBalance: null,
    resetCreditsAvailable: null,
    rawText: "",
    statusMessage: "已通过 Codex app-server 更新 1 周额度；当前接口未提供 5 小时额度。",
    warnings: [
      {
        code: "missing-five-hour",
        message: "当前账户没有返回短周期额度窗口。",
      },
    ],
  };
}
