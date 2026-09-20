import {
  ArrowUpRight,
  Check,
  CheckSquare,
  Square,
} from "@phosphor-icons/react";
import { AgentLogo } from "./AgentLogo";
import {
  resolveClaudeLifecycleStatus,
  type ClaudeLifecycleStatus,
  type ClaudeSessionState,
} from "../model/claudeStatus";

export interface ClaudeLocalSession {
  id: string;
  project: string;
  name?: string;
  agentName?: string;
  isSubagent?: boolean;
  activity?: string;
  thinking?: boolean;
  status?: ClaudeLifecycleStatus;
  archived?: boolean;
  cwd: string;
  updatedAt: string;
  state: ClaudeSessionState;
  source: "Claude Code";
  resumeAvailable: boolean;
}

export interface ClaudeSessionListProps {
  sessions: readonly ClaudeLocalSession[];
  onOpenSession: (session: ClaudeLocalSession) => void | Promise<void>;
  onMarkRead?: (session: ClaudeLocalSession) => void;
  selectionMode?: boolean;
  selectedSessionIds?: ReadonlySet<string>;
  onToggleSession?: (session: ClaudeLocalSession) => void;
  openingSessionId?: string | null;
  now?: Date | number;
}

const CLAUDE_SESSION_LIST_STYLES = `
  .claude-session-list {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .claude-session-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    width: 100%;
    min-height: 62px;
    border: 1px solid var(--line, rgba(255, 255, 255, 0.095));
    border-radius: 11px;
    color: var(--text, #efeff2);
    background: rgba(255, 255, 255, 0.035);
    overflow: hidden;
    transition: border-color 140ms ease, background-color 140ms ease;
  }

  .claude-session-row:hover {
    border-color: var(--line-strong, rgba(255, 255, 255, 0.16));
    background: var(--surface-hover, rgba(255, 255, 255, 0.082));
  }

  .claude-session-open-button {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) 18px;
    align-items: center;
    gap: 10px;
    min-width: 0;
    min-height: 60px;
    padding: 9px 10px;
    border: 0;
    color: inherit;
    text-align: left;
    background: transparent;
    cursor: pointer;
    transition: transform 100ms ease;
  }

  .claude-session-row:not([data-reviewable="true"]) .claude-session-open-button {
    grid-column: 1 / -1;
  }

  .claude-session-open-button:active:not(:disabled) {
    transform: scale(0.985);
  }

  .claude-session-open-button:disabled {
    cursor: wait;
    opacity: 0.66;
  }

  .claude-session-read {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    min-height: 25px;
    margin-right: 8px;
    padding: 4px 8px;
    border: 1px solid color-mix(in srgb, var(--review, #d2a65f) 34%, transparent);
    border-radius: 999px;
    color: var(--review, #d2a65f);
    background: color-mix(in srgb, var(--review, #d2a65f) 8%, transparent);
    cursor: pointer;
    font-size: 8px;
    font-weight: 700;
    white-space: nowrap;
    transition: color 140ms ease, background-color 140ms ease, transform 100ms ease;
  }

  .claude-session-read:hover {
    color: var(--text, #efeff2);
    background: color-mix(in srgb, var(--review, #d2a65f) 16%, transparent);
  }

  .claude-session-read:active {
    transform: scale(0.96);
  }

  .claude-session-read svg {
    width: 9px;
    height: 9px;
  }

  .claude-session-folder {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 1px solid rgba(208, 177, 126, 0.22);
    border-radius: 9px;
    color: #d0b17e;
    background: rgba(208, 177, 126, 0.08);
  }

  .claude-session-folder svg {
    width: 14px;
    height: 14px;
  }

  .claude-session-folder .agent-logo {
    width: 16px;
    height: 16px;
  }

  .claude-session-folder[data-selected="true"] {
    color: var(--done, #78ad88);
    border-color: color-mix(in srgb, var(--done, #78ad88) 36%, transparent);
    background: color-mix(in srgb, var(--done, #78ad88) 10%, transparent);
  }

  .claude-session-row[data-selection-mode="true"] {
    border-color: color-mix(in srgb, var(--accent, #9698c0) 28%, var(--line, transparent));
  }

  .claude-session-row[data-selected="true"] {
    border-color: color-mix(in srgb, var(--done, #78ad88) 46%, transparent);
    background: color-mix(in srgb, var(--done, #78ad88) 8%, rgba(255, 255, 255, 0.035));
  }

  .claude-session-copy {
    display: grid;
    min-width: 0;
    gap: 3px;
  }

  .claude-session-title-line {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 7px;
  }

  .claude-session-dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    border-radius: 999px;
    background: #6b6e76;
    box-shadow: 0 0 0 2px rgba(107, 110, 118, 0.12);
  }

  .claude-session-dot[data-status="working"] {
    background: var(--done, #78ad88);
    box-shadow: 0 0 5px rgba(120, 173, 136, 0.8), 0 0 12px rgba(120, 173, 136, 0.28);
    animation: claude-status-breathe 1.8s ease-in-out infinite;
  }

  .claude-session-dot[data-status="to_review"],
  .claude-session-dot[data-status="waiting"] {
    background: var(--review, #d2a65f);
    box-shadow: 0 0 5px rgba(210, 166, 95, 0.78), 0 0 12px rgba(210, 166, 95, 0.25);
  }

  .claude-session-dot[data-status="done"] {
    background: var(--accent, #9698c0);
    box-shadow: 0 0 5px rgba(150, 152, 192, 0.72), 0 0 11px rgba(150, 152, 192, 0.22);
  }

  .claude-session-dot[data-status="failed"] {
    background: var(--danger, #d47b7b);
    box-shadow: 0 0 5px rgba(212, 123, 123, 0.76), 0 0 12px rgba(212, 123, 123, 0.22);
  }

  .claude-session-dot[data-status="recent"] {
    background: #8c8f99;
    box-shadow: 0 0 5px rgba(140, 143, 153, 0.55);
  }

  .claude-session-project,
  .claude-session-cwd {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .claude-session-project {
    min-width: 0;
    font-size: 11px;
    font-weight: 650;
    letter-spacing: -0.015em;
  }

  .claude-agent-badge {
    flex: 0 0 auto;
    max-width: 90px;
    overflow: hidden;
    padding: 2px 6px;
    border: 1px solid rgba(208, 177, 126, 0.2);
    border-radius: 999px;
    color: #d0b17e;
    font-size: 7px;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .claude-session-activity {
    overflow: hidden;
    color: var(--text-soft, #b6b7bd);
    font-size: 9px;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: linear-gradient(90deg, #777a82 0%, #f0f0f3 48%, #777a82 100%);
    background-size: 210% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    animation: claude-activity-shimmer 2.1s linear infinite;
  }

  @keyframes claude-status-breathe {
    50% { opacity: 0.72; transform: scale(0.84); }
  }

  @keyframes claude-activity-shimmer {
    to { background-position: -210% 0; }
  }

  .claude-session-cwd {
    color: var(--text-muted, #85878f);
    font-size: 9px;
    direction: rtl;
    text-align: left;
  }

  .claude-session-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text-muted, #85878f);
    font-size: 8px;
    font-variant-numeric: tabular-nums;
  }

  .claude-session-meta > span + span,
  .claude-session-meta time {
    padding-left: 6px;
    border-left: 1px solid var(--line, rgba(255, 255, 255, 0.095));
  }

  .claude-session-state[data-status="working"] {
    color: var(--done, #78ad88);
  }

  .claude-session-state[data-status="to_review"],
  .claude-session-state[data-status="waiting"] {
    color: var(--review, #d2a65f);
  }

  .claude-session-state[data-status="done"] {
    color: var(--accent, #9698c0);
  }

  .claude-session-state[data-status="failed"] {
    color: var(--danger, #d47b7b);
  }

  .claude-session-open {
    width: 13px;
    height: 13px;
    color: var(--text-muted, #85878f);
    transition: color 140ms ease, transform 140ms ease;
  }

  .claude-session-open-button:hover:not(:disabled) .claude-session-open {
    color: var(--text, #efeff2);
    transform: translate(1px, -1px);
  }

  @media (prefers-reduced-motion: reduce) {
    .claude-session-row,
    .claude-session-open-button,
    .claude-session-read,
    .claude-session-open,
    .claude-session-dot,
    .claude-session-activity {
      animation: none;
      transition-duration: 0.01ms;
    }
  }
`;

