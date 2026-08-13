export type SnapshotSource = "pasted-status" | "codex-cli" | "codex-app-server";

export type ProviderId = "codex" | "deepseek" | "kimi";

export type StorageStatus = "ready" | "missing" | "recovered" | "unsupported-version";

export type ParseWarning = {
  code: string;
  message: string;
};

export type QuotaReading = {
  remainingPercent: number | null;
  resetAt: string | null;
  resetCountdownSeconds: number | null;
};

export type QuotaSnapshot = {
  id: string;
  source: SnapshotSource;
  capturedAt: string;
  weekly: QuotaReading;
  planType: string | null;
  creditsBalance: string | null;
  resetCreditsAvailable: number | null;
  rawText: string;
  statusMessage: string;
  warnings: ParseWarning[];
};

export type CodexSnapshot = QuotaSnapshot;

export type DeepSeekBalance = {
  currency: string;
  totalBalance: string;
  grantedBalance: string;
  toppedUpBalance: string;
};

export type DeepSeekSnapshot = {
  id: string;
  capturedAt: string;
  isAvailable: boolean;
  balances: DeepSeekBalance[];
};

export type KimiRegion = "china";

export type CredentialAvailability =
  | "configured"
  | "not-configured"
  | "unavailable";

export type CredentialTarget =
  | { provider: "deepseek" }
  | { provider: "kimi"; region: "china" };

export type ProviderCredentialStatus =
  | {
      providerId: "deepseek";
      region: null;
      availability: CredentialAvailability;
    }
  | {
      providerId: "kimi";
      region: "china";
      availability: CredentialAvailability;
    };

export type KimiSnapshot = {
  id: string;
  capturedAt: string;
  region: KimiRegion;
  currency: string;
  availableBalance: string;
  cashBalance: string;
  voucherBalance: string;
};

export type ProviderSnapshot =
  | { provider: "codex"; data: CodexSnapshot }
  | { provider: "deepseek"; data: DeepSeekSnapshot }
  | { provider: "kimi"; data: KimiSnapshot };

export type ProviderHealth =
  | "not-configured"
  | "idle"
  | "fresh"
  | "refreshing"
  | "stale"
  | "error";

export type ProviderErrorCategory =
  | "not-configured"
  | "busy"
  | "unauthorized"
  | "insufficient-balance"
  | "rate-limited"
  | "timeout"
  | "network"
  | "server"
  | "invalid-response"
  | "credential-store";

export type ProviderState = {
  configured: boolean;
  latestSnapshot: ProviderSnapshot | null;
  lastAttemptAt: string | null;
  health: ProviderHealth;
  errorCategory: ProviderErrorCategory | null;
};

export type ProviderStates = {
  codex: ProviderState;
  deepseek: ProviderState;
  kimi: ProviderState;
};

export type ParseResult = {
  snapshot: QuotaSnapshot;
};

export type UsageHistoryPoint = {
  capturedAt: string;
  weeklyRemainingPercent: number | null;
};

export type AppSettings = {
  automaticUpdateChecks: boolean;
  lowQuotaNotifications: boolean;
  floatingProviderIds: ProviderId[];
};

export type RecoveryNotice = {
  status: StorageStatus;
  message: string;
  backupPath: string;
};

export type AppState = {
  version: number;
  revision: number;
  providers: ProviderStates;
  /** Read-only compatibility projection of providers.codex.latestSnapshot.data. */
  latestSnapshot: QuotaSnapshot | null;
  storageStatus: StorageStatus;
  storagePath: string | null;
  backupPath: string | null;
  statusMessage: string;
  history: UsageHistoryPoint[];
  settings: AppSettings;
  recoveryNotice: RecoveryNotice | null;
};

export type RefreshUsageResult = {
  appState: AppState;
  updated: boolean;
  message: string;
};

export type ProviderRefreshOutcome =
  | "updated"
  | "unchanged"
  | "skipped"
  | "failed";

export type ProviderRefreshResult = {
  providerId: ProviderId;
  outcome: ProviderRefreshOutcome;
  message: string;
  errorCategory: ProviderErrorCategory | null;
};

export type RefreshProvidersResult = {
  appState: AppState;
  providerResults: ProviderRefreshResult[];
  anyUpdated: boolean;
  message: string;
};

export type SettingsPatch = {
  automaticUpdateChecks?: boolean;
  lowQuotaNotifications?: boolean;
  floatingProviderIds?: ProviderId[];
};

export type AppDiagnostics = {
  appVersion: string;
  codexPath: string | null;
  codexVersion: string | null;
  latestSource: SnapshotSource | null;
  latestSuccessAt: string | null;
  storagePath: string | null;
  storageStatus: StorageStatus;
  startupEnabled: boolean;
  signedUpdatesEnabled: boolean;
};

export type UpdatePhase =
  | "idle"
  | "checking"
  | "up-to-date"
  | "downloading"
  | "ready"
  | "installing"
  | "error";

export type UpdateStatus = {
  currentVersion: string;
  phase: UpdatePhase;
  message: string;
  technicalDetail: string | null;
  availableVersion: string | null;
  progressPercent: number | null;
  checkedAt: string | null;
};
