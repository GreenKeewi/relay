import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DockView } from "./DockView";

const { invokeMock, startDraggingMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  startDraggingMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: startDraggingMock }),
}));

const placement = (edge: "top" | "right" | "bottom" | "left") => ({
  edge,
  x: edge === "right" ? 100 : 0,
  y: edge === "bottom" ? 100 : 0,
});

describe("DockView", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    startDraggingMock.mockReset();
    startDraggingMock.mockResolvedValue(undefined);
    vi.stubGlobal("PointerEvent", MouseEvent);
    vi.stubGlobal(
      "matchMedia",
      vi.fn().mockReturnValue({ matches: false }),
    );
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it.each(["top", "right", "bottom", "left"] as const)(
    "reflects a %s wall placement in its attached shape",
    async (edge) => {
      invokeMock.mockResolvedValue(placement(edge));

      render(<DockView recentCount={2} idleCount={1} />);

      const dock = screen.getByRole("main", {
        name: "2 in progress and 1 to review Claude Code sessions",
      });
      await waitFor(() => expect(dock).toHaveAttribute("data-edge", edge));
    },
  );

  it("becomes circular for the full native drag, then adopts the snapped edge", async () => {
    const calls: string[] = [];
    let snapCount = 0;
    let releaseDrag!: () => void;
    const nativeDrag = new Promise<void>((resolve) => {
      releaseDrag = resolve;
    });

    invokeMock.mockImplementation(async (command: string) => {
      calls.push(command);
      if (command === "prepare_dock_drag") return undefined;
      if (command === "snap_dock_to_nearest_edge") {
        snapCount += 1;
        return snapCount === 1 ? placement("left") : placement("bottom");
      }
      return undefined;
    });
    startDraggingMock.mockImplementation(() => {
      calls.push("startDragging");
      return nativeDrag;
    });

    render(<DockView recentCount={2} idleCount={1} />);
    const dock = screen.getByRole("main", {
      name: "2 in progress and 1 to review Claude Code sessions",
    });
    await waitFor(() => expect(dock).toHaveAttribute("data-edge", "left"));

    fireEvent.pointerDown(dock, { button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(dock, { clientX: 18, clientY: 10 });

    await waitFor(() => expect(dock).toHaveAttribute("data-dragging", "true"));
    expect(calls.slice(-2)).toEqual(["prepare_dock_drag", "startDragging"]);

    releaseDrag();

    await waitFor(() => {
      expect(dock).toHaveAttribute("data-dragging", "false");
      expect(dock).toHaveAttribute("data-edge", "bottom");
    });
    expect(calls.slice(-3)).toEqual([
      "prepare_dock_drag",
      "startDragging",
      "snap_dock_to_nearest_edge",
    ]);
  });

  it("opens quick actions on a click instead of expanding Relay immediately", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "snap_dock_to_nearest_edge") return placement("right");
      if (command === "open_dock_menu") return placement("right");
      return undefined;
    });

    render(<DockView recentCount={1} idleCount={2} />);
    const trigger = screen.getByRole("button", { name: /Quick actions/ });
    fireEvent.click(trigger);

    expect(await screen.findByRole("menu", { name: "Relay quick actions" })).toBeVisible();
    expect(invokeMock).toHaveBeenCalledWith("open_dock_menu");
    expect(invokeMock).not.toHaveBeenCalledWith("expand_from_dock", expect.anything());
  });

  it.each([
    ["Hide for 1 hour", 3_600],
    ["Hide for 24 hours", 86_400],
  ] as const)("runs the %s timed hide action natively", async (label, durationSeconds) => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "snap_dock_to_nearest_edge") return placement("left");
      if (command === "open_dock_menu") return placement("left");
      return undefined;
    });

    render(<DockView recentCount={0} idleCount={2} />);
    fireEvent.click(screen.getByRole("button", { name: /Quick actions/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: label }));

    expect(invokeMock).toHaveBeenCalledWith("hide_dock_for", { durationSeconds });
  });

  it("opens Relay from the quick actions menu", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "snap_dock_to_nearest_edge") return placement("bottom");
      if (command === "open_dock_menu") return placement("bottom");
      return undefined;
    });

    render(<DockView recentCount={3} idleCount={1} />);
    fireEvent.click(screen.getByRole("button", { name: /Quick actions/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open Relay" }));

    expect(invokeMock).toHaveBeenCalledWith("expand_from_dock", { reducedMotion: false });
  });

  it("suppresses release clicks after a long drag but preserves later menu clicks", async () => {
    let now = 1_000;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    let releaseDrag!: () => void;
    const nativeDrag = new Promise<void>((resolve) => {
      releaseDrag = resolve;
    });
    let snapCount = 0;

    invokeMock.mockImplementation(async (command: string) => {
      if (command === "snap_dock_to_nearest_edge") {
        snapCount += 1;
        return placement(snapCount === 1 ? "right" : "top");
      }
      return undefined;
    });
    startDraggingMock.mockReturnValue(nativeDrag);

    render(<DockView recentCount={0} idleCount={3} />);
    const dock = screen.getByRole("main", {
      name: "0 in progress and 3 to review Claude Code sessions",
    });
    const quickActionsButton = screen.getByRole("button", { name: /Quick actions/ });

    await waitFor(() => expect(dock).toHaveAttribute("data-edge", "right"));

    fireEvent.pointerDown(dock, { button: 0, clientX: 8, clientY: 8 });
    fireEvent.pointerMove(dock, { clientX: 16, clientY: 8 });
    await waitFor(() => expect(dock).toHaveAttribute("data-dragging", "true"));

    now += 2_000;
    fireEvent.click(quickActionsButton);
    expect(invokeMock).not.toHaveBeenCalledWith("open_dock_menu");

    releaseDrag();
    await waitFor(() => expect(dock).toHaveAttribute("data-dragging", "false"));
    fireEvent.click(quickActionsButton);
    expect(invokeMock).not.toHaveBeenCalledWith("open_dock_menu");

    now += 351;
    fireEvent.click(quickActionsButton);
    expect(invokeMock).toHaveBeenCalledWith("open_dock_menu");
  });
});
