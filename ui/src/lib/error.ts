import { copy } from "./copy";
import type { AdapterError } from "./types";

export function parseInvokeError(error: unknown): AdapterError {
  if (error && typeof error === "object" && "kind" in error && "message" in error) {
    const value = error as AdapterError;
    return {
      kind: value.kind,
      message: value.message,
      path: value.path ?? null,
    };
  }
  return {
    kind: "message",
    message: error instanceof Error ? error.message : String(error),
    path: null,
  };
}

export function displayError(error: AdapterError, agentName: string): string {
  switch (error.kind) {
    case "cli_missing":
      return copy.cliMissing(agentName);
    case "cli_too_old":
      return copy.cliTooOld(agentName);
    case "parse":
      return copy.parseError(error.path ?? "config", error.message);
    case "write":
      return copy.writeRollback;
    case "message":
      return error.message;
  }
}
