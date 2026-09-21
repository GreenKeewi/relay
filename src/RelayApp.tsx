import {
  Archive,
  ChartDonut,
  ChatCircle,
  CheckSquare,
  Gear,
  Minus,
  Trash,
  X,
} from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  discoverClaudeSessions,
  enableClaudeLiveUsage,
  openClaudeInstallGuide,
  readClaudeUsage,
  reauthenticateClaude,
  resumeClaudeSession,
  type ClaudeDiscoveryResult,
  type ClaudeUsage,
} from "./claude";
import { ClaudeOnboarding, type ClaudeDetection } from "./components/ClaudeOnboarding";
import { AgentLogo } from "./components/AgentLogo";
import {
  ClaudeSessionList,
  type ClaudeLocalSession,
} from "./components/ClaudeSessionList";
import { DockView } from "./components/DockView";
import { FilterRail } from "./components/FilterRail";
import {
  SettingsView,
  UsageView,
  type RefreshIntervalMs,
} from "./components/SecondaryViews";
import { invokeWindowCommand } from "./components/tauriBridge";
import {
  countClaudeStatusFilters,
  matchesClaudeStatusFilter,
  resolveClaudeLifecycleStatus,
  type ClaudeStatusFilter,
} from "./model/claudeStatus";
import "./styles.css";

type Tab = "Chats" | "Usage" | "Settings";
type ConnectedAgentFilter = ClaudeLocalSession["source"];

const tabs = [
  { label: "Chats", icon: ChatCircle },
  { label: "Usage", icon: ChartDonut },
  { label: "Settings", icon: Gear },
] as const;

const statusFilters = ["All", "In progress", "To review", "Done", "Archived"] as const;
const statusFilterTones: Record<ClaudeStatusFilter, string> = {
  All: "all",
  "In progress": "working",
  "To review": "to_review",
  Done: "done",
  Archived: "archived",
};
const ONBOARDING_KEY = "relay:claude-onboarded";
const REVIEW_BASELINE_KEY = "relay:review-baseline-v1";
const REVIEWED_SESSION_KEYS = "relay:reviewed-session-keys";
const STATUS_OVERRIDE_KEY = "relay:session-status-overrides-v1";
const ARCHIVED_SESSION_KEYS = "relay:archived-session-keys-v1";
const DELETED_SESSION_KEYS = "relay:deleted-session-keys-v1";
const REFRESH_INTERVAL_KEY = "relay:refresh-interval-ms";
const DEFAULT_REFRESH_INTERVAL_MS: RefreshIntervalMs = 30_000;
const REFRESH_INTERVALS = new Set<RefreshIntervalMs>([5_000, 10_000, 30_000, 60_000]);

function readRefreshInterval(): RefreshIntervalMs {
  const stored = Number(window.localStorage.getItem(REFRESH_INTERVAL_KEY));
  return REFRESH_INTERVALS.has(stored as RefreshIntervalMs)
    ? stored as RefreshIntervalMs
    : DEFAULT_REFRESH_INTERVAL_MS;
}

function readReviewedSessionKeys() {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(REVIEWED_SESSION_KEYS) ?? "[]");
    return new Set<string>(Array.isArray(parsed) ? parsed.filter((item) => typeof item === "string") : []);
  } catch {
    return new Set<string>();
  }
}

function readStringSet(key: string) {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(key) ?? "[]");
    return new Set<string>(Array.isArray(parsed) ? parsed.filter((item) => typeof item === "string") : []);
  } catch {
    return new Set<string>();
  }
}

type ManagedStatus = "working" | "to_review" | "done";

function readStatusOverrides() {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(STATUS_OVERRIDE_KEY) ?? "{}");
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).filter((entry): entry is [string, ManagedStatus] => (
        typeof entry[0] === "string"
        && (entry[1] === "working" || entry[1] === "to_review" || entry[1] === "done")
      )),
    );
  } catch {
    return {};
  }
}

function reviewVersionKey(session: ClaudeLocalSession) {
  return `${session.id}:${Date.parse(session.updatedAt)}`;
}

function useCurrentTime() {
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  return now;
}

function toLocalSession(session: ClaudeDiscoveryResult["sessions"][number]): ClaudeLocalSession {
  return {
    id: session.id,
    project: session.project,
    name: session.sessionName ?? session.name ?? undefined,
    agentName: session.agentName ?? undefined,
    isSubagent: session.isSubagent,
    activity: session.safeActivity ?? session.activity ?? undefined,
    thinking: session.thinking,
    status: session.status,
    cwd: session.cwd,
    updatedAt: new Date(session.lastActivityMs).toISOString(),
    state: session.state,
    source: "Claude Code",
    resumeAvailable: session.resumeAvailable,
  };
}

