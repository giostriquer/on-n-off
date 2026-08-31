export type NotchEdge = "left" | "right";
export type NotchSize = "compact" | "standard" | "large";
export type NotchSettings = {
  enabled: boolean;
  displayId: string | null;
  edge: NotchEdge;
  size: NotchSize;
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
