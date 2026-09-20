import claudeCodeLogo from "../assets/agent-logos/claude-code.svg";
import codexLogo from "../assets/agent-logos/codex.svg";
import openAiLogo from "../assets/agent-logos/openai.svg";
import zedLogo from "../assets/agent-logos/zed.svg";
import type { SessionProvider } from "../model/session";

const AGENT_LOGOS: Record<SessionProvider, { label: string; src: string }> = {
  claude_code: { label: "Claude Code", src: claudeCodeLogo },
  codex: { label: "Codex", src: codexLogo },
  chatgpt_work: { label: "ChatGPT Work", src: openAiLogo },
  zed: { label: "Zed", src: zedLogo },
};

type AgentLogoProps = {
  provider: SessionProvider;
  className?: string;
};

export function AgentLogo({ provider, className }: AgentLogoProps) {
  const logo = AGENT_LOGOS[provider];

  return (
    <img
      className={["agent-logo", className].filter(Boolean).join(" ")}
      data-provider={provider}
      src={logo.src}
      alt=""
      aria-hidden="true"
      draggable={false}
      title={logo.label}
    />
  );
}
