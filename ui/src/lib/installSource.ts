export type InstallSource =
  | { kind: "git-url"; value: string }
  | { kind: "github"; owner: string; repo: string; ref?: string }
  | { kind: "plugin"; id: string }
  | { kind: "folder"; value: string }
  | { kind: "npx-skills"; source: string; skill?: string };

const GITHUB_SHORTHAND = /^([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)(?:@([A-Za-z0-9._/-]+))?$/;
const PLUGIN_ID = /^[^/\s\\]+@[^/\s\\]+$/;
const HTTPS_GIT = /^https:\/\/[^\s]+\/[^\s]+$/i;
const INVALID = "Use an HTTPS git URL, owner/repo, name@marketplace, or npx skills add.";

export function parseInstallSource(input: string): InstallSource | { error: string } {
  const value = input.trim();
  if (!value) {
    return { error: INVALID };
  }
  if (/^(git@|ssh:\/\/)/i.test(value)) {
    return { error: INVALID };
  }
  const npx = parseNpxSkills(value);
  if (npx) {
    return npx;
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
  return { error: INVALID };
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
  if (parsed.kind === "npx-skills") {
    return parsed.skill
      ? `npx skills add ${parsed.source} --skill ${parsed.skill}`
      : `npx skills add ${parsed.source}`;
  }
  return parsed.value;
}

export function installHint(parsed: InstallSource | { error: string }): string {
  if ("error" in parsed) {
    return "HTTPS git, owner/repo, name@marketplace, or npx skills add. SSH later.";
  }
  switch (parsed.kind) {
    case "plugin":
      return "Install this plugin from a registered marketplace.";
    case "github":
      return "Add this GitHub repo as a marketplace. Then install plugins with name@marketplace.";
    case "git-url":
      return "Add this git repo as a marketplace.";
    case "folder":
      return "Link a local marketplace or plugin folder.";
    case "npx-skills":
      return parsed.skill
        ? `Install skill “${parsed.skill}” globally via npx.`
        : "Install skills globally via npx skills add.";
  }
}

function parseNpxSkills(value: string): InstallSource | { error: string } | null {
  const tokens = tokenize(value);
  const first = tokens[0];
  if (!first) {
    return null;
  }
  const npx = first.toLowerCase() === "npx";
  if (!npx && first.toLowerCase() !== "skills") {
    return null;
  }
  let index = npx ? 1 : 0;
  if (npx) {
    while (index < tokens.length && isNpxPrefixFlag(tokens[index])) {
      index += 1;
    }
  }
  if (tokens[index]?.toLowerCase() !== "skills") {
    return { error: INVALID };
  }
  index += 1;
  if (tokens[index]?.toLowerCase() !== "add") {
    return { error: INVALID };
  }
  index += 1;
  let source: string | undefined;
  let skill: string | undefined;
  while (index < tokens.length) {
    const token = tokens[index];
    if (token === "--skill" || token === "-s") {
      index += 1;
      skill = tokens[index];
    } else if (token === "-a" || token === "--agent" || token === "--agents") {
      index += 1;
    } else if (!isSkillsIgnoreFlag(token) && !token.startsWith("-") && !source) {
      source = token;
    }
    index += 1;
  }
  if (!source) {
    return { error: INVALID };
  }
  if (/^(git@|ssh:\/\/)/i.test(source)) {
    return { error: INVALID };
  }
  return skill ? { kind: "npx-skills", source, skill } : { kind: "npx-skills", source };
}

function tokenize(value: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: string | null = null;
  for (const ch of value) {
    if (quote) {
      if (ch === quote) {
        quote = null;
      } else {
        current += ch;
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (/\s/.test(ch)) {
      if (current) {
        tokens.push(current);
        current = "";
      }
      continue;
    }
    current += ch;
  }
  if (current) {
    tokens.push(current);
  }
  return tokens;
}

function isNpxPrefixFlag(token: string): boolean {
  return token === "-y" || token === "--yes" || token === "-q" || token === "--quiet";
}

function isSkillsIgnoreFlag(token: string): boolean {
  return (
    token === "-g" ||
    token === "--global" ||
    token === "-y" ||
    token === "--yes" ||
    token === "--all" ||
    token === "--copy" ||
    token === "-l" ||
    token === "--list"
  );
}

function looksLikeAbsPath(value: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(value) || value.startsWith("\\\\") || value.startsWith("/");
}
