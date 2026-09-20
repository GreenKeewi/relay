export type ClaudeSessionState = "recent" | "idle";
export type ClaudeLifecycleStatus = "working" | "to_review" | "done" | "failed" | "waiting" | "recent" | "idle";
export type ClaudeStatusFilter = "All" | "In progress" | "To review" | "Done" | "Archived";

type ClaudeStatusSource = {
  state: ClaudeSessionState;
  status?: ClaudeLifecycleStatus;
  archived?: boolean;
};

export function resolveClaudeLifecycleStatus(
  session: ClaudeStatusSource,
): Exclude<ClaudeLifecycleStatus, "recent" | "idle"> {
  if (session.status && session.status !== "recent" && session.status !== "idle") {
    return session.status;
  }

  return session.state === "recent" ? "working" : "to_review";
}

export function matchesClaudeStatusFilter(
  session: ClaudeStatusSource,
  filter: ClaudeStatusFilter,
) {
  if (filter === "Archived") return session.archived === true;
  if (session.archived) return false;
  if (filter === "All") return true;

  const status = resolveClaudeLifecycleStatus(session);
  if (filter === "In progress") return status === "working";
  if (filter === "Done") return status === "done";
  return status === "to_review" || status === "waiting" || status === "failed";
}

export function countClaudeStatusFilters(sessions: readonly ClaudeStatusSource[]) {
  return {
    All: sessions.filter((session) => matchesClaudeStatusFilter(session, "All")).length,
    "In progress": sessions.filter((session) => matchesClaudeStatusFilter(session, "In progress")).length,
    "To review": sessions.filter((session) => matchesClaudeStatusFilter(session, "To review")).length,
    Done: sessions.filter((session) => matchesClaudeStatusFilter(session, "Done")).length,
    Archived: sessions.filter((session) => matchesClaudeStatusFilter(session, "Archived")).length,
  } satisfies Record<ClaudeStatusFilter, number>;
}
