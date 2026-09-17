import type { AgentId } from "./types";
export type AccountProvider = Extract<AgentId, "claude" | "codex">;
export type AccountIdentity = { provider: AgentId; userId: string; workspaceId: string };
export type SavedProfile = { id: string; observationId?: string; identity: AccountIdentity; label: string; email?: string | null; category?: string | null; savedAt: string; active: boolean; needsLogin: boolean; pendingActivation?: boolean };
export type AccountsReading = { profiles: SavedProfile[]; nativeAccount: AccountIdentity | null; nativeObservationId?: string | null; recoveryRequired: boolean; rememberAccounts?: boolean; notice: string | null };
export type AccountAction = "unlock" | "remember" | "stopRemembering" | "save" | "use" | "useAlongsideClients" | "remove" | "rename" | "category" | "signOut" | "recover";
