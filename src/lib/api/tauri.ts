import { invoke } from "@tauri-apps/api/core";
import type {
  AppDiagnostics,
  AppState,
  RefreshUsageResult,
  SettingsPatch,
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
};

function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function defaultAppState(statusMessage: string): AppState {
  return {
    version: 3,
    latestSnapshot: null,
    storageStatus: "missing",
    storagePath: null,
    backupPath: null,
    statusMessage,
    history: [],
    settings: {
      automaticUpdateChecks: true,
      lowQuotaNotifications: false,
    },
    recoveryNotice: null,
  };
}
