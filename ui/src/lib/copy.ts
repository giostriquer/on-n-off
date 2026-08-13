export const copy = {
  cliMissing: (agent: string) =>
    `${agent} CLI not found. Plugin install and enable need it. Skill toggles still work.`,
  cliTooOld: (agent: string) => `${agent} CLI is too old for plugin commands.`,
  emptyPlugins: "No plugins on this circuit.",
  emptyUserSkills: "No user skills.",
  filterMiss: (q: string) => `Nothing matches “${q}”.`,
  skillLocked: "Claude only enables this with the whole plugin.",
  folderUnsupported: "This agent’s CLI can’t install from a folder.",
  parseError: (path: string, error: string) => `Couldn’t read ${path}. ${error}`,
  writeRollback: "Write failed. Restored the previous file.",
  uninstallTitle: (name: string) => `Uninstall “${name}”?`,
  uninstallBody: (agent: string) =>
    `Removes it from ${agent}. This does not delete your backup.`,
  installHelper: "HTTPS git or owner/repo. SSH later.",
  installInvalid: "Use an HTTPS git URL or owner/repo.",
  filterPlaceholder: "Filter plugins & skills…",
  refresh: "Refresh",
  install: "Install",
  installing: "Installing…",
  folder: "Choose folder…",
  cancel: "Cancel",
  uninstall: "Uninstall",
} as const;
