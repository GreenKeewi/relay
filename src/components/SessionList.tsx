import {
  ArrowUpRight,
  CaretLeft,
  CaretRight,
  CheckCircle,
  Clock,
  WarningCircle,
} from "@phosphor-icons/react";
import type { RelaySession } from "../model/session";
import { providerIdFromLabel } from "../model/agentProvider";
import { AgentLogo } from "./AgentLogo";

export type SessionStatus = "In progress" | "To review" | "Done" | "Failed" | "Paused";

export type Session = {
  id: string;
  provider: "Codex" | "ChatGPT Work" | "Claude Code" | "Zed";
  project: string;
  title: string;
  summary: string;
  status: SessionStatus;
  updated: string;
  source: RelaySession;
};

type SessionListProps = {
  sessions: Session[];
  page: number;
  pageSize: number;
  onPageChange: (page: number) => void;
  onOpenSession: (session: RelaySession) => void;
};

const statusIcon = {
  "In progress": Clock,
  "To review": WarningCircle,
  Done: CheckCircle,
  Failed: WarningCircle,
  Paused: Clock,
};

export function SessionList({
  sessions,
  page,
  pageSize,
  onPageChange,
  onOpenSession,
}: SessionListProps) {
  const pageCount = Math.max(1, Math.ceil(sessions.length / pageSize));
  const safePage = Math.min(page, pageCount - 1);
  const start = safePage * pageSize;
  const visibleSessions = sessions.slice(start, start + pageSize);

  if (sessions.length === 0) {
    return (
      <div className="empty-state" role="status">
        <span className="empty-icon" aria-hidden="true">
          <Clock weight="duotone" />
        </span>
        <strong>No sessions here</strong>
        <p>Try another provider or status filter.</p>
      </div>
    );
  }

  return (
    <div className="session-region">
      <div className="session-list" aria-live="polite">
        {visibleSessions.map((session) => {
          const StatusIcon = statusIcon[session.status];
          const providerId = providerIdFromLabel(session.provider);

          return (
            <button
              className="session-row"
              type="button"
              key={session.id}
              onClick={() => onOpenSession(session.source)}
              aria-label={`Open ${session.title} in ${session.provider}`}
            >
              <span className="provider-mark" aria-hidden="true">
                {providerId && <AgentLogo provider={providerId} />}
              </span>

              <span className="session-copy">
                <span className="session-context">
                  <span>{session.provider}</span>
                  <span aria-hidden="true">/</span>
                  <span>{session.project}</span>
                </span>
                <strong>{session.title}</strong>
                <span className="session-summary">{session.summary}</span>
                <span className={`session-status status-${session.status.toLowerCase().replace(" ", "-")}`}>
                  <StatusIcon aria-hidden="true" weight="fill" />
                  <span>{session.status}</span>
                  <span className="status-separator" aria-hidden="true">/</span>
                  <span>{session.updated}</span>
                </span>
              </span>

              <ArrowUpRight className="open-session-icon" aria-hidden="true" weight="bold" />
            </button>
          );
        })}
      </div>

      <nav className="pagination" aria-label="Session pages">
        <span>
          {start + 1}-{Math.min(start + pageSize, sessions.length)} of {sessions.length}
        </span>
        <div className="pagination-actions">
          <button
            type="button"
            aria-label="Previous session page"
            title="Previous page"
            disabled={safePage === 0}
            onClick={() => onPageChange(Math.max(0, safePage - 1))}
          >
            <CaretLeft aria-hidden="true" weight="bold" />
          </button>
          <button
            type="button"
            aria-label="Next session page"
            title="Next page"
            disabled={safePage >= pageCount - 1}
            onClick={() => onPageChange(Math.min(pageCount - 1, safePage + 1))}
          >
            <CaretRight aria-hidden="true" weight="bold" />
          </button>
        </div>
      </nav>
    </div>
  );
}
