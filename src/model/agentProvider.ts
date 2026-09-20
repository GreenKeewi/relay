import type { SessionProvider } from "./session";

const PROVIDER_IDS_BY_LABEL: Record<string, SessionProvider> = {
  "Claude Code": "claude_code",
  Codex: "codex",
  "ChatGPT Work": "chatgpt_work",
  Zed: "zed",
};

export function providerIdFromLabel(label: string): SessionProvider | null {
  return PROVIDER_IDS_BY_LABEL[label] ?? null;
}
