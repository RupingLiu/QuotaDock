import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import ConfidenceBadge from "$lib/components/ConfidenceBadge.svelte";

afterEach(() => cleanup());

describe("ConfidenceBadge", () => {
  it("renders partial state labels", () => {
    render(ConfidenceBadge, { props: { state: "partial", warningCount: 0 } });

    expect(screen.getByTestId("confidence-badge").textContent).toContain("Partial");
  });

  it("renders parse warning counts", () => {
    render(ConfidenceBadge, { props: { state: "manual", warningCount: 2 } });

    expect(screen.getByTestId("confidence-badge").textContent).toContain("Manual");
    expect(screen.getByTestId("confidence-badge").textContent).toContain("2 warnings");
  });
});