function timestamp(value: Date | number): number {
  return value instanceof Date ? value.getTime() : value;
}

function formatLastActivity(updatedAt: string, now: Date | number): string {
  const updated = Date.parse(updatedAt);
  const anchor = timestamp(now);

  if (!Number.isFinite(updated) || !Number.isFinite(anchor)) {
    return "Activity time unavailable";
  }

  const elapsed = anchor - updated;
  if (elapsed < -60_000) return "Activity time is ahead of this clock";

  const minutes = Math.floor(elapsed / 60_000);

  if (minutes < 1) return "Last activity just now";
  if (minutes < 60) return `Last activity ${minutes}m ago`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Last activity ${hours}h ago`;

  const days = Math.floor(hours / 24);
  return `Last activity ${days}d ago`;
}

export function ClaudeSessionList({
  sessions,
  onOpenSession,
  onMarkRead,
  selectionMode = false,
  selectedSessionIds = new Set<string>(),
  onToggleSession,
  openingSessionId = null,
  now = Date.now(),
}: ClaudeSessionListProps) {
  return (
    <>
      <style>{CLAUDE_SESSION_LIST_STYLES}</style>
      <ul className="claude-session-list" aria-label="Claude Code sessions">
        {sessions.map((session) => {
          const isOpening = openingSessionId === session.id;
          const activity = formatLastActivity(session.updatedAt, now);
          const status = resolveClaudeLifecycleStatus(session);
          const statusLabel = {
            working: "In progress",
            to_review: "To review",
            done: "Done",
            failed: "Failed",
            waiting: "Waiting",
          }[status];
          const isSelected = selectedSessionIds.has(session.id);
          const reviewable = !selectionMode && status === "to_review" && onMarkRead !== undefined;
          const rowLabel = selectionMode
            ? `${isSelected ? "Deselect" : "Select"} ${session.name ?? session.project}`
            : session.resumeAvailable
              ? `${isOpening ? "Opening" : "Open"} ${session.project} in Claude Code`
              : `${session.project} cannot be resumed because the Claude CLI is unavailable`;

          return (
            <li key={session.id}>
              <div
                className="claude-session-row"
                data-reviewable={reviewable}
                data-selection-mode={selectionMode}
                data-selected={isSelected}
              >
                <button
                  className="claude-session-open-button"
                  type="button"
                  disabled={!selectionMode && (isOpening || !session.resumeAvailable)}
                  aria-label={rowLabel}
                  aria-pressed={selectionMode ? isSelected : undefined}
                  onClick={() => {
                    if (selectionMode) {
                      onToggleSession?.(session);
                      return;
                    }
                    void onOpenSession(session);
                  }}
                >
                  <span className="claude-session-folder" data-selected={isSelected} aria-hidden="true">
                    {selectionMode
                      ? isSelected ? <CheckSquare weight="fill" /> : <Square weight="regular" />
                      : <AgentLogo provider="claude_code" />}
                  </span>

                  <span className="claude-session-copy">
                    <span className="claude-session-title-line">
                      <span className="claude-session-dot" data-status={status} aria-hidden="true" />
                      <strong className="claude-session-project">{session.name ?? session.project}</strong>
                      {session.agentName && (
                        <span className="claude-agent-badge" title={session.agentName}>
                          {session.isSubagent ? "Subagent · " : "Agent · "}{session.agentName}
                        </span>
                      )}
                    </span>
                    {(session.activity || session.thinking) && (
                      <span className="claude-session-activity">
                        {session.thinking ? "Thinking" : status === "working" ? "Working" : "Last action"}
                        {session.activity ? ` · ${session.activity}` : ""}
                      </span>
                    )}
                    <span className="claude-session-cwd" title={session.cwd}>{session.cwd}</span>
                    <span className="claude-session-meta">
                      <span>{session.source}</span>
                      <span className="claude-session-state" data-status={status}>
                        {statusLabel}
                      </span>
                      <time dateTime={session.updatedAt}>{activity}</time>
                    </span>
                  </span>

                  {!selectionMode && (
                    <ArrowUpRight className="claude-session-open" weight="bold" aria-hidden="true" />
                  )}
                </button>
                {reviewable && (
                  <button
                    className="claude-session-read"
                    type="button"
                    aria-label={`Mark ${session.name ?? session.project} as read`}
                    onClick={() => onMarkRead(session)}
                  >
                    <Check weight="bold" aria-hidden="true" />
                    Read
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </>
  );
}
