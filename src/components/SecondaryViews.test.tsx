import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClaudeUsage } from "../claude";
import { describeRefreshImpact, UsageView } from "./SecondaryViews";

const baseUsage: ClaudeUsage = {
  source: "claude-statusline",
  status: "available",
  sessionPercent: 37.5,
  sessionResetAtMs: 0,
  weeklyPercent: 64.2,
  weeklyResetAtMs: null,
  updatedAtMs: 1_000_000,
  ageSeconds: 20,
};

describe("UsageView", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("labels the initial read as checking instead of unavailable", () => {
    render(<UsageView usage={null} isLoading />);

    expect(screen.getByText("Checking local usage.")).toBeInTheDocument();
    expect(screen.getAllByText("Checking")).toHaveLength(4);
  });

  it("shows current status-line values with compact reset timing", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-20T16:00:00.000Z"));
    render(
      <UsageView
        usage={{
          ...baseUsage,
          sessionResetAtMs: Date.now() + 90 * 60_000,
        }}
      />,
    );

    expect(screen.getByText("Current")).toBeInTheDocument();
    expect(screen.getByText("1hr 30m")).toBeInTheDocument();
    expect(screen.getByText("37.5%")).toBeInTheDocument();
    expect(screen.getByText("64.2%")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Claude Code", level: 2 })).toBeInTheDocument();
    expect(screen.getByLabelText("Claude Code usage").querySelectorAll("dd")).toHaveLength(3);
    expect(screen.getByText("Status line, updated just now.")).toBeInTheDocument();
  });

  it("identifies available ccstatusline data as a recent cache", () => {
    render(
      <UsageView
        usage={{ ...baseUsage, source: "ccstatusline-cache", ageSeconds: 75 }}
      />,
    );

    expect(screen.getByText("Recent cache")).toBeInTheDocument();
    expect(screen.getByText("ccstatusline cache, updated 1m ago.")).toBeInTheDocument();
  });

  it("hides stale values while explaining their age", () => {
    render(
      <UsageView
        usage={{ ...baseUsage, status: "stale", ageSeconds: 181 }}
      />,
    );

    expect(screen.getByText("Stale")).toBeInTheDocument();
    expect(screen.queryByText("37.5%")).not.toBeInTheDocument();
    expect(screen.queryByText("64.2%")).not.toBeInTheDocument();
    expect(screen.getAllByText("Unavailable")).toHaveLength(3);
    expect(screen.getByText("Updated 3m ago. Old values hidden.")).toBeInTheDocument();
  });

  it("explains unavailable and backend error states without estimating", () => {
    const { rerender } = render(
      <UsageView
        usage={{
          ...baseUsage,
          status: "unavailable",
          reason: "no_usage_data",
        }}
      />,
    );

    expect(screen.getByText("Unavailable", { selector: ".usage-status" })).toBeInTheDocument();
    expect(screen.getByText("Claude has not provided limits. No estimates shown.")).toBeInTheDocument();

    rerender(
      <UsageView
        usage={{ ...baseUsage, status: "error", reason: "rate_limited" }}
      />,
    );
    expect(screen.getByText("Error")).toBeInTheDocument();
    expect(screen.getByText("Usage check is rate limited. Retrying automatically.")).toBeInTheDocument();
  });

  it("distinguishes an invoke failure from a backend unavailable result", () => {
    render(<UsageView usage={null} loadError />);

    expect(screen.getByText("Relay could not read usage. Retrying automatically.")).toBeInTheDocument();
  });

  it("offers live usage only when capture is missing or stale", () => {
    const onEnableLiveUsage = vi.fn();
    const { rerender } = render(
      <UsageView usage={baseUsage} onEnableLiveUsage={onEnableLiveUsage} />,
    );

    expect(screen.queryByRole("button", { name: "Enable live usage" })).not.toBeInTheDocument();

    rerender(
      <UsageView
        usage={{ ...baseUsage, status: "stale", ageSeconds: 181 }}
        onEnableLiveUsage={onEnableLiveUsage}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Enable live usage" }));
    expect(onEnableLiveUsage).toHaveBeenCalledOnce();

    rerender(
      <UsageView
        usage={{ ...baseUsage, source: "ccstatusline-cache" }}
        onEnableLiveUsage={onEnableLiveUsage}
        isEnablingLiveUsage
      />,
    );
    expect(screen.getByRole("button", { name: "Enabling..." })).toBeDisabled();
  });
});

describe("refresh impact guidance", () => {
  it("warns when five-second polling is aggressive for the device or session count", () => {
    expect(describeRefreshImpact(5_000, 4, 3, 4).tone).toBe("warning");
    expect(describeRefreshImpact(5_000, 16, 24, 16).tone).toBe("warning");
  });

  it("keeps capable systems and slower intervals out of the warning state", () => {
    expect(describeRefreshImpact(5_000, 16, 3, 16).tone).toBe("active");
    expect(describeRefreshImpact(30_000, 4, 30, 4).tone).toBe("quiet");
    expect(describeRefreshImpact(60_000, 2, 50, 2).tone).toBe("quiet");
  });
});
