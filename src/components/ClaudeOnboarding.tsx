import {
  ArrowLeft,
  ArrowRight,
  CheckCircle,
  HardDrive,
  LockKey,
  MagnifyingGlass,
  SealCheck,
  WarningCircle,
} from "@phosphor-icons/react";
import {
  ClaudeSessionList,
  type ClaudeLocalSession,
} from "./ClaudeSessionList";
import { AgentLogo } from "./AgentLogo";

export interface ClaudeDetection {
  installed: boolean;
  configDir?: string;
  sessionCount: number;
  error?: string;
}

export type ClaudeOnboardingStage = "privacy" | "detection";

export interface ClaudeOnboardingProps {
  stage: ClaudeOnboardingStage;
  detection: ClaudeDetection | null;
  sessions: readonly ClaudeLocalSession[];
  isDetecting: boolean;
  openingSessionId?: string | null;
  onContinue: () => void | Promise<void>;
  onBack?: () => void;
  onRetry: () => void | Promise<void>;
  onOpenSession: (session: ClaudeLocalSession) => void | Promise<void>;
  onOpenInstallGuide?: () => void | Promise<void>;
  now?: Date | number;
}

const CLAUDE_ONBOARDING_STYLES = `
  .claude-onboarding {
    display: flex;
    min-height: 100%;
    flex-direction: column;
    padding: 16px 14px 14px;
    color: var(--text, #efeff2);
  }

  .claude-onboarding-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 18px;
  }

  .claude-onboarding-heading {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 7px;
  }

  .claude-onboarding-back {
    display: grid;
    width: 26px;
    height: 26px;
    flex: 0 0 auto;
    place-items: center;
    padding: 0;
    border: 1px solid var(--line, rgba(255, 255, 255, 0.095));
    border-radius: 9px;
    color: var(--text-soft, #b6b7bd);
    background: var(--surface-raised, rgba(255, 255, 255, 0.055));
    cursor: pointer;
    transition: color 140ms ease, border-color 140ms ease, background-color 140ms ease, transform 100ms ease;
  }

  .claude-onboarding-back:hover {
    border-color: var(--line-strong, rgba(255, 255, 255, 0.16));
    color: var(--text, #efeff2);
    background: var(--surface-hover, rgba(255, 255, 255, 0.082));
  }

  .claude-onboarding-back:active {
    transform: scale(0.94);
  }

  .claude-onboarding-back svg {
    width: 13px;
    height: 13px;
  }

  .claude-onboarding-brand {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-soft, #b6b7bd);
    font-size: 10px;
    font-weight: 650;
  }

  .claude-onboarding-brand svg,
  .claude-onboarding-brand .agent-logo {
    width: 15px;
    height: 15px;
    color: var(--accent);
  }

  .claude-local-seal {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 23px;
    padding: 3px 8px;
    border: 1px solid rgba(120, 173, 136, 0.22);
    border-radius: 999px;
    color: var(--done, #78ad88);
    background: rgba(120, 173, 136, 0.07);
    font-size: 8px;
    font-weight: 650;
  }

  .claude-local-seal svg {
    width: 11px;
    height: 11px;
  }

  .claude-onboarding-copy {
    max-width: 320px;
  }

  .claude-onboarding h1 {
    margin: 0;
    color: var(--text, #efeff2);
    font-family: "Instrument Serif", Georgia, "Times New Roman", serif;
    font-size: 29px;
    font-weight: 400;
    letter-spacing: -0.035em;
    line-height: 0.98;
  }

  .claude-onboarding-lede {
    margin: 10px 0 0;
    color: var(--text-soft, #b6b7bd);
    font-size: 11px;
    line-height: 1.45;
  }

  .claude-privacy-ledger {
    display: grid;
    gap: 1px;
    margin: 17px 0 16px;
    overflow: hidden;
    border: 1px solid var(--line, rgba(255, 255, 255, 0.095));
    border-radius: var(--radius-control, 11px);
    background: var(--line, rgba(255, 255, 255, 0.095));
  }

  .claude-privacy-row {
    display: grid;
    grid-template-columns: 25px minmax(0, 1fr);
    align-items: center;
    gap: 9px;
    min-height: 51px;
    padding: 8px 10px;
    background: rgba(255, 255, 255, 0.026);
  }

  .claude-privacy-row svg {
    width: 16px;
    height: 16px;
    color: var(--accent);
  }

  .claude-privacy-row span {
    display: grid;
    gap: 2px;
  }

  .claude-privacy-row strong {
    font-size: 10px;
    font-weight: 650;
  }

  .claude-privacy-row small {
    color: var(--text-muted, #85878f);
    font-size: 9px;
    line-height: 1.35;
  }

  .claude-primary-action,
  .claude-secondary-action {
    display: inline-flex;
    min-height: 34px;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 7px 11px;
    border-radius: var(--radius-control, 11px);
    font-size: 10px;
    font-weight: 650;
    cursor: pointer;
    transition: background-color 140ms ease, border-color 140ms ease, transform 100ms ease;
  }

  .claude-primary-action {
    width: 100%;
    border: 1px solid color-mix(in srgb, var(--accent) 48%, transparent);
    color: #15161a;
    background: var(--accent);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16);
  }

  .claude-secondary-action {
    border: 1px solid var(--line-strong, rgba(255, 255, 255, 0.16));
    color: var(--text, #efeff2);
    background: var(--surface-raised, rgba(255, 255, 255, 0.055));
  }

  .claude-primary-action:hover,
  .claude-secondary-action:hover {
    filter: brightness(1.07);
  }

  .claude-primary-action:active,
  .claude-secondary-action:active {
    transform: scale(0.98);
  }

  .claude-primary-action svg,
  .claude-secondary-action svg {
    width: 13px;
    height: 13px;
  }

  .claude-detection-header {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 11px;
  }

  .claude-detection-header h1 {
    font-family: "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
    font-size: 17px;
    font-weight: 650;
    line-height: 1.1;
  }

  .claude-detection-header p {
    margin: 4px 0 0;
    color: var(--text-muted, #85878f);
    font-size: 9px;
  }

  .claude-session-count {
    color: var(--accent);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .claude-result-state {
    display: grid;
    min-height: 190px;
    place-items: center;
    align-content: center;
    padding: 18px 16px;
    border: 1px solid var(--line, rgba(255, 255, 255, 0.095));
    border-radius: var(--radius-control, 11px);
    color: var(--text-soft, #b6b7bd);
    text-align: center;
    background: rgba(255, 255, 255, 0.025);
  }

  .claude-result-icon {
    display: grid;
    width: 34px;
    height: 34px;
    margin-bottom: 10px;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--accent) 28%, transparent);
    border-radius: 11px;
    color: var(--accent);
    background: var(--accent-surface);
  }

  .claude-result-icon svg {
    width: 17px;
    height: 17px;
  }

  .claude-result-state strong {
    color: var(--text, #efeff2);
    font-size: 11px;
  }

  .claude-result-state p {
    max-width: 260px;
    margin: 5px 0 13px;
    color: var(--text-muted, #85878f);
    font-size: 9px;
    line-height: 1.4;
  }

  .claude-result-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
  }

  .claude-error-detail {
    overflow-wrap: anywhere;
  }

  .claude-scan-icon {
    animation: claude-scan 1.2s ease-in-out infinite alternate;
  }

  @keyframes claude-scan {
    to { transform: translateX(3px); }
  }

  @media (prefers-reduced-motion: reduce) {
    .claude-primary-action,
    .claude-secondary-action,
    .claude-onboarding-back,
    .claude-scan-icon {
      animation: none;
      transition-duration: 0.01ms;
    }
  }
`;

