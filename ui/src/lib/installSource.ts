export type InstallSource =
  | { kind: "git-url"; value: string }
  | { kind: "github"; owner: string; repo: string; ref?: string }
  | { kind: "plugin"; id: string }
  | { kind: "folder"; value: string };

const GITHUB_SHORTHAND = /^([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)(?:@([A-Za-z0-9._/-]+))?$/;
const PLUGIN_ID = /^[^/\s\\]+@[^/\s\\]+$/;
const HTTPS_GIT = /^https:\/\/[^\s]+\/[^\s]+$/i;

export function parseInstallSource(input: string): InstallSource | { error: string } {
  const value = input.trim();
  if (!value) {
    return { error: "Use an HTTPS git URL or owner/repo." };
  }
  if (/^(git@|ssh:\/\/)/i.test(value)) {
    return { error: "Use an HTTPS git URL or owner/repo." };
  }
  if (HTTPS_GIT.test(value)) {
    return { kind: "git-url", value };
  }
  if (looksLikeAbsPath(value)) {
    return { kind: "folder", value };
  }
  if (PLUGIN_ID.test(value)) {
    return { kind: "plugin", id: value };
  }
  const shorthand = GITHUB_SHORTHAND.exec(value);
  if (shorthand) {
    return {
      kind: "github",
      owner: shorthand[1],
      repo: shorthand[2],
      ref: shorthand[3],
    };
  }
  return { error: "Use an HTTPS git URL or owner/repo." };
}

export function isValidInstallInput(text: string): boolean {
  return !("error" in parseInstallSource(text));
}

export function resolvedInstallSource(text: string): string | null {
  const parsed = parseInstallSource(text);
  if ("error" in parsed) {
    return null;
  }
  if (parsed.kind === "github") {
    return parsed.ref ? `${parsed.owner}/${parsed.repo}@${parsed.ref}` : `${parsed.owner}/${parsed.repo}`;
  }
  if (parsed.kind === "plugin") {
    return parsed.id;
  }
  return parsed.value;
}

function looksLikeAbsPath(value: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(value) || value.startsWith("\\\\") || value.startsWith("/");
}
