import type { GithubListId } from "./githubTypes";
import type { AgentId } from "./types";

export type NotchEdge = "left" | "right" | "top" | "bottom";
export type NotchSize = "compact" | "standard" | "large";
/** Whether the rail stays open or waits behind a small pill at the edge. */
export type NotchShowMode = "always" | "onHover";
export type NotchSettings = {
  enabled: boolean;
  displayId: string | null;
  edge: NotchEdge;
  size: NotchSize;
  show: NotchShowMode;
  /** Providers with a cell on the rail; the backend keeps them in rail order. */
  providers: AgentId[];
  /** The pull-request cell: on by default, listing only the user's own pull requests. */
  pullRequests: { enabled: boolean; lists: GithubListId[] };
};
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
