import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  closeDockMenu,
  dragDockToNearestEdge,
  hideDockFor,
  invokeWindowCommand,
  openDockMenu,
  snapDockToNearestEdge,
  type DockEdge,
} from "./tauriBridge";

type DockViewProps = {
  recentCount: number;
  idleCount: number;
};

export function DockView({ recentCount, idleCount }: DockViewProps) {
  const [edge, setEdge] = useState<DockEdge>("right");
  const [isDragging, setIsDragging] = useState(false);
  const [menuState, setMenuState] = useState<"closed" | "open" | "closing">("closed");
  const dragOrigin = useRef<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const suppressClickUntil = useRef(0);
  const closeTimer = useRef<number | null>(null);
  const total = recentCount + idleCount;
  const gap = recentCount > 0 && idleCount > 0 ? 3 : 0;
  const available = 100 - gap;
  const recentArc = total > 0 ? (recentCount / total) * available : 0;
  const idleArc = total > 0 ? (idleCount / total) * available : 0;
  const label = `${recentCount} in progress and ${idleCount} to review Claude Code sessions`;

  useEffect(() => {
    void snapDockToNearestEdge()
      .then((placement) => setEdge(placement.edge))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && menuState === "open") closeQuickActions();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("keydown", closeOnEscape);
      if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    };
  }, [menuState]);

  const startPointer = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || menuState !== "closed") return;
    dragOrigin.current = { x: event.clientX, y: event.clientY };
  };

  const movePointer = (event: ReactPointerEvent<HTMLElement>) => {
    const origin = dragOrigin.current;
    if (!origin || dragging.current) return;
    if (Math.hypot(event.clientX - origin.x, event.clientY - origin.y) < 4) return;

    dragging.current = true;
    setIsDragging(true);
    dragOrigin.current = null;
    suppressClickUntil.current = Number.POSITIVE_INFINITY;
    void dragDockToNearestEdge()
      .then((placement) => setEdge(placement.edge))
      .catch(() => undefined)
      .finally(() => {
        suppressClickUntil.current = Date.now() + 350;
        dragging.current = false;
        setIsDragging(false);
      });
  };

  const endPointer = () => {
    dragOrigin.current = null;
  };

  const openQuickActions = () => {
    if (Date.now() < suppressClickUntil.current) return;
    if (menuState !== "closed") return;
    void openDockMenu()
      .then((placement) => {
        setEdge(placement.edge);
        setMenuState("open");
      })
      .catch(() => undefined);
  };

  const closeQuickActions = () => {
    if (menuState !== "open") return;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduceMotion) {
      void closeDockMenu().then((placement) => setEdge(placement.edge)).catch(() => undefined);
      setMenuState("closed");
      return;
    }
    setMenuState("closing");
    closeTimer.current = window.setTimeout(() => {
      void closeDockMenu().then((placement) => setEdge(placement.edge)).catch(() => undefined);
      setMenuState("closed");
      closeTimer.current = null;
    }, 150);
  };

  const openRelay = () => {
    setMenuState("closed");
    invokeWindowCommand("expand_from_dock", "relay:expand-from-dock");
  };

  const hideFor = (durationSeconds: 3_600 | 86_400) => {
    setMenuState("closed");
    void hideDockFor(durationSeconds).catch(() => undefined);
  };

  const menuOrigin = edge === "right"
    ? "top-right"
    : edge === "top"
      ? "top-center"
      : edge === "bottom"
        ? "bottom-center"
        : "top-left";

  return (
    <main
      className="dock-view"
      aria-label={label}
      data-edge={edge}
      data-dragging={isDragging}
      data-menu-open={menuState !== "closed"}
      onPointerDown={startPointer}
      onPointerMove={movePointer}
      onPointerUp={endPointer}
      onPointerCancel={endPointer}
      onClick={(event) => {
        if (event.target === event.currentTarget) closeQuickActions();
      }}
    >
      <button
        className="dock-ring"
        type="button"
        aria-label={`Quick actions. ${label}`}
        aria-haspopup="menu"
        aria-expanded={menuState !== "closed"}
        title={label}
        onClick={openQuickActions}
      >
        <svg className="dock-ring-chart" viewBox="0 0 40 40" aria-hidden="true">
          <circle className="dock-ring-track" cx="20" cy="20" r="16" pathLength="100" />
          <circle
            className="dock-ring-progress dock-ring-active"
            cx="20"
            cy="20"
            r="16"
            pathLength="100"
            style={{ strokeDasharray: `${recentArc} ${100 - recentArc}` }}
          />
          <circle
            className="dock-ring-progress dock-ring-idle"
            cx="20"
            cy="20"
            r="16"
            pathLength="100"
            style={{
              strokeDasharray: `${idleArc} ${100 - idleArc}`,
              strokeDashoffset: -(recentArc + gap),
            }}
          />
        </svg>
        <span className="dock-ring-core">
          <strong>{idleCount}</strong>
          <small>review</small>
        </span>
      </button>

      {menuState !== "closed" && (
        <div
          className={`dock-quick-menu t-dropdown ${menuState === "open" ? "is-open" : "is-closing"}`}
          data-origin={menuOrigin}
          role="menu"
          aria-label="Relay quick actions"
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => event.stopPropagation()}
        >
          <button type="button" role="menuitem" onClick={openRelay}>Open Relay</button>
          <button type="button" role="menuitem" onClick={() => hideFor(3_600)}>Hide for 1 hour</button>
          <button type="button" role="menuitem" onClick={() => hideFor(86_400)}>Hide for 24 hours</button>
        </div>
      )}
    </main>
  );
}
