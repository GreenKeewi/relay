import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  dragDockToNearestEdge,
  invokeWindowCommand,
  snapDockToNearestEdge,
  type DockEdge,
} from "./tauriBridge";

type DockViewProps = {
  recentCount: number;
  idleCount: number;
};

export function DockView({ recentCount, idleCount }: DockViewProps) {
  const [edge, setEdge] = useState<DockEdge>("right");
  const dragOrigin = useRef<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const suppressClickUntil = useRef(0);
  const total = recentCount + idleCount;
  const gap = recentCount > 0 && idleCount > 0 ? 3 : 0;
  const available = 100 - gap;
  const recentArc = total > 0 ? (recentCount / total) * available : 0;
  const idleArc = total > 0 ? (idleCount / total) * available : 0;
  const label = `${recentCount} in progress and ${idleCount} to review Claude Code sessions`;

  useEffect(() => {
    void snapDockToNearestEdge().then(setEdge).catch(() => undefined);
  }, []);

  const startPointer = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    dragOrigin.current = { x: event.clientX, y: event.clientY };
  };

  const movePointer = (event: ReactPointerEvent<HTMLElement>) => {
    const origin = dragOrigin.current;
    if (!origin || dragging.current) return;
    if (Math.hypot(event.clientX - origin.x, event.clientY - origin.y) < 4) return;

    dragging.current = true;
    dragOrigin.current = null;
    suppressClickUntil.current = Date.now() + 350;
    void dragDockToNearestEdge()
      .then(setEdge)
      .catch(() => undefined)
      .finally(() => {
        dragging.current = false;
      });
  };

  const endPointer = () => {
    dragOrigin.current = null;
  };

  const openRelay = () => {
    if (Date.now() < suppressClickUntil.current) return;
    invokeWindowCommand("expand_from_dock", "relay:expand-from-dock");
  };

  return (
    <main
      className="dock-view"
      aria-label={label}
      data-edge={edge}
      onPointerDown={startPointer}
      onPointerMove={movePointer}
      onPointerUp={endPointer}
      onPointerCancel={endPointer}
    >
      <button
        className="dock-ring"
        type="button"
        aria-label={`Open Relay. ${label}`}
        title={label}
        onClick={openRelay}
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
    </main>
  );
}
