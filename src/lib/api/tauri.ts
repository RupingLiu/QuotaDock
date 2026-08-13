import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppDiagnostics,
  AppState,
  CredentialTarget,
  ProviderCredentialStatus,
  ProviderId,
  ProviderRefreshResult,
  ProviderState,
  ProviderStates,
  QuotaSnapshot,
  RefreshProvidersResult,
  RefreshUsageResult,
  SettingsPatch,
  UpdateStatus,
} from "$lib/types/usage";

export type QuotaDockApi = {
  getAppState(): Promise<AppState>;
  refreshUsage(): Promise<RefreshUsageResult>;
  refreshProviders(): Promise<RefreshProvidersResult>;
  refreshProvider(provider: ProviderId): Promise<RefreshProvidersResult>;
  onProviderState(
    listener: (result: RefreshProvidersResult) => void,
  ): Promise<UnlistenFn>;
  showDashboardContextMenu(x: number, y: number): Promise<void>;
  setProviderCredential(
    target: CredentialTarget,
    secret: string,
  ): Promise<ProviderCredentialStatus>;
  deleteProviderCredential(target: CredentialTarget): Promise<ProviderCredentialStatus>;
  getProviderCredentialStatus(): Promise<ProviderCredentialStatus[]>;
  updateSettings(patch: SettingsPatch): Promise<AppState>;
  acknowledgeRecovery(): Promise<AppState>;
  getDiagnostics(): Promise<AppDiagnostics>;
  setStartupEnabled(enabled: boolean): Promise<boolean>;
  hideDetails(): Promise<void>;
  openOfficialUsage(): Promise<void>;
  openProviderPortal(provider: ProviderId): Promise<void>;
  getUpdateStatus(): Promise<UpdateStatus>;
  checkForUpdates(): Promise<UpdateStatus>;
  installDownloadedUpdate(): Promise<UpdateStatus>;
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
  refreshProviders: () =>
    hasTauriRuntime()
      ? invoke<RefreshProvidersResult>("refresh_providers")
      : Promise.resolve(browserPreviewRefreshResult()),
  refreshProvider: (provider) =>
    hasTauriRuntime()
      ? invoke<RefreshProvidersResult>("refresh_provider", { provider })
      : Promise.resolve(browserPreviewRefreshResult([provider])),
  onProviderState: (listener) =>
    hasTauriRuntime()
      ? listen<RefreshProvidersResult>("provider-state-changed", (event) =>
          listener(event.payload),
        )
      : Promise.resolve(() => {}),
  showDashboardContextMenu: (x, y) =>
    hasTauriRuntime()
      ? invoke<void>("show_dashboard_context_menu", { x, y })
      : Promise.resolve(),
  setProviderCredential: (target, secret) => {
    const args = credentialArguments(target);
    return hasTauriRuntime()
      ? invoke<ProviderCredentialStatus>("set_provider_credential", {
          provider: args.provider,
          region: args.region,
          secret,
        })
      : Promise.resolve(previewCredentialStatus(args, "configured"));
  },
  deleteProviderCredential: (target) => {
    const args = credentialArguments(target);
    return hasTauriRuntime()
      ? invoke<ProviderCredentialStatus>("delete_provider_credential", {
          provider: args.provider,
          region: args.region,
        })
      : Promise.resolve(previewCredentialStatus(args, "not-configured"));
  },
  getProviderCredentialStatus: () =>
    hasTauriRuntime()
      ? invoke<ProviderCredentialStatus[]>("get_provider_credential_status")
      : Promise.resolve([
          {
            providerId: "deepseek",
            region: null,
            availability: "not-configured",
          },
          {
            providerId: "kimi",
            region: "china",
            availability: "not-configured",
          },
        ]),
  updateSettings: (patch) =>
    hasTauriRuntime()
      ? invoke<AppState>("update_settings", { patch })
      : Promise.resolve(browserPreviewUpdatedState(patch)),
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
  openProviderPortal: (provider) =>
    hasTauriRuntime()
      ? invoke<void>("open_provider_portal", { provider })
      : Promise.resolve(),
  getUpdateStatus: () =>
    hasTauriRuntime()
      ? invoke<UpdateStatus>("get_update_status")
      : Promise.resolve(defaultUpdateStatus()),
  checkForUpdates: () =>
    hasTauriRuntime()
      ? invoke<UpdateStatus>("check_for_updates")
      : Promise.resolve(browserPreviewCheckedUpdateStatus()),
  installDownloadedUpdate: () =>
    hasTauriRuntime()
      ? invoke<UpdateStatus>("install_downloaded_update")
      : Promise.resolve(browserPreviewInstallingUpdateStatus()),
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
  const state: AppState = {
    version: 5,
    revision: 0,
    providers: {
      codex: {
        configured: true,
        latestSnapshot: previewSnapshot
          ? { provider: "codex", data: previewSnapshot }
          : null,
        lastAttemptAt: previewSnapshot?.capturedAt ?? null,
        health: previewSnapshot ? "fresh" : "idle",
        errorCategory: null,
      },
      deepseek: unconfiguredProviderState(),
      kimi: unconfiguredProviderState(),
    },
    latestSnapshot: previewSnapshot,
    storageStatus: "missing",
    storagePath: null,
    backupPath: null,
    statusMessage: previewSnapshot?.statusMessage ?? statusMessage,
    history: [],
    settings: {
      automaticUpdateChecks: true,
      lowQuotaNotifications: false,
      floatingProviderIds: ["codex"],
    },
    recoveryNotice: null,
  };
  return applyBrowserProviderFixture(state);
}

