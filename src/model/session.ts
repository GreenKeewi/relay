export const SESSION_PROVIDERS = ["codex", "claude_code", "chatgpt_work", "zed"] as const;
export const SESSION_STATUSES = [
  "in_progress",
  "ready_for_review",
  "done",
  "failed",
  "paused",
] as const;

export type SessionProvider = (typeof SESSION_PROVIDERS)[number];
export type SessionStatus = (typeof SESSION_STATUSES)[number];
export type ReviewStatus =
  | "not_requested"
  | "pending"
  | "approved"
  | "changes_requested"
  | "not_required";
export type SessionCapability =
  | "open_source"
  | "open_repository"
  | "open_worktree"
  | "resume"
  | "review";

export type SessionSource =
  | { kind: "url"; url: string }
  | { kind: "path"; path: string };

export interface RelaySession {
  id: string;
  provider: SessionProvider;
  project: string;
  title: string;
  latestUpdate: string;
  currentStep: string;
  status: SessionStatus;
  confidence: number;
  startedAt: string;
  completedAt?: string;
  reviewedAt?: string;
  reviewStatus: ReviewStatus;
  updatedAt: string;
  source: SessionSource;
  repository?: string;
  branch?: string;
  worktree?: string;
  capabilities: readonly SessionCapability[];
}

export interface SessionFilter {
  providers?: readonly SessionProvider[];
  statuses?: readonly SessionStatus[];
  query?: string;
}

export interface SessionSummary {
  total: number;
  inProgress: number;
  readyForReview: number;
  done: number;
  needsAttention: number;
}

export interface TimeFormatOptions {
  locale?: string;
  timeZone?: string;
}

export type SafeSourceTarget =
  | { kind: "url"; value: string }
  | { kind: "path"; value: string };

export const PROVIDER_LABELS: Readonly<Record<SessionProvider, string>> = {
  codex: "Codex",
  claude_code: "Claude Code",
  chatgpt_work: "ChatGPT Work",
  zed: "Zed",
};

export const STATUS_LABELS: Readonly<Record<SessionStatus, string>> = {
  in_progress: "In progress",
  ready_for_review: "To review",
  done: "Done",
  failed: "Failed",
  paused: "Paused",
};

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function timestamp(value: string | Date): number | null {
  const milliseconds = value instanceof Date ? value.getTime() : Date.parse(value);
  return Number.isFinite(milliseconds) ? milliseconds : null;
}

export function filterSessions(
  sessions: readonly RelaySession[],
  filter: SessionFilter = {},
): RelaySession[] {
  const providerSet = new Set(filter.providers ?? []);
  const statusSet = new Set(filter.statuses ?? []);
  const query = filter.query?.trim().toLocaleLowerCase() ?? "";

  return sessions.filter((session) => {
    if (providerSet.size > 0 && !providerSet.has(session.provider)) return false;
    if (statusSet.size > 0 && !statusSet.has(session.status)) return false;
    if (!query) return true;

    return [
      PROVIDER_LABELS[session.provider],
      session.project,
      session.title,
      session.latestUpdate,
      session.currentStep,
      session.repository,
      session.branch,
    ].some((value) => value?.toLocaleLowerCase().includes(query));
  });
}

export function needsReview(session: RelaySession): boolean {
  return (
    session.status === "ready_for_review" ||
    session.reviewStatus === "pending" ||
    session.reviewStatus === "changes_requested"
  );
}

export function isDone(session: RelaySession): boolean {
  return session.status === "done";
}

export function needsAttention(session: RelaySession): boolean {
  return session.status === "failed" || needsReview(session);
}

function attentionRank(session: RelaySession): number {
  if (session.status === "failed" || session.reviewStatus === "changes_requested") return 0;
  if (needsReview(session)) return 1;
  if (session.status === "in_progress") return 2;
  if (session.status === "paused") return 3;
  return 4;
}

export function sortByAttention(sessions: readonly RelaySession[]): RelaySession[] {
  return [...sessions].sort((left, right) => {
    const rankDifference = attentionRank(left) - attentionRank(right);
    if (rankDifference !== 0) return rankDifference;

    return (timestamp(right.updatedAt) ?? 0) - (timestamp(left.updatedAt) ?? 0);
  });
}

export function summarizeSessions(sessions: readonly RelaySession[]): SessionSummary {
  return sessions.reduce<SessionSummary>(
    (summary, session) => {
      summary.total += 1;
      if (session.status === "in_progress") summary.inProgress += 1;
      if (session.status === "ready_for_review") summary.readyForReview += 1;
      if (session.status === "done") summary.done += 1;
      if (needsAttention(session)) summary.needsAttention += 1;
      return summary;
    },
    { total: 0, inProgress: 0, readyForReview: 0, done: 0, needsAttention: 0 },
  );
}

export function formatRelativeTime(value: string | Date, now: Date = new Date()): string {
  const valueTime = timestamp(value);
  const nowTime = timestamp(now);
  if (valueTime === null || nowTime === null) return "Unknown";

  const difference = nowTime - valueTime;
  const absoluteDifference = Math.abs(difference);
  if (absoluteDifference < MINUTE) return "just now";

  const suffix = difference >= 0 ? "ago" : null;
  const prefix = difference < 0 ? "in " : "";
  let amount: number;
  let unit: string;

  if (absoluteDifference < HOUR) {
    amount = Math.round(absoluteDifference / MINUTE);
    unit = "m";
  } else if (absoluteDifference < DAY) {
    amount = Math.round(absoluteDifference / HOUR);
    unit = "h";
  } else {
    amount = Math.round(absoluteDifference / DAY);
    unit = "d";
  }

  return suffix ? `${amount}${unit} ${suffix}` : `${prefix}${amount}${unit}`;
}

export function formatAbsoluteTime(
  value: string | Date,
  options: TimeFormatOptions = {},
): string {
  const valueTime = timestamp(value);
  if (valueTime === null) return "Unknown";

  return new Intl.DateTimeFormat(options.locale ?? "en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: options.timeZone,
  }).format(valueTime);
}

export function formatSessionTime(
  session: RelaySession,
  now: Date = new Date(),
  options: TimeFormatOptions = {},
): string {
  if (needsReview(session) && session.completedAt) {
    return `completed ${formatAbsoluteTime(session.completedAt, options)}`;
  }

  return formatRelativeTime(session.updatedAt, now);
}

export function getSafeSourceTarget(source: SessionSource): SafeSourceTarget | null {
  if (source.kind === "url") {
    const value = source.url.trim();
    try {
      const parsed = new URL(value);
      if (parsed.protocol !== "https:" || parsed.username || parsed.password || !parsed.hostname) {
        return null;
      }
      return { kind: "url", value };
    } catch {
      return null;
    }
  }

  const value = source.path.trim();
  const isWindowsAbsolute = /^[A-Za-z]:[\\/]/.test(value);
  const isPosixAbsolute = value.startsWith("/");
  const isUncAbsolute = value.startsWith("\\\\");
  const hasParentTraversal = /(^|[\\/])\.\.([\\/]|$)/.test(value);

  if (
    !value ||
    value.includes("\0") ||
    hasParentTraversal ||
    (!isWindowsAbsolute && !isPosixAbsolute && !isUncAbsolute)
  ) {
    return null;
  }

  return { kind: "path", value };
}
