import { afterEach, describe, expect, it } from "vitest";
import type { CredentialTarget, ProviderStates } from "$lib/types/usage";
import { normalizeFloatingProviderIds, tauriApi } from "./tauri";

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
  window.history.replaceState({}, "", "/");
});

function configuredProviders(): ProviderStates {
  return {
    codex: {
      configured: true,
      latestSnapshot: null,
      lastAttemptAt: null,
      health: "idle",
      errorCategory: null,
    },
    deepseek: {
      configured: true,
      latestSnapshot: null,
      lastAttemptAt: null,
      health: "idle",
      errorCategory: null,
    },
    kimi: {
      configured: true,
      latestSnapshot: null,
      lastAttemptAt: null,
      health: "idle",
      errorCategory: null,
    },
  };
}

describe("browser preview settings", () => {
  it("normalizes floating providers to fixed order without duplicates", () => {
    expect(
      normalizeFloatingProviderIds(
        ["kimi", "deepseek", "codex", "deepseek"],
        configuredProviders(),
      ),
    ).toEqual(["codex", "deepseek", "kimi"]);
  });

  it("filters unconfigured providers and falls back to codex", async () => {
    const state = await tauriApi.updateSettings({
      automaticUpdateChecks: false,
      lowQuotaNotifications: true,
      floatingProviderIds: ["kimi", "deepseek", "deepseek"],
    });

    expect(state.settings).toEqual({
      automaticUpdateChecks: false,
      lowQuotaNotifications: true,
      floatingProviderIds: ["codex"],
    });
  });

  it("prevents an explicitly empty floating selection", async () => {
    const state = await tauriApi.updateSettings({ floatingProviderIds: [] });

    expect(state.settings.floatingProviderIds).toEqual(["codex"]);
  });
});

describe("browser preview provider refresh fixtures", () => {
  it("returns three configured successes for the all-provider fixture", async () => {
    window.history.replaceState({}, "", "/?fixture=providers-all");
    const result = await tauriApi.refreshProviders();
    expect(result.appState.settings.floatingProviderIds).toEqual([
      "codex",
      "deepseek",
      "kimi",
    ]);
    expect(result.providerResults.map((item) => [item.providerId, item.outcome])).toEqual([
      ["codex", "updated"],
      ["deepseek", "updated"],
      ["kimi", "updated"],
    ]);
    expect(result.anyUpdated).toBe(true);
  });

  it("matches the partial fixture and preserves single-provider semantics", async () => {
    window.history.replaceState({}, "", "/?fixture=providers-partial");
    const all = await tauriApi.refreshProviders();
    expect(all.appState.providers.deepseek.configured).toBe(true);
    expect(all.appState.providers.deepseek.health).toBe("error");
    expect(all.providerResults.find((item) => item.providerId === "deepseek")).toMatchObject({
      outcome: "failed",
      errorCategory: "network",
    });
    expect(all.providerResults.find((item) => item.providerId === "kimi")?.outcome).toBe(
      "updated",
    );
    expect(all.message).toContain("部分供应商");

    const deepseek = await tauriApi.refreshProvider("deepseek");
    expect(deepseek.providerResults).toHaveLength(1);
    expect(deepseek.providerResults[0]?.providerId).toBe("deepseek");
    expect(deepseek.anyUpdated).toBe(false);
  });
});

describe("browser preview credential boundary", () => {
  it("uses the same DeepSeek and Kimi China targets as the Rust commands", async () => {
    await expect(
      tauriApi.setProviderCredential({ provider: "deepseek" }, "preview-secret"),
    ).resolves.toEqual({
      providerId: "deepseek",
      region: null,
      availability: "configured",
    });
    await expect(
      tauriApi.deleteProviderCredential({ provider: "kimi", region: "china" }),
    ).resolves.toEqual({
      providerId: "kimi",
      region: "china",
      availability: "not-configured",
    });
    await expect(tauriApi.getProviderCredentialStatus()).resolves.toEqual([
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
    ]);
  });

  it("rejects a runtime attempt to omit the Kimi China region", () => {
    const unsupported = { provider: "kimi" } as unknown as CredentialTarget;

    expect(() =>
      tauriApi.deleteProviderCredential(unsupported),
    ).toThrow("首版只支持 DeepSeek 和 Kimi 国内站凭据。");
  });
});