function applyBrowserProviderFixture(state: AppState): AppState {
  if (!import.meta.env.DEV || typeof window === "undefined") return state;
  const fixture = new URLSearchParams(window.location.search).get("fixture");
  if (!["providers-all", "providers-partial", "providers-zero", "providers-stale"].includes(fixture ?? "")) return state;
  const capturedAt = fixture === "providers-stale"
    ? `unix:${Math.floor(Date.now() / 1000) - 86_400}`
    : `unix:${Math.floor(Date.now() / 1000)}`;
  state.providers.deepseek = {
    configured: true,
    latestSnapshot: {
      provider: "deepseek",
      data: {
        id: `preview-deepseek-${fixture}`,
        capturedAt,
        isAvailable: fixture !== "providers-partial",
        balances: [{
          currency: "CNY",
          totalBalance: fixture === "providers-zero" ? "0.00" : "108.50",
          grantedBalance: fixture === "providers-zero" ? "0.00" : "8.50",
          toppedUpBalance: fixture === "providers-zero" ? "0.00" : "100.00",
        }],
      },
    },
    lastAttemptAt: capturedAt,
    health: fixture === "providers-partial" ? "error" : fixture === "providers-stale" ? "stale" : "fresh",
    errorCategory: fixture === "providers-partial" ? "network" : null,
  };
  state.providers.kimi = {
    configured: true,
    latestSnapshot: {
      provider: "kimi",
      data: {
        id: `preview-kimi-${fixture}`,
        capturedAt,
        region: "china",
        currency: "CNY",
        availableBalance: fixture === "providers-zero" ? "0" : "49.59",
        cashBalance: fixture === "providers-zero" ? "-1.25" : "40.00",
        voucherBalance: fixture === "providers-zero" ? "0" : "9.59",
      },
    },
    lastAttemptAt: capturedAt,
    health: fixture === "providers-stale" ? "stale" : "fresh",
    errorCategory: null,
  };
  state.settings.floatingProviderIds = ["codex", "deepseek", "kimi"];
  return state;
}

function browserPreviewRefreshResult(
  providerIds: readonly ProviderId[] = ["codex", "deepseek", "kimi"],
): RefreshProvidersResult {
  const appState = defaultAppState("浏览器预览模式无法查询供应商额度。");
  const providerResults: ProviderRefreshResult[] = providerIds.map((providerId) => {
    const state = appState.providers[providerId];
    const name = providerId === "codex" ? "Codex" : providerId === "deepseek" ? "DeepSeek" : "Kimi";
    if (!state.configured) {
      return {
        providerId,
        outcome: "skipped",
        message: `${name} 尚未配置。`,
        errorCategory: "not-configured",
      };
    }
    if (state.health === "error") {
      return {
        providerId,
        outcome: "failed",
        message: `${name} 预览数据模拟刷新失败。`,
        errorCategory: state.errorCategory ?? "invalid-response",
      };
    }
    if (state.health === "stale") {
      return {
        providerId,
        outcome: "unchanged",
        message: `${name} 保留预览中的陈旧数据。`,
        errorCategory: null,
      };
    }
    if (state.latestSnapshot) {
      return {
        providerId,
        outcome: "updated",
        message: `${name} 预览数据已更新。`,
        errorCategory: null,
      };
    }
    return {
      providerId,
      outcome: "failed",
      message: `${name} 在浏览器预览中没有可刷新数据。`,
      errorCategory: "invalid-response",
    };
  });
  const anyUpdated = providerResults.some((result) => result.outcome === "updated");
  const anyFailed = providerResults.some((result) => result.outcome === "failed");
  return {
    appState,
    providerResults,
    anyUpdated,
    message: anyFailed && anyUpdated
      ? "浏览器预览：部分供应商已更新。"
      : anyUpdated
        ? "浏览器预览：所选供应商已更新。"
        : "浏览器预览：没有新的供应商数据。",
  };
}

