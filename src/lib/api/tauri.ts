import { invoke } from "@tauri-apps/api/core";
import type { AppState, RefreshUsageResult } from "$lib/types/usage";

export type QuotaDockApi = {
  getAppState(): Promise<AppState>;
  refreshUsage(): Promise<RefreshUsageResult>;
  showDashboardContextMenu(x: number, y: number): Promise<void>;
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
};

function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function defaultAppState(statusMessage: string): AppState {
  return {
    version: 2,
    latestSnapshot: null,
    storageStatus: "missing",
    storagePath: null,
    backupPath: null,
    statusMessage,
  };
}
