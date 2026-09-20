import {
  BellSimple,
  CaretDown,
  Check,
  CheckCircle,
  Gauge,
  ShieldCheck,
  ToggleRight,
} from "@phosphor-icons/react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ClaudeUsage, ClaudeUsageReason } from "../claude";
import { AgentLogo } from "./AgentLogo";

function usePersistentBoolean(key: string, initialValue: boolean) {
  const [value, setValue] = useState(() => {
    const stored = window.localStorage.getItem(key);
    return stored === null ? initialValue : stored === "true";
  });

  const update = (next: boolean) => {
    window.localStorage.setItem(key, String(next));
    setValue(next);
  };

  return [value, update] as const;
}

function formatReset(resetAtMs?: number | null) {
  if (!resetAtMs) return "Not available";
  const remainingMinutes = Math.max(0, Math.ceil((resetAtMs - Date.now()) / 60_000));
  if (remainingMinutes === 0) return "Now";
  const hours = Math.floor(remainingMinutes / 60);
  const minutes = remainingMinutes % 60;
  if (hours === 0) return `${minutes}m`;
  if (minutes === 0) return `${hours}hr`;
  return `${hours}hr ${minutes}m`;
}

function formatAge(ageSeconds?: number | null) {
  if (ageSeconds == null || ageSeconds < 60) return "just now";
  const minutes = Math.floor(ageSeconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}

const reasonMessages: Partial<Record<ClaudeUsageReason, string>> = {
  live_snapshot_not_found: "No verified snapshot yet. Start Claude Code to refresh.",
  cache_not_found: "No verified snapshot yet. Start Claude Code to refresh.",
  no_usage_data: "Claude has not provided limits. No estimates shown.",
  timeout: "Usage check timed out. Retrying automatically.",
  rate_limited: "Usage check is rate limited. Retrying automatically.",
  no_credentials: "Sign in to Claude Code to read usage.",
};

function usagePresentation(
  usage: ClaudeUsage | null,
  isLoading: boolean,
  loadError: boolean,
) {
  if (isLoading && usage === null) {
    return {
      status: "Checking",
      tone: "loading",
      note: "Checking local usage.",
      placeholder: "Checking",
    };
  }

  if (loadError) {
    return {
      status: "Unavailable",
      tone: "unavailable",
      note: "Relay could not read usage. Retrying automatically.",
      placeholder: "Unavailable",
    };
  }

  if (usage?.status === "available") {
    const updated = formatAge(usage.ageSeconds);
    return usage.source === "claude-statusline"
      ? {
          status: "Current",
          tone: "current",
          note: `Status line, updated ${updated}.`,
          placeholder: "Unavailable",
        }
      : {
          status: "Recent cache",
          tone: "cached",
          note: `ccstatusline cache, updated ${updated}.`,
          placeholder: "Unavailable",
        };
  }

  if (usage?.status === "stale") {
    return {
      status: "Stale",
      tone: "stale",
      note: `Updated ${formatAge(usage.ageSeconds)}. Old values hidden.`,
      placeholder: "Unavailable",
    };
  }

  if (usage?.status === "error") {
    return {
      status: "Error",
      tone: "error",
      note: reasonMessages[usage.reason ?? "unknown_upstream_error"]
        ?? "Usage could not be verified. Values hidden.",
      placeholder: "Unavailable",
    };
  }

  return {
    status: "Unavailable",
    tone: "unavailable",
    note: usage?.reason
      ? reasonMessages[usage.reason]
        ?? "No verified usage available. No estimates shown."
      : "No verified usage available. No estimates shown.",
    placeholder: "Unavailable",
  };
}

type UsageViewProps = {
  usage: ClaudeUsage | null;
  isLoading?: boolean;
  loadError?: boolean;
  onEnableLiveUsage?: () => void;
  isEnablingLiveUsage?: boolean;
};

function shouldOfferLiveUsage(
  usage: ClaudeUsage | null,
  isLoading: boolean,
  loadError: boolean,
) {
  if (isLoading || loadError) return false;
  if (usage === null) return true;
  if (usage.status === "stale") return true;
  if (usage.status === "available") return usage.source === "ccstatusline-cache";
  if (usage.status !== "unavailable") return false;

  return usage.reason === "live_snapshot_not_found"
    || usage.reason === "cache_not_found"
    || usage.reason === "no_usage_data";
}

export function UsageView({
  usage,
  isLoading = false,
  loadError = false,
  onEnableLiveUsage,
  isEnablingLiveUsage = false,
}: UsageViewProps) {
  const isCurrent = usage?.status === "available";
  const presentation = usagePresentation(usage, isLoading, loadError);
  const session = isCurrent && usage.sessionPercent != null
    ? `${usage.sessionPercent.toFixed(1)}%`
    : presentation.placeholder;
  const weekly = isCurrent && usage.weeklyPercent != null
    ? `${usage.weeklyPercent.toFixed(1)}%`
    : presentation.placeholder;
  const reset = isCurrent ? formatReset(usage.sessionResetAtMs) : presentation.placeholder;
  const offerLiveUsage = Boolean(onEnableLiveUsage)
    && shouldOfferLiveUsage(usage, isLoading, loadError);
  const providers = [
    {
      id: "claude-code",
      name: "Claude Code",
      status: presentation.status,
      tone: presentation.tone,
      note: presentation.note,
      metrics: [
        {
          label: "Reset",
          hint: "Five-hour window",
          value: reset,
          available: isCurrent && usage?.sessionResetAtMs != null,
        },
        {
          label: "Weekly",
          hint: "Seven-day used",
          value: weekly,
          available: isCurrent && usage?.weeklyPercent != null,
        },
        {
          label: "Session",
          hint: "Five-hour used",
          value: session,
          available: isCurrent && usage?.sessionPercent != null,
        },
      ],
    },
  ];

  return (
    <section className="secondary-view usage-view" aria-labelledby="usage-title">
      <header className="usage-page-header">
        <p>Account limits</p>
        <h1 id="usage-title">Usage</h1>
      </header>

      <div className="usage-providers">
        {providers.map((provider) => (
          <section
            className="usage-provider"
            aria-labelledby={`usage-provider-${provider.id}`}
            key={provider.id}
          >
            <header className="usage-provider-header">
              <div className="usage-provider-identity">
                <span className="usage-provider-mark" aria-hidden="true">
                  <AgentLogo provider="claude_code" />
                </span>
                <h2 id={`usage-provider-${provider.id}`}>{provider.name}</h2>
              </div>
              <span className="usage-status" data-tone={provider.tone}>
                {provider.status}
              </span>
            </header>

            <dl className="usage-metrics" aria-label={`${provider.name} usage`}>
              {provider.metrics.map((metric) => (
                <div className="usage-metric" key={metric.label}>
                  <dt>{metric.label}<small>{metric.hint}</small></dt>
                  <dd data-muted={!metric.available}>{metric.value}</dd>
                </div>
              ))}
            </dl>

            <footer className="usage-provider-footer">
              <p className="usage-note" role="status">
                <Gauge aria-hidden="true" />
                <span>{provider.note}</span>
              </p>
              {offerLiveUsage && (
                <button
                  className="usage-live-button"
                  type="button"
                  disabled={isEnablingLiveUsage}
                  onClick={onEnableLiveUsage}
                >
                  {isEnablingLiveUsage ? "Enabling..." : "Enable live usage"}
                </button>
              )}
            </footer>
          </section>
        ))}
      </div>
    </section>
  );
}

type SettingsViewProps = {
  sessionCount: number;
  cliAvailable: boolean;
  configDir?: string;
  isRefreshing: boolean;
  refreshIntervalMs: RefreshIntervalMs;
  logicalProcessorCount: number;
  deviceMemoryGb?: number;
  onRefresh: () => void | Promise<void>;
  onRefreshIntervalChange: (interval: RefreshIntervalMs) => void;
  onDisconnect: () => void;
};

export type RefreshIntervalMs = 5_000 | 10_000 | 30_000 | 60_000;

const refreshOptions: ReadonlyArray<{
  value: RefreshIntervalMs;
  label: string;
  description: string;
}> = [
  { value: 5_000, label: "5 sec", description: "Fastest" },
  { value: 10_000, label: "10 sec", description: "Frequent" },
  { value: 30_000, label: "30 sec", description: "Recommended" },
  { value: 60_000, label: "1 min", description: "Lightest" },
];

type RefreshIntervalPickerProps = {
  value: RefreshIntervalMs;
  onChange: (value: RefreshIntervalMs) => void;
  title: string;
};

function RefreshIntervalPicker({ value, onChange, title }: RefreshIntervalPickerProps) {
  const [isOpen, setIsOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const pendingFocusIndex = useRef<number | null>(null);
  const selectedIndex = refreshOptions.findIndex((option) => option.value === value);
  const selectedOption = refreshOptions[selectedIndex] ?? refreshOptions[2];

  useEffect(() => {
    if (!isOpen) return;

    const focusIndex = pendingFocusIndex.current ?? Math.max(0, selectedIndex);
    pendingFocusIndex.current = null;
    optionRefs.current[focusIndex]?.focus();

    const closeOnOutsidePress = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setIsOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePress);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePress);
  }, [isOpen, selectedIndex]);

  const openFromKeyboard = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    pendingFocusIndex.current = Math.max(0, selectedIndex);
    setIsOpen(true);
  };

  const moveOptionFocus = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex: number | null = null;
    if (event.key === "ArrowDown") nextIndex = (index + 1) % refreshOptions.length;
    if (event.key === "ArrowUp") nextIndex = (index - 1 + refreshOptions.length) % refreshOptions.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = refreshOptions.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    optionRefs.current[nextIndex]?.focus();
  };

  const chooseOption = (nextValue: RefreshIntervalMs) => {
    onChange(nextValue);
    setIsOpen(false);
    triggerRef.current?.focus();
  };

  return (
    <div
      className="refresh-picker"
      data-open={isOpen}
      ref={rootRef}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setIsOpen(false);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !isOpen) return;
        event.preventDefault();
        setIsOpen(false);
        triggerRef.current?.focus();
      }}
    >
      <button
        className="refresh-picker-trigger"
        type="button"
        aria-label={`Session refresh interval: ${selectedOption.label}`}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls="refresh-interval-options"
        title={title}
        ref={triggerRef}
        onClick={() => setIsOpen((current) => !current)}
        onKeyDown={openFromKeyboard}
      >
        <span>{selectedOption.label}</span>
        <CaretDown aria-hidden="true" weight="bold" />
      </button>

      {isOpen && (
        <div
          className="refresh-picker-menu"
          id="refresh-interval-options"
          role="listbox"
          aria-label="Refresh rate"
        >
          {refreshOptions.map((option, index) => {
            const isSelected = option.value === value;
            return (
              <button
                className="refresh-picker-option"
                type="button"
                role="option"
                aria-selected={isSelected}
                key={option.value}
                ref={(element) => { optionRefs.current[index] = element; }}
                tabIndex={isSelected ? 0 : -1}
                onClick={() => chooseOption(option.value)}
                onKeyDown={(event) => moveOptionFocus(event, index)}
              >
                <span>
                  <strong>{option.label}</strong>
                  <small>{option.description}</small>
                </span>
                <Check aria-hidden="true" weight="bold" />
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

export function describeRefreshImpact(
  intervalMs: RefreshIntervalMs,
  logicalProcessorCount: number,
  sessionCount: number,
  deviceMemoryGb?: number,
) {
  const constrainedDevice = logicalProcessorCount <= 4
    || (deviceMemoryGb !== undefined && deviceMemoryGb <= 4);
  const manySessions = sessionCount >= 20;

  if (intervalMs === 5_000 && (constrainedDevice || manySessions)) {
    return {
      tone: "warning",
      text: "May feel slower on this system while several sessions are active.",
    } as const;
  }

  if (intervalMs === 5_000) {
    return {
      tone: "active",
      text: "Fastest updates. More background work, but this system should handle it.",
    } as const;
  }

  if (intervalMs === 10_000 && constrainedDevice) {
    return {
      tone: "caution",
      text: "Frequent updates. A slower rate may feel smoother on this system.",
    } as const;
  }

  if (intervalMs === 10_000) {
    return {
      tone: "active",
      text: "Frequent updates with moderate background work.",
    } as const;
  }

  if (intervalMs === 30_000) {
    return {
      tone: "quiet",
      text: "Recommended. Timely updates with low background work.",
    } as const;
  }

  return {
    tone: "quiet",
    text: "Lightest background work. Updates may take up to a minute.",
  } as const;
}

export function SettingsView({
  sessionCount,
  cliAvailable,
  configDir,
  isRefreshing,
  refreshIntervalMs,
  logicalProcessorCount,
  deviceMemoryGb,
  onRefresh,
  onRefreshIntervalChange,
  onDisconnect,
}: SettingsViewProps) {
  const [notifications, setNotifications] = usePersistentBoolean("relay:review-alerts", true);
  const [privateMode, setPrivateMode] = usePersistentBoolean("relay:private-mode", true);
  const refreshImpact = describeRefreshImpact(
    refreshIntervalMs,
    logicalProcessorCount,
    sessionCount,
    deviceMemoryGb,
  );

  return (
    <section className="secondary-view" aria-labelledby="settings-title">
      <header>
        <p>Widget preferences</p>
        <h1 id="settings-title">Settings</h1>
      </header>
      <section className="settings-section" aria-labelledby="agents-settings-title">
        <h2 id="agents-settings-title">Agents</h2>
        <div className="detail-list">
          <div className="detail-row">
            <AgentLogo provider="claude_code" />
            <span>
              <strong>Claude Code</strong>
              <small title={configDir}>{sessionCount} local sessions</small>
            </span>
            <span className="state-label state-done">Connected</span>
          </div>
        </div>
      </section>
      <section className="settings-section" aria-labelledby="preferences-settings-title">
        <h2 id="preferences-settings-title">Preferences</h2>
      <div className="settings-list">
        <button type="button" aria-pressed={notifications} onClick={() => setNotifications(!notifications)}>
          <BellSimple aria-hidden="true" />
          <span><strong>Review alerts</strong><small>Notify when work is ready</small></span>
          <ToggleRight className={notifications ? "toggle-on" : ""} aria-hidden="true" weight="fill" />
        </button>
        <button type="button" aria-pressed={privateMode} onClick={() => setPrivateMode(!privateMode)}>
          <ShieldCheck aria-hidden="true" />
          <span><strong>Private mode</strong><small>Keep session details local</small></span>
          <ToggleRight className={privateMode ? "toggle-on" : ""} aria-hidden="true" weight="fill" />
        </button>
        <div className="settings-control-row">
          <Gauge aria-hidden="true" />
          <span>
            <strong>Refresh rate</strong>
            <small data-tone={refreshImpact.tone}>{refreshImpact.text}</small>
          </span>
          <RefreshIntervalPicker
            value={refreshIntervalMs}
            onChange={onRefreshIntervalChange}
            title={`Estimate based on ${logicalProcessorCount} logical processors and ${sessionCount} watched sessions`}
          />
        </div>
        <button type="button" disabled={isRefreshing} onClick={() => void onRefresh()}>
          <Gauge aria-hidden="true" />
          <span><strong>{isRefreshing ? "Refreshing…" : "Refresh sessions"}</strong><small>Read Claude's local metadata again</small></span>
          <CheckCircle aria-hidden="true" weight="regular" />
        </button>
        <button type="button" onClick={onDisconnect}>
          <ShieldCheck aria-hidden="true" />
          <span><strong>Reconnect Claude Code</strong><small>Show the local-access explanation again</small></span>
          <span aria-hidden="true">›</span>
        </button>
      </div>
      </section>
      {!cliAvailable && <p className="settings-warning">Claude sessions can be read, but the Claude CLI is not available to resume them.</p>}
      <p className="settings-confirmation"><CheckCircle aria-hidden="true" weight="fill" /> Changes save instantly.</p>
    </section>
  );
}