type CredentialCommandArguments =
  | { provider: "deepseek"; region: null }
  | { provider: "kimi"; region: "china" };

function credentialArguments(target: CredentialTarget): CredentialCommandArguments {
  if (target.provider === "deepseek" && !("region" in target)) {
    return { provider: "deepseek", region: null };
  }
  if (target.provider === "kimi" && target.region === "china") {
    return { provider: "kimi", region: "china" };
  }
  throw new Error("首版只支持 DeepSeek 和 Kimi 国内站凭据。");
}

function previewCredentialStatus(
  args: CredentialCommandArguments,
  availability: ProviderCredentialStatus["availability"],
): ProviderCredentialStatus {
  return args.provider === "deepseek"
    ? { providerId: "deepseek", region: null, availability }
    : { providerId: "kimi", region: "china", availability };
}

function unconfiguredProviderState(): ProviderState {
  return {
    configured: false,
    latestSnapshot: null,
    lastAttemptAt: null,
    health: "not-configured",
    errorCategory: null,
  };
}

function browserPreviewUpdatedState(patch: SettingsPatch): AppState {
  const state = defaultAppState("浏览器预览模式：设置不会写入系统。");
  const requestedProviderIds =
    patch.floatingProviderIds ?? state.settings.floatingProviderIds;
  return {
    ...state,
    settings: {
      ...state.settings,
      automaticUpdateChecks:
        patch.automaticUpdateChecks ?? state.settings.automaticUpdateChecks,
      lowQuotaNotifications:
        patch.lowQuotaNotifications ?? state.settings.lowQuotaNotifications,
      floatingProviderIds: normalizeFloatingProviderIds(
        requestedProviderIds,
        state.providers,
      ),
    },
  };
}

export function normalizeFloatingProviderIds(
  requestedProviderIds: readonly ProviderId[],
  providers: ProviderStates,
): ProviderId[] {
  const selected = new Set(requestedProviderIds);
  const normalized = (["codex", "deepseek", "kimi"] as const).filter(
    (providerId) => selected.has(providerId) && providers[providerId].configured,
  );
  return normalized.length > 0 ? normalized : ["codex"];
}

function defaultUpdateStatus(): UpdateStatus {
  const fixture =
    import.meta.env.DEV && typeof window !== "undefined"
      ? new URLSearchParams(window.location.search).get("fixture")
      : null;
  if (fixture === "update-error") {
    return {
      currentVersion: "0.6.0",
      phase: "error",
      message: "暂时无法连接更新服务，请检查网络或代理后重试。",
      technicalDetail:
        "获取签名更新清单失败：error sending request for url (https://github.com/RupingLiu/QuotaDock/releases/latest/download/latest.json)",
      availableVersion: null,
      progressPercent: null,
      checkedAt: `unix:${Math.floor(Date.now() / 1000)}`,
    };
  }
  if (fixture === "update-ready") {
    return {
      currentVersion: "preview",
      phase: "ready",
      message: "新版本已下载并通过签名校验，可以立即安装。",
      technicalDetail: null,
      availableVersion: "next",
      progressPercent: 100,
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

function browserPreviewInstallingUpdateStatus(): UpdateStatus {
  const status = defaultUpdateStatus();
  return {
    ...status,
    phase: "installing",
    message: "浏览器预览模式不会安装桌面更新。",
  };
}

function browserPreviewSnapshot(): QuotaSnapshot | null {
  if (
    !import.meta.env.DEV ||
    typeof window === "undefined" ||
    !["weekly-only", "providers-all", "providers-partial", "providers-zero", "providers-stale"].includes(
      new URLSearchParams(window.location.search).get("fixture") ?? "",
    )
  ) {
    return null;
  }

  const capturedAt = `unix:${Math.floor(Date.now() / 1000)}`;
  return {
    id: capturedAt,
    source: "codex-app-server",
    capturedAt,
    weekly: {
      remainingPercent: 76,
      resetAt: null,
      resetCountdownSeconds: null,
    },
    planType: "preview",
    creditsBalance: null,
    resetCreditsAvailable: null,
    rawText: "",
    statusMessage: "已通过 Codex app-server 更新 1 周额度。",
    warnings: [],
  };
}