type ResultStateProps = {
  detection: ClaudeDetection | null;
  isDetecting: boolean;
  onRetry: () => void | Promise<void>;
  onOpenInstallGuide?: () => void | Promise<void>;
};

function DetectionState({
  detection,
  isDetecting,
  onRetry,
  onOpenInstallGuide,
}: ResultStateProps) {
  if (isDetecting || detection === null) {
    return (
      <div className="claude-result-state" role="status" aria-live="polite">
        <span className="claude-result-icon claude-scan-icon" aria-hidden="true">
          <MagnifyingGlass weight="duotone" />
        </span>
        <strong>Looking for Claude Code</strong>
        <p>Checking the local Claude session directory on this PC.</p>
      </div>
    );
  }

  if (detection.error) {
    return (
      <div className="claude-result-state" role="alert">
        <span className="claude-result-icon" aria-hidden="true">
          <WarningCircle weight="duotone" />
        </span>
        <strong>Couldn’t read Claude sessions</strong>
        <p className="claude-error-detail">{detection.error}</p>
        <button className="claude-secondary-action" type="button" onClick={() => void onRetry()}>
          Try again
        </button>
      </div>
    );
  }

  if (!detection.installed) {
    return (
      <div className="claude-result-state" role="status">
        <span className="claude-result-icon" aria-hidden="true">
          <WarningCircle weight="duotone" />
        </span>
        <strong>Claude Code isn’t installed</strong>
        <p>Install Claude Code, start one local session, then check again.</p>
        <div className="claude-result-actions">
          {onOpenInstallGuide && (
            <button
              className="claude-secondary-action"
              type="button"
              onClick={() => void onOpenInstallGuide()}
            >
              Installation guide
            </button>
          )}
          <button className="claude-secondary-action" type="button" onClick={() => void onRetry()}>
            Check again
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="claude-result-state" role="status">
      <span className="claude-result-icon" aria-hidden="true">
        <CheckCircle weight="duotone" />
      </span>
      <strong>No local sessions yet</strong>
      <p>Start Claude Code in a project. Relay will find its session metadata here.</p>
      <button className="claude-secondary-action" type="button" onClick={() => void onRetry()}>
        Check again
      </button>
    </div>
  );
}

export function ClaudeOnboarding({
  stage,
  detection,
  sessions,
  isDetecting,
  openingSessionId = null,
  onContinue,
  onBack,
  onRetry,
  onOpenSession,
  onOpenInstallGuide,
  now,
}: ClaudeOnboardingProps) {
  const hasSessions =
    stage === "detection"
    && !isDetecting
    && detection?.installed === true
    && !detection.error
    && sessions.length > 0;

  return (
    <section className="claude-onboarding" aria-labelledby="claude-onboarding-title">
      <style>{CLAUDE_ONBOARDING_STYLES}</style>
      <header className="claude-onboarding-header" data-tauri-drag-region>
        <div className="claude-onboarding-heading" data-tauri-drag-region>
          {onBack && (
            <button
              className="claude-onboarding-back"
              type="button"
              aria-label="Back"
              title="Back"
              onClick={onBack}
            >
              <ArrowLeft weight="bold" aria-hidden="true" />
            </button>
          )}
          <span className="claude-onboarding-brand" data-tauri-drag-region>
            <AgentLogo provider="claude_code" />
            Relay + Claude Code
          </span>
        </div>
        <span className="claude-local-seal" data-tauri-drag-region>
          <SealCheck weight="fill" aria-hidden="true" />
          Stays on this PC
        </span>
      </header>

      {stage === "privacy" ? (
        <>
          <div className="claude-onboarding-copy">
            <h1 id="claude-onboarding-title">Pick up where Claude left off.</h1>
            <p className="claude-onboarding-lede">
              Relay can find your real Claude Code sessions and open them from this widget.
            </p>
          </div>

          <div className="claude-privacy-ledger">
            <div className="claude-privacy-row">
              <HardDrive weight="duotone" aria-hidden="true" />
              <span>
                <strong>Reads local session metadata</strong>
                <small>Project, session name, agent label, safe activity, and observable state.</small>
              </span>
            </div>
            <div className="claude-privacy-row">
              <LockKey weight="duotone" aria-hidden="true" />
              <span>
                <strong>Nothing leaves this PC</strong>
                <small>No prompts, responses, tool inputs, or hidden thinking are shown or uploaded.</small>
              </span>
            </div>
          </div>

          <button className="claude-primary-action" type="button" onClick={() => void onContinue()}>
            Find my Claude sessions
            <ArrowRight weight="bold" aria-hidden="true" />
          </button>
        </>
      ) : (
        <>
          {hasSessions ? (
            <>
              <div className="claude-detection-header">
                <div>
                  <h1 id="claude-onboarding-title">Claude Code connected</h1>
                  <p>Choose a real local session to open or resume it.</p>
                </div>
                <span className="claude-session-count">
                  {sessions.length} {sessions.length === 1 ? "session" : "sessions"}
                </span>
              </div>
              <ClaudeSessionList
                sessions={sessions}
                onOpenSession={onOpenSession}
                openingSessionId={openingSessionId}
                now={now}
              />
            </>
          ) : (
            <>
              <h1 id="claude-onboarding-title" style={{ position: "absolute", width: 1, height: 1, overflow: "hidden", clip: "rect(0 0 0 0)" }}>
                Claude Code detection
              </h1>
              <DetectionState
                detection={detection}
                isDetecting={isDetecting}
                onRetry={onRetry}
                onOpenInstallGuide={onOpenInstallGuide}
              />
            </>
          )}
        </>
      )}
    </section>
  );
}
