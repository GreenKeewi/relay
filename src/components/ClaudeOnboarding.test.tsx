import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ClaudeOnboarding } from "./ClaudeOnboarding";

const baseProps = {
  detection: null,
  sessions: [],
  isDetecting: false,
  onContinue: vi.fn(),
  onRetry: vi.fn(),
  onOpenSession: vi.fn(),
};

describe("ClaudeOnboarding navigation", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows and invokes the supplied back action", () => {
    const onBack = vi.fn();
    render(
      <ClaudeOnboarding
        {...baseProps}
        stage="detection"
        onBack={onBack}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("does not show a back button when the flow has nowhere to return", () => {
    render(<ClaudeOnboarding {...baseProps} stage="privacy" />);

    expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
  });
});
