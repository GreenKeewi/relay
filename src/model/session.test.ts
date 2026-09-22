import { describe, expect, it } from "vitest";

import {
  filterSessions,
  formatAbsoluteTime,
  formatRelativeTime,
  formatSessionTime,
  getSafeSourceTarget,
  isDone,
  needsReview,
  sortByAttention,
  summarizeSessions,
  type RelaySession,
} from "./session";

const NOW = new Date("2026-09-20T16:00:00.000Z");

function sessions(): RelaySession[] {
  return [
    {
      id: "active-session",
      provider: "claude_code",
      project: "Relay",
      title: "Active session",
      latestUpdate: "Working",
      currentStep: "Working",
      status: "in_progress",
      confidence: 1,
      startedAt: "2026-09-20T15:00:00.000Z",
      reviewStatus: "not_requested",
      updatedAt: "2026-09-20T15:58:00.000Z",
      source: { kind: "path", path: "C:\\work\\relay" },
      capabilities: [],
    },
    {
      id: "review-session",
      provider: "claude_code",
      project: "Relay",
      title: "Review session",
      latestUpdate: "Awaiting review",
      currentStep: "Awaiting review",
      status: "ready_for_review",
      confidence: 1,
      startedAt: "2026-09-20T14:00:00.000Z",
      completedAt: "2026-09-20T19:18:00.000Z",
      reviewStatus: "pending",
      updatedAt: "2026-09-20T19:18:00.000Z",
      source: { kind: "path", path: "C:\\work\\relay" },
      capabilities: [],
    },
    {
      id: "done-session",
      provider: "claude_code",
      project: "Relay",
      title: "Completed session",
      latestUpdate: "Complete",
      currentStep: "Complete",
      status: "done",
      confidence: 1,
      startedAt: "2026-09-20T13:00:00.000Z",
      completedAt: "2026-09-20T15:30:00.000Z",
      reviewedAt: "2026-09-20T15:32:00.000Z",
      reviewStatus: "approved",
      updatedAt: "2026-09-20T15:32:00.000Z",
      source: { kind: "path", path: "C:\\work\\relay" },
      capabilities: [],
    },
  ];
}

describe("filterSessions", () => {
  it("matches selected providers and statuses together", () => {
    const result = filterSessions(sessions(), {
      providers: ["claude_code"],
      statuses: ["in_progress"],
    });

    expect(result.map(({ id }) => id)).toEqual(["active-session"]);
  });

  it("returns every session when a filter selection is empty", () => {
    expect(filterSessions(sessions(), { providers: [], statuses: [] })).toHaveLength(3);
  });
});

describe("attention ordering", () => {
  it("puts review work before active work and completed history", () => {
    expect(sortByAttention(sessions()).map(({ id }) => id)).toEqual([
      "review-session",
      "active-session",
      "done-session",
    ]);
  });

  it("keeps review and completion semantics distinct", () => {
    const review = sessions()[1];
    const done = sessions()[2];

    expect(needsReview(review)).toBe(true);
    expect(isDone(review)).toBe(false);
    expect(needsReview(done)).toBe(false);
    expect(isDone(done)).toBe(true);
  });
});

describe("summarizeSessions", () => {
  it("counts active, review, completed, and attention states without overlap", () => {
    expect(summarizeSessions(sessions())).toEqual({
      total: 3,
      inProgress: 1,
      readyForReview: 1,
      done: 1,
      needsAttention: 1,
    });
  });
});

describe("timestamp labels", () => {
  it("formats recent updates as compact relative time", () => {
    expect(formatRelativeTime("2026-09-20T15:58:00.000Z", NOW)).toBe("2m ago");
    expect(formatRelativeTime("2026-09-20T15:42:00.000Z", NOW)).toBe("18m ago");
  });

  it("formats an absolute clock time with an explicit time zone", () => {
    expect(
      formatAbsoluteTime("2026-09-20T19:18:00.000Z", {
        timeZone: "America/Toronto",
      }),
    ).toBe("3:18 PM");
  });

  it("labels review-ready sessions by completion time", () => {
    const review = sessions()[1];
    expect(
      formatSessionTime(review, NOW, { timeZone: "America/Toronto" }),
    ).toBe("completed 3:18 PM");
  });
});

describe("safe source semantics", () => {
  it("allows HTTPS URLs and explicit local paths", () => {
    expect(
      getSafeSourceTarget({ kind: "url", url: "https://example.com/report" }),
    ).toEqual({ kind: "url", value: "https://example.com/report" });
    expect(getSafeSourceTarget({ kind: "path", path: "C:\\work\\relay" })).toEqual({
      kind: "path",
      value: "C:\\work\\relay",
    });
  });

  it("rejects executable URL schemes, credentials, and malformed paths", () => {
    expect(getSafeSourceTarget({ kind: "url", url: "javascript:alert(1)" })).toBeNull();
    expect(
      getSafeSourceTarget({ kind: "url", url: "https://secret@example.com/report" }),
    ).toBeNull();
    expect(getSafeSourceTarget({ kind: "path", path: "  " })).toBeNull();
    expect(getSafeSourceTarget({ kind: "path", path: "C:\\work\0relay" })).toBeNull();
  });
});
