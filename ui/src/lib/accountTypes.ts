import type { AgentId } from "./types";
export type AccountIdentity = { provider: AgentId; userId: string; workspaceId: string };
export type SavedProfile = { id: string; observationId: string; legacyId?: string | null; identity: AccountIdentity; label: string; email?: string | null; category?: string | null; savedAt: string; active: boolean; needsLogin: boolean; pendingActivation?: boolean; archived?: boolean; signInEndsAt?: string | null };
export type AccountsReading = { profiles: SavedProfile[]; nativeAccount: AccountIdentity | null; nativeObservationId?: string | null; recoveryRequired: boolean; rememberAccounts?: boolean; notice: string | null };
export type AccountCategories = Partial<Record<import("./providers").LimitsProvider, Record<string, string>>>;
export type AccountAction = "unlock" | "remember" | "stopRemembering" | "save" | "use" | "useAlongsideClients" | "remove" | "signOut" | "recover";
