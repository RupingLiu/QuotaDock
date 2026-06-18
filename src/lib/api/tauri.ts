import { invoke } from "@tauri-apps/api/core";
import type { AppState, ParseResult, QuotaSnapshot, RefreshUsageResult } from "$lib/types/usage";

export type QuotaDockApi = {
  getAppState(): Promise<AppState>;
  refreshUsage(): Promise<RefreshUsageResult>;
  parseStatusText(rawText: string): Promise<ParseResult>;
  saveSnapshot(snapshot: QuotaSnapshot): Promise<AppState>;
  clearSnapshot(): Promise<AppState>;
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
  parseStatusText: (rawText) =>
    hasTauriRuntime()
      ? invoke<ParseResult>("parse_status_text", { rawText })
      : Promise.reject(new Error("请在桌面应用中粘贴 /status 解析。")),
  saveSnapshot: (snapshot) =>
    hasTauriRuntime()
      ? invoke<AppState>("save_snapshot", { snapshot })
      : Promise.resolve({
          ...defaultAppState("浏览器预览模式不会保存额度。"),
          latestSnapshot: snapshot,
        }),
  clearSnapshot: () =>
    hasTauriRuntime()
      ? invoke<AppState>("clear_snapshot")
      : Promise.resolve(defaultAppState("已清空浏览器预览。")),
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