export default function RelayApp() {
  const isDockView = new URLSearchParams(window.location.search).get("view") === "dock";
  const now = useCurrentTime();
  const [activeTab, setActiveTab] = useState<Tab>("Chats");
  const [agentFilter, setAgentFilter] = useState<ConnectedAgentFilter>("Claude Code");
  const [statusFilter, setStatusFilter] = useState<ClaudeStatusFilter>("All");
  const [discovery, setDiscovery] = useState<ClaudeDiscoveryResult | null>(null);
  const [usage, setUsage] = useState<ClaudeUsage | null>(null);
  const [isUsageLoading, setIsUsageLoading] = useState(true);
  const [usageLoadError, setUsageLoadError] = useState(false);
  const [isEnablingLiveUsage, setIsEnablingLiveUsage] = useState(false);
  const [isDetecting, setIsDetecting] = useState(false);
  const [detectionError, setDetectionError] = useState<string | null>(null);
  const [openingSessionId, setOpeningSessionId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [onboardingStage, setOnboardingStage] = useState<"privacy" | "detection">("privacy");
  const [canExitConnectionFlow, setCanExitConnectionFlow] = useState(false);
  const [refreshIntervalMs, setRefreshIntervalMs] = useState<RefreshIntervalMs>(readRefreshInterval);
  const [reviewedSessionKeys, setReviewedSessionKeys] = useState(readReviewedSessionKeys);
  const [statusOverrides, setStatusOverrides] = useState<Record<string, ManagedStatus>>(readStatusOverrides);
  const [archivedSessionKeys, setArchivedSessionKeys] = useState(
    () => readStringSet(ARCHIVED_SESSION_KEYS),
  );
  const [deletedSessionKeys, setDeletedSessionKeys] = useState(
    () => readStringSet(DELETED_SESSION_KEYS),
  );
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedSessionIds, setSelectedSessionIds] = useState<Set<string>>(() => new Set());
  const [reviewBaselineReady, setReviewBaselineReady] = useState(
    () => window.localStorage.getItem(REVIEW_BASELINE_KEY) === "true",
  );
  const [isOnboarded, setIsOnboarded] = useState(
    () => window.localStorage.getItem(ONBOARDING_KEY) === "true",
  );

  const scanClaude = useCallback(async (finishOnSuccess = false) => {
    setIsDetecting(true);
    setDetectionError(null);
    try {
      const result = await discoverClaudeSessions();
      setDiscovery(result);
      if (finishOnSuccess && result.detection.installed && result.sessions.length > 0) {
        window.localStorage.setItem(ONBOARDING_KEY, "true");
        setCanExitConnectionFlow(false);
        setIsOnboarded(true);
      }
      return result;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setDetectionError(message);
      return null;
    } finally {
      setIsDetecting(false);
    }
  }, []);

  const refreshUsage = useCallback(async () => {
    setIsUsageLoading(true);
    setUsageLoadError(false);
    try {
      setUsage(await readClaudeUsage());
    } catch {
      setUsage(null);
      setUsageLoadError(true);
    } finally {
      setIsUsageLoading(false);
    }
  }, []);

  const enableLiveUsage = useCallback(async () => {
    setIsEnablingLiveUsage(true);
    setUsageLoadError(false);
    setActionError(null);
    try {
      await enableClaudeLiveUsage();
      await refreshUsage();
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    } finally {
      setIsEnablingLiveUsage(false);
    }
  }, [refreshUsage]);

  const reauthenticateClaudeUsage = useCallback(async () => {
    setActionError(null);
    try {
      await reauthenticateClaude();
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => {
    if (!isOnboarded) return;
    void scanClaude();
    void refreshUsage();
    const timer = window.setInterval(() => {
      void scanClaude();
      void refreshUsage();
    }, refreshIntervalMs);
    return () => window.clearInterval(timer);
  }, [isOnboarded, refreshIntervalMs, refreshUsage, scanClaude]);

  const sessions = useMemo(() => (
    (discovery?.sessions ?? []).map(toLocalSession).map((session) => {
      const versionKey = reviewVersionKey(session);
      const override = statusOverrides[versionKey];
      const status = resolveClaudeLifecycleStatus(session);
      const isReviewed = reviewedSessionKeys.has(versionKey);
      const resolvedSession = override
        ? { ...session, status: override }
        : status === "to_review" && (!reviewBaselineReady || isReviewed)
          ? { ...session, status: "done" as const }
          : session;
      return {
        ...resolvedSession,
        archived: archivedSessionKeys.has(versionKey),
      };
    }).filter((session) => !deletedSessionKeys.has(reviewVersionKey(session)))
  ), [
    archivedSessionKeys,
    deletedSessionKeys,
    discovery,
    reviewBaselineReady,
    reviewedSessionKeys,
    statusOverrides,
  ]);

  useEffect(() => {
    if (reviewBaselineReady || discovery === null) return;

    const baselineKeys = discovery.sessions
      .map(toLocalSession)
      .filter((session) => resolveClaudeLifecycleStatus(session) === "to_review")
      .map(reviewVersionKey);
    const next = new Set(reviewedSessionKeys);
    baselineKeys.forEach((key) => next.add(key));
    const bounded = [...next].slice(-500);

    window.localStorage.setItem(REVIEWED_SESSION_KEYS, JSON.stringify(bounded));
    window.localStorage.setItem(REVIEW_BASELINE_KEY, "true");
    setReviewedSessionKeys(new Set(bounded));
    setReviewBaselineReady(true);
  }, [discovery, reviewBaselineReady, reviewedSessionKeys]);
  const agentFilteredSessions = useMemo(
    () => sessions.filter((session) => session.source === agentFilter),
    [agentFilter, sessions],
  );
  const statusCounts = useMemo(
    () => countClaudeStatusFilters(agentFilteredSessions),
    [agentFilteredSessions],
  );
  const visibleSessions = useMemo(
    () => agentFilteredSessions.filter((session) => matchesClaudeStatusFilter(session, statusFilter)),
    [agentFilteredSessions, statusFilter],
  );
  const selectedSessions = useMemo(
    () => sessions.filter((session) => selectedSessionIds.has(session.id)),
    [selectedSessionIds, sessions],
  );

  useEffect(() => {
    if (!selectionMode) return;
    const visibleIds = new Set(visibleSessions.map((session) => session.id));
    setSelectedSessionIds((current) => {
      const next = new Set([...current].filter((id) => visibleIds.has(id)));
      return next.size === current.size ? current : next;
    });
  }, [selectionMode, visibleSessions]);
  const recentCount = sessions.filter(
    (session) => resolveClaudeLifecycleStatus(session) === "working",
  ).length;
  const idleCount = sessions.filter((session) => {
    const status = resolveClaudeLifecycleStatus(session);
    return status === "to_review" || status === "waiting" || status === "failed";
  }).length;

  const detection: ClaudeDetection | null = discovery
    ? {
        installed: discovery.detection.installed,
        configDir: discovery.detection.projectsDirectory,
        sessionCount: discovery.detection.sessionsDiscovered,
        error: detectionError ?? undefined,
      }
    : detectionError
      ? { installed: false, sessionCount: 0, error: detectionError }
      : null;

  const beginOnboarding = async () => {
    setOnboardingStage("detection");
    await scanClaude(true);
  };

  const openSession = async (session: ClaudeLocalSession) => {
    setOpeningSessionId(session.id);
    setActionError(null);
    try {
      await resumeClaudeSession(session.id, session.cwd);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    } finally {
      setOpeningSessionId(null);
    }
  };

  const resetConnection = () => {
    setCanExitConnectionFlow(true);
    setIsOnboarded(false);
    setOnboardingStage("privacy");
  };

  const goBackFromConnection = () => {
    if (onboardingStage === "detection") {
      setOnboardingStage("privacy");
      return;
    }

    if (canExitConnectionFlow) {
      setCanExitConnectionFlow(false);
      setIsOnboarded(true);
    }
  };

  const updateRefreshInterval = (next: RefreshIntervalMs) => {
    window.localStorage.setItem(REFRESH_INTERVAL_KEY, String(next));
    setRefreshIntervalMs(next);
  };

  const markSessionRead = (session: ClaudeLocalSession) => {
    const next = new Set(reviewedSessionKeys);
    next.add(reviewVersionKey(session));
    const bounded = [...next].slice(-500);
    window.localStorage.setItem(REVIEWED_SESSION_KEYS, JSON.stringify(bounded));
    setReviewedSessionKeys(new Set(bounded));
  };

  const toggleSelectionMode = () => {
    setSelectionMode((current) => !current);
    setSelectedSessionIds(new Set());
  };

  const toggleSessionSelection = (session: ClaudeLocalSession) => {
    setSelectedSessionIds((current) => {
      const next = new Set(current);
      if (next.has(session.id)) next.delete(session.id);
      else next.add(session.id);
      return next;
    });
  };

  const toggleAllVisible = () => {
    setSelectedSessionIds((current) => {
      const visibleIds = visibleSessions.map((session) => session.id);
      const allSelected = visibleIds.length > 0 && visibleIds.every((id) => current.has(id));
      return allSelected ? new Set() : new Set(visibleIds);
    });
  };

  const moveSelectedSessions = (status: ManagedStatus) => {
    if (selectedSessions.length === 0) return;
    const nextOverrides = { ...statusOverrides };
    const nextArchived = new Set(archivedSessionKeys);
    selectedSessions.forEach((session) => {
      const key = reviewVersionKey(session);
      nextOverrides[key] = status;
      nextArchived.delete(key);
    });
    window.localStorage.setItem(STATUS_OVERRIDE_KEY, JSON.stringify(nextOverrides));
    window.localStorage.setItem(ARCHIVED_SESSION_KEYS, JSON.stringify([...nextArchived]));
    setStatusOverrides(nextOverrides);
    setArchivedSessionKeys(nextArchived);
    setSelectedSessionIds(new Set());
  };

  const archiveSelectedSessions = () => {
    if (selectedSessions.length === 0) return;
    const next = new Set(archivedSessionKeys);
    const restoring = statusFilter === "Archived";
    selectedSessions.forEach((session) => {
      const key = reviewVersionKey(session);
      if (restoring) next.delete(key);
      else next.add(key);
    });
    window.localStorage.setItem(ARCHIVED_SESSION_KEYS, JSON.stringify([...next]));
    setArchivedSessionKeys(next);
    setSelectedSessionIds(new Set());
  };

  const deleteSelectedSessions = () => {
    if (selectedSessions.length === 0) return;
    const confirmed = window.confirm(
      `Delete ${selectedSessions.length} selected session${selectedSessions.length === 1 ? "" : "s"} from Relay? Claude Code files will not be deleted.`,
    );
    if (!confirmed) return;
    const next = new Set(deletedSessionKeys);
    selectedSessions.forEach((session) => next.add(reviewVersionKey(session)));
    window.localStorage.setItem(DELETED_SESSION_KEYS, JSON.stringify([...next]));
    setDeletedSessionKeys(next);
    setSelectedSessionIds(new Set());
  };

  if (isDockView) {
    return <DockView recentCount={recentCount} idleCount={idleCount} />;
  }

  if (!isOnboarded) {
    return (
      <main className="relay-shell onboarding-shell">
        <ClaudeOnboarding
          stage={onboardingStage}
          detection={detection}
          sessions={sessions}
          isDetecting={isDetecting}
          openingSessionId={openingSessionId}
          onContinue={beginOnboarding}
          onBack={onboardingStage === "detection" || canExitConnectionFlow
            ? goBackFromConnection
            : undefined}
          onRetry={async () => { await scanClaude(true); }}
          onOpenSession={openSession}
          onOpenInstallGuide={openClaudeInstallGuide}
          now={now}
        />
      </main>
    );
  }

  return (
    <main className="relay-shell">
      <header className="topbar" data-tauri-drag-region>
        <span className="wordmark" aria-label="Relay" data-tauri-drag-region>Relay</span>

        <nav className="segment-control" aria-label="Relay sections">
          {tabs.map(({ label, icon: Icon }) => {
            const active = activeTab === label;
            return (
              <button
                key={label}
                type="button"
                className="segment"
                data-active={active}
                aria-current={active ? "page" : undefined}
                aria-label={label}
                title={label}
                onClick={() => setActiveTab(label)}
              >
                <Icon aria-hidden="true" weight={active ? "fill" : "regular"} />
                {active && <span>{label}</span>}
              </button>
            );
          })}
        </nav>

        <button
          className="dock-button"
          type="button"
          aria-label="Collapse Relay to dock"
          title="Collapse to dock"
          onClick={() => invokeWindowCommand("collapse_to_dock", "relay:collapse-to-dock")}
        >
          <Minus aria-hidden="true" weight="bold" />
        </button>
      </header>

      <div className="widget-content" key={activeTab}>
        {activeTab === "Chats" && (
          <section className="chats-view" aria-label="Claude Code sessions">
            <div className="filter-stack">
              {discovery?.detection.installed && (
                <>
                  <FilterRail
                    label="Connected agents"
                    options={["Claude Code"] as const}
                    value={agentFilter}
                    onChange={setAgentFilter}
                    renderLeading={() => <AgentLogo provider="claude_code" />}
                  />
                  <div className="filter-group-separator" role="separator" aria-orientation="horizontal" />
                </>
              )}
              <div className="status-filter-row">
                <FilterRail
                  label="Status filters"
                  options={statusFilters}
                  value={statusFilter}
                  onChange={setStatusFilter}
                  renderLeading={(filter) => (
                    <span
                      className="filter-status-dot"
                      data-status={statusFilterTones[filter]}
                      aria-hidden="true"
                    />
                  )}
                  renderTrailing={(filter) => (
                    <span className="filter-session-count" aria-label={`${statusCounts[filter]} sessions`}>
                      {statusCounts[filter]}
                    </span>
                  )}
                />
                <button
                  className="selection-mode-button"
                  type="button"
                  aria-label={selectionMode ? "Exit session selection" : "Select sessions"}
                  aria-pressed={selectionMode}
                  title={selectionMode ? "Exit selection" : "Select sessions"}
                  onClick={toggleSelectionMode}
                >
                  {selectionMode
                    ? <X aria-hidden="true" weight="bold" />
                    : <CheckSquare aria-hidden="true" />}
                </button>
              </div>
            </div>

            {selectionMode && (
              <div className="session-selection-tools">
                <span>{selectedSessions.length} selected</span>
                <button type="button" onClick={toggleAllVisible} disabled={visibleSessions.length === 0}>
                  {visibleSessions.length > 0
                    && visibleSessions.every((session) => selectedSessionIds.has(session.id))
                    ? "Clear"
                    : "All"}
                </button>
                <select
                  aria-label="Move selected sessions to status"
                  value=""
                  disabled={selectedSessions.length === 0}
                  onChange={(event) => {
                    if (event.target.value) moveSelectedSessions(event.target.value as ManagedStatus);
                  }}
                >
                  <option value="">Move...</option>
                  <option value="working">In progress</option>
                  <option value="to_review">To review</option>
                  <option value="done">Done</option>
                </select>
                <button
                  type="button"
                  disabled={selectedSessions.length === 0}
                  onClick={archiveSelectedSessions}
                >
                  <Archive aria-hidden="true" />
                  {statusFilter === "Archived" ? "Restore" : "Archive"}
                </button>
                <button
                  className="session-delete-action"
                  type="button"
                  disabled={selectedSessions.length === 0}
                  onClick={deleteSelectedSessions}
                >
                  <Trash aria-hidden="true" />
                  Delete
                </button>
              </div>
            )}

            <div className="claude-session-scroll">
              {isDetecting && discovery === null ? (
                <div className="empty-state" role="status">
                  <strong>Reading local Claude sessions…</strong>
                </div>
              ) : visibleSessions.length > 0 ? (
                <ClaudeSessionList
                  sessions={visibleSessions}
                  onOpenSession={openSession}
                  onMarkRead={markSessionRead}
                  selectionMode={selectionMode}
                  selectedSessionIds={selectedSessionIds}
                  onToggleSession={toggleSessionSelection}
                  openingSessionId={openingSessionId}
                  now={now}
                />
              ) : (
                <div className="empty-state" role="status">
                  <strong>No matching Claude sessions</strong>
                  <p>Start Claude Code in a project, then refresh from Settings.</p>
                </div>
              )}
            </div>
            {actionError && <p className="action-error" role="alert">{actionError}</p>}
          </section>
        )}
        {activeTab === "Usage" && (
          <UsageView
            usage={usage}
            isLoading={isUsageLoading}
            loadError={usageLoadError}
            onEnableLiveUsage={enableLiveUsage}
            isEnablingLiveUsage={isEnablingLiveUsage}
            onReauthenticateClaude={() => { void reauthenticateClaudeUsage(); }}
          />
        )}
        {activeTab === "Settings" && (
          <SettingsView
            sessionCount={sessions.length}
            cliAvailable={discovery?.detection.cliAvailable ?? false}
            configDir={discovery?.detection.projectsDirectory}
            isRefreshing={isDetecting}
            refreshIntervalMs={refreshIntervalMs}
            logicalProcessorCount={navigator.hardwareConcurrency || 1}
            deviceMemoryGb={(navigator as Navigator & { deviceMemory?: number }).deviceMemory}
            onRefresh={async () => { await scanClaude(); }}
            onRefreshIntervalChange={updateRefreshInterval}
            onDisconnect={resetConnection}
          />
        )}
      </div>
    </main>
  );
}
