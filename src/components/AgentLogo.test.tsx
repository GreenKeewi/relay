import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { providerIdFromLabel } from "../model/agentProvider";
import { AgentLogo } from "./AgentLogo";

describe("AgentLogo", () => {
  it.each([
    ["claude_code", "Claude%20Code"],
    ["codex", "Codex"],
    ["chatgpt_work", "OpenAI"],
    ["zed", "Zed%20Industries"],
  ] as const)("renders the real bundled mark for %s", (provider, embeddedTitle) => {
    const { container } = render(<AgentLogo provider={provider} />);
    const logo = container.querySelector("img.agent-logo");

    expect(logo).toHaveAttribute("src", expect.stringContaining("data:image/svg+xml"));
    expect(logo).toHaveAttribute("src", expect.stringContaining(embeddedTitle));
    expect(logo).toHaveAttribute("draggable", "false");
    expect(logo).toHaveAttribute("data-provider", provider);
  });

  it("maps display labels to the provider registry", () => {
    render(<span>Agent identities</span>);

    expect(screen.getByText("Agent identities")).toBeInTheDocument();
    expect(providerIdFromLabel("Claude Code")).toBe("claude_code");
    expect(providerIdFromLabel("Codex")).toBe("codex");
    expect(providerIdFromLabel("ChatGPT Work")).toBe("chatgpt_work");
    expect(providerIdFromLabel("Zed")).toBe("zed");
  });
});
