export const marketplaceCopy = {
  installSummary: (picked: number, required: number, missing: number) =>
    [
      required > 0 ? `${picked} picked + ${required} required` : `${picked} picked`,
      missing > 0 ? `${missing} not selected` : null,
    ]
      .filter(Boolean)
      .join(" · "),
  needs: "needs",
  requiredBy: (names: string) => `required by ${names}`,
  depNotSelected: "not selected",
  depAdd: (name: string) => `+ add ${name}`,
  pluginExtrasAdvisory: (extras: string) =>
    `This plugin also ships ${extras}; local copies won’t include them — Install plugin covers everything.`,
  pluginFilesAdvisory: (names: string, plural: boolean) =>
    `${names} ${plural ? "use" : "uses"} plugin files a local copy won’t include — Install plugin covers everything.`,
  extraCommands: "commands",
  extraHooks: "hooks",
  extraMcp: "MCP servers",
} as const;
