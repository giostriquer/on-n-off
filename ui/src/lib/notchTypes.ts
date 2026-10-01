import type { GithubListId } from "./githubTypes";
import type { AgentId } from "./types";

export type NotchEdge = "left" | "right" | "top" | "bottom";
export type NotchSize = "compact" | "standard" | "large";
export type NotchShowMode = "always" | "onHover";
export type NotchSettings = {
  enabled: boolean;
  displayId: string | null;
  edge: NotchEdge;
  size: NotchSize;
  show: NotchShowMode;
  providers: AgentId[];
  pullRequests: { enabled: boolean; lists: GithubListId[] };
};
export function defaultNotchSettings(overrides: Partial<NotchSettings> = {}): NotchSettings {
  return {
    enabled: false,
    displayId: null,
    edge: "right",
    size: "standard",
    show: "always",
    providers: ["claude", "codex", "antigravity", "cursor"],
    pullRequests: { enabled: true, lists: ["mine"] },
    ...overrides,
  };
}

export type NotchDisplay = {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  workY: number;
  workHeight: number;
  scale: number;
  mirrored: boolean;
};
export type NotchSnapshot = {
  revision: number;
  supported: boolean;
  settings: NotchSettings;
  displays: NotchDisplay[];
  error: string | null;
};
export type NotchChanged = { snapshot: NotchSnapshot };
