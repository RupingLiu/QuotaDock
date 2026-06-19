import type { QuotaDockApi } from "$lib/api/tauri";
import { tauriApi } from "$lib/api/tauri";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";

export class UsageState {
  appState = $state<AppState | null>(null);
  loading = $state(false);
  refreshing = $state(false);
  errorMessage = $state<string | null>(null);
  noticeMessage = $state<string | null>(null);

  constructor(private readonly api: QuotaDockApi = tauriApi) {}

  get activeSnapshot(): QuotaSnapshot | null {
    return this.appState?.latestSnapshot ?? null;
  }

  async load(): Promise<void> {
    await this.capture(async () => {
      this.loading = true;
      this.appState = await this.api.getAppState();
      this.noticeMessage = this.appState.statusMessage;
    }).finally(() => {
      this.loading = false;
    });
  }

  async refreshUsage(): Promise<void> {
    await this.capture(async () => {
      this.refreshing = true;
      const result = await this.api.refreshUsage();
      this.applyRefreshResult(result);
    }).finally(() => {
      this.refreshing = false;
    });
  }

  applyRefreshResult(result: { appState: AppState; message: string }): void {
    this.errorMessage = null;
    this.appState = result.appState;
    this.noticeMessage = result.message;
  }

  private async capture(work: () => Promise<void>): Promise<void> {
    this.errorMessage = null;
    try {
      await work();
    } catch (error) {
      this.errorMessage = error instanceof Error ? error.message : String(error);
    }
  }
}

export function createUsageState(api?: QuotaDockApi): UsageState {
  return new UsageState(api);
}
