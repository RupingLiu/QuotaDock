import { describe, expect, it } from "vitest";
import type {
  ProviderId,
  ProviderSnapshot,
  RefreshProvidersResult,
} from "./usage";

function primaryAmount(snapshot: ProviderSnapshot): string | number | null {
  switch (snapshot.provider) {
    case "codex":
      return snapshot.data.weekly.remainingPercent;
    case "deepseek":
      return snapshot.data.balances[0]?.toppedUpBalance ?? null;
    case "kimi":
      return snapshot.data.total?.limit ?? null;
  }
}

describe("multi-provider DTOs", () => {
  it("narrows tagged snapshots without converting decimal strings", () => {
    const snapshots: ProviderSnapshot[] = [
      {
        provider: "deepseek",
        data: {
          id: "deepseek-1",
          capturedAt: "unix:1",
          isAvailable: true,
          balances: [
            {
              currency: "CNY",
              totalBalance: "110.0000",
              grantedBalance: "10.0000",
              toppedUpBalance: "100.0000",
            },
          ],
        },
      },
      {
        provider: "kimi",
        data: {
          id: "kimi-1",
          capturedAt: "unix:2",
          total: {
            name: "总使用量",
            window: null,
            used: "50410000000000000000",
            limit: "100000000000000000000",
            resetAt: "2030-08-26T00:00:00Z",
          },
          limits: [],
        },
      },
    ];

    expect(snapshots.map(primaryAmount)).toEqual(["100.0000", "100000000000000000000"]);
  });

  it("represents partial refresh results per provider", () => {
    const providerIds: ProviderId[] = ["codex", "deepseek", "kimi"];
    const result = {
      anyUpdated: true,
      providerResults: [
        {
          providerId: "codex",
          outcome: "updated",
          message: "updated",
          errorCategory: null,
        },
        {
          providerId: "deepseek",
          outcome: "failed",
          message: "failed",
          errorCategory: "network",
        },
      ],
    } satisfies Pick<RefreshProvidersResult, "anyUpdated" | "providerResults">;

    expect(providerIds).toEqual(["codex", "deepseek", "kimi"]);
    expect(result.anyUpdated).toBe(true);
    expect(result.providerResults[1]).toMatchObject({
      providerId: "deepseek",
      outcome: "failed",
      errorCategory: "network",
    });
  });
});
