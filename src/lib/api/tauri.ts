import { invoke } from "@tauri-apps/api/core";
import type {
  AppState,
  ManualUpdateInput,
  ParseResult,
  Settings,
  UsageSnapshot,
  CodexHealth,
} from "$lib/types/usage";

export type QuotaDockApi = {
  getAppState(): Promise<AppState>;
  parseStatusText(rawText: string): Promise<ParseResult>;
  saveSnapshot(snapshot: UsageSnapshot): Promise<AppState>;
  updateManualFields(input: ManualUpdateInput): Promise<AppState>;
  refreshCodexProbe(): Promise<CodexHealth>;
  updateSettings(settings: Settings): Promise<AppState>;
  backupAndResetStore(): Promise<AppState>;
  openOfficialUsage(): Promise<void>;
};

export const tauriApi: QuotaDockApi = {
  getAppState: () => invoke<AppState>("get_app_state"),
  parseStatusText: (rawText) => invoke<ParseResult>("parse_status_text", { rawText }),
  saveSnapshot: (snapshot) => invoke<AppState>("save_snapshot", { snapshot }),
  updateManualFields: (input) => invoke<AppState>("update_manual_fields", { input }),
  refreshCodexProbe: () => invoke<CodexHealth>("refresh_codex_probe"),
  updateSettings: (settings) => invoke<AppState>("update_settings", { settings }),
  backupAndResetStore: () => invoke<AppState>("backup_and_reset_store"),
  openOfficialUsage: () => invoke<void>("open_official_usage"),
};
