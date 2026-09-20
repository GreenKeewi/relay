import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ClaudeSessionList, type ClaudeLocalSession } from "./ClaudeSessionList";
import {
  countClaudeStatusFilters,
  matchesClaudeStatusFilter,
  resolveClaudeLifecycleStatus,
} from "../model/claudeStatus";

afterEach(cleanup);

const reviewSession: ClaudeLocalSession = {
  id: "12345678-1234-4abc-8def-1234567890ab",
  project: "Relay",
  name: "Review onboarding state",
  cwd: "C:\\Users\\User\\Documents\\code\\Relay",
  updatedAt: "2026-09-20T18:00:00.000Z",
  state: "idle",
  source: "Claude Code",
  resumeAvailable: true,
};

describe("Claude session lifecycle status", () => {
  it("uses explicit lifecycle states when Claude provides one", () => {
    expect(resolveClaudeLifecycleStatus({ state: "recent", status: "done" })).toBe("done");
    expect(resolveClaudeLifecycleStatus({ state: "idle", status: "working" })).toBe("working");
  });

  it("maps observable activity to honest lifecycle fallbacks", () => {
    expect(resolveClaudeLifecycleStatus({ state: "recent" })).toBe("working");
    expect(resolveClaudeLifecycleStatus({ state: "idle" })).toBe("to_review");
  });

  it("matches the visible status filter groups", () => {
    expect(matchesClaudeStatusFilter({ state: "recent" }, "In progress")).toBe(true);
    expect(matchesClaudeStatusFilter({ state: "idle" }, "To review")).toBe(true);
    expect(matchesClaudeStatusFilter({ state: "idle", status: "waiting" }, "To review")).toBe(true);
    expect(matchesClaudeStatusFilter({ state: "recent", status: "done" }, "Done")).toBe(true);
    expect(matchesClaudeStatusFilter({ state: "recent", status: "done" }, "In progress")).toBe(false);
    expect(matchesClaudeStatusFilter({ state: "idle", archived: true }, "Archived")).toBe(true);
    expect(matchesClaudeStatusFilter({ state: "idle", archived: true }, "All")).toBe(false);
  });

  it("counts statuses from the agent-filtered session set", () => {
    expect(countClaudeStatusFilters([
      { state: "recent" },
      { state: "idle" },
      { state: "recent", status: "done" },
      { state: "idle", archived: true },
    ])).toEqual({
      All: 3,
      "In progress": 1,
      "To review": 1,
      Done: 1,
      Archived: 1,
    });
  });
});

describe("ClaudeSessionList review action", () => {
  it("marks a review item as read without opening its chat", () => {
    const onOpenSession = vi.fn();
    const onMarkRead = vi.fn();
    render(
      <ClaudeSessionList
        sessions={[reviewSession]}
        onOpenSession={onOpenSession}
        onMarkRead={onMarkRead}
        now={new Date("2026-09-20T18:10:00.000Z")}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Mark Review onboarding state as read" }));

    expect(onMarkRead).toHaveBeenCalledWith(reviewSession);
    expect(onOpenSession).not.toHaveBeenCalled();
  });

  it("does not show Read for completed sessions", () => {
    render(
      <ClaudeSessionList
        sessions={[{ ...reviewSession, status: "done" }]}
        onOpenSession={vi.fn()}
        onMarkRead={vi.fn()}
      />,
    );

    expect(screen.queryByRole("button", { name: /mark .* as read/i })).not.toBeInTheDocument();
  });

  it("selects a session without opening it while selection mode is active", () => {
    const onOpenSession = vi.fn();
    const onToggleSession = vi.fn();
    render(
      <ClaudeSessionList
        sessions={[reviewSession]}
        onOpenSession={onOpenSession}
        onMarkRead={vi.fn()}
        selectionMode
        selectedSessionIds={new Set()}
        onToggleSession={onToggleSession}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Select Review onboarding state" }));

    expect(onToggleSession).toHaveBeenCalledWith(reviewSession);
    expect(onOpenSession).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /mark .* as read/i })).not.toBeInTheDocument();
  });
});
