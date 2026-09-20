import { invoke } from "@tauri-apps/api/core";

export type ClaudeSessionState = "recent" | "idle";

export interface ClaudeDetectionResult {
  source: "claude-code";
  installed: boolean;
  cliAvailable: boolean;
  projectsDirectory: string;
  projectsDirectoryExists: boolean;
  sessionFilesSeen: number;
  sessionsDiscovered: number;
  unreadableFiles: number;
  skippedNestedDirectories: number;
}

export interface ClaudeSessionSummary {
  id: string;
  source: "claude-code";
  project: string;
  name?: string | null;
  sessionName?: string | null;
  agentName?: string | null;
  isSubagent?: boolean;
  activity?: string | null;
  safeActivity?: string | null;
  thinking?: boolean;
  status?: "working" | "to_review" | "done" | "failed" | "waiting" | "recent" | "idle";
  cwd: string;
  repository?: string | null;
  branch?: string | null;
  title: string;
  lastActivityMs: number;
  state: ClaudeSessionState;
  resumeAvailable: boolean;
}

export interface ClaudeDiscoveryResult {
  detection: ClaudeDetectionResult;
  sessions: ClaudeSessionSummary[];
}

export type ClaudeUsageReason =
  | "live_snapshot_not_found"
  | "cache_not_found"
  | "cache_unreadable"
  | "cache_too_large"
  | "invalid_cache"
  | "no_usage_data"
  | "timeout"
  | "rate_limited"
  | "api_error"
  | "parse_error"
  | "no_credentials"
  | "unknown_upstream_error";

export interface ClaudeUsage {
  source: "claude-statusline" | "ccstatusline-cache";
  status: "available" | "stale" | "unavailable" | "error";
  reason?: ClaudeUsageReason | null;
  sessionPercent?: number | null;
  sessionResetAtMs?: number | null;
  weeklyPercent?: number | null;
  weeklyResetAtMs?: number | null;
  updatedAtMs?: number | null;
  ageSeconds?: number | null;
}

export function discoverClaudeSessions() {
  return invoke<ClaudeDiscoveryResult>("discover_claude_sessions");
}

export function resumeClaudeSession(sessionId: string, cwd: string) {
  return invoke<void>("resume_claude_session", { sessionId, cwd });
}

export function readClaudeUsage() {
  return invoke<ClaudeUsage>("read_claude_usage");
}

export function enableClaudeLiveUsage() {
  return invoke<void>("enable_claude_live_usage");
}

export function openClaudeInstallGuide() {
  return invoke<void>("open_session", {
    source: "https://code.claude.com/docs/en/setup",
  });
}
