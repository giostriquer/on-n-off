import { readFile } from "node:fs/promises";
import { parseArgs } from "node:util";

const STEP_KEYS = ["click", "fill", "press", "hover", "scroll", "wait", "shot"];

const ARCHIVED = {
  claude: "role=region[name='Claude archived accounts']",
  codex: "role=region[name='Codex archived accounts']",
};
const ARCHIVED_OPEN = [
  { click: `${ARCHIVED.claude} >> role=button[name='Archived (2)']` },
  { click: `${ARCHIVED.codex} >> role=button[name='Archived (1)']` },
  { wait: "role=button[name='Unarchive former@example.com']" },
];
const ARCHIVE_ACTION = "role=group[name='Actions for person@acme.example'] >> role=button[name='Archive account']";

export const SCENES = [
  { name: "all-providers", url: "/overview?mock=ok&allProviders", viewport: { width: 960, height: 760 } },
  { name: "github-ok", url: "/github?mock=ok" },
  { name: "github-ok-light", url: "/github?mock=ok", theme: "light" },
  { name: "github-stale", url: "/github?mock=stale" },
  { name: "github-gh-missing", url: "/github?mock=ghMissing" },
  { name: "github-many", url: "/github?mock=many" },
  { name: "github-scrolled", url: "/github?mock=ok", steps: [{ click: "role=searchbox[name='Search pull requests']" }, { press: "PageDown" }, { press: "PageDown" }] },
  { name: "overview-catalog", url: "/overview?mock=catalog", steps: [{ scroll: "text=Live on this scope" }] },
  { name: "overview-catalog-wide", url: "/overview?mock=catalog", viewport: { width: 1440, height: 900 }, steps: [{ scroll: "text=Live on this scope" }] },
  { name: "overview-catalog-light", url: "/overview?mock=catalog", theme: "light", steps: [{ scroll: "text=Live on this scope" }] },
  { name: "overview-empty", url: "/overview?mock=ok", steps: [{ scroll: "text=Live on this scope" }] },
  { name: "settings-github", url: "/settings?mock=ok", steps: [{ wait: "role=region[name='Pull requests']" }] },
  { name: "hooks", url: "/hooks?mock=hooks", steps: [
    { wait: "role=heading[name='Hooks']" },
    { shot: "hooks" },
    { click: "role=tab[name='Codex']" },
    { wait: "text=acme-webapp-tools" },
    { shot: "hooks-codex" },
  ] },
  { name: "limits-band", url: "/limits?mock=limitsBand", steps: [{ wait: "role=region[name='Codex limits · 50% of the week']" }] },
  { name: "limits-band-light", url: "/limits?mock=limitsBand", theme: "light", steps: [{ wait: "role=region[name='Codex limits · 50% of the week']" }] },
  { name: "limits-muse-idle", url: "/limits?mock=museIdle", viewport: { width: 1280, height: 900 }, steps: [{ wait: "text=Muse Code Power Usage" }] },
  { name: "limits-ok", url: "/limits?mock=ok", steps: [{ wait: "role=region[name='Codex limits · person@acme.example']" }] },
  { name: "limits-ok-light", url: "/limits?mock=ok", theme: "light", steps: [{ wait: "role=region[name='Codex limits · person@acme.example']" }] },
  { name: "limits-sign-in", url: "/limits?mock=claudeSignIn", steps: [{ wait: "role=button[name='Claude sign-in ended Aug 23']" }, { hover: "role=button[name='Claude sign-in ends Aug 27']" }, { wait: "role=tooltip" }] },
  { name: "limits-sign-in-light", url: "/limits?mock=claudeSignIn", theme: "light", steps: [{ wait: "role=button[name='Claude sign-in ended Aug 23']" }] },
  ...[960, 1024, 1120, 1440].map(width => ({
    name: `limits-sign-in-${width}`, url: "/limits?mock=claudeSignIn", viewport: { width, height: 980 },
    limitsColumns: 3,
    steps: [{ wait: "role=button[name='Claude sign-in ended Aug 23']" }],
  })),
  { name: "limits-saved-logins-1280", url: "/limits?mock=savedLogins", viewport: { width: 1280, height: 980 }, limitsColumns: 3, steps: [{ wait: "role=region[name='Codex limits · moved@example.com']" }] },
  { name: "limits-saved-logins-menu", url: "/limits?mock=savedLogins", viewport: { width: 1280, height: 980 }, steps: [
    { click: "role=region[name='Codex limits · refused@example.com'] >> role=button[name='More actions for refused@example.com']" },
    { wait: "role=group[name='Actions for refused@example.com']" },
  ] },
  { name: "limits-saved-paused-1280", url: "/limits?mock=savedRefreshPaused", viewport: { width: 1280, height: 980 }, limitsColumns: 3, steps: [{ wait: "role=button[name='Usage status: Last known usage'] >> nth=0" }] },
  ...["light", "dark"].map(theme => ({
    name: `usage-muse-${theme}`, url: "/usage?mock=ok", theme, checkMuseChart: true,
    steps: [{ wait: "role=region[name='Usage over time'] >> svg path[stroke] >> nth=0" }],
  })),
  { name: "limits-popover", url: "/?surface=limits-popover&mock=ok", viewport: { width: 350, height: 480 }, popoverHeaders: true, steps: [{ wait: "role=article[name='Codex limits · person@acme.example']" }] },
  { name: "popover-saved-paused", url: "/?surface=limits-popover&mock=savedRefreshPaused", viewport: { width: 350, height: 700 }, popoverHeaders: true, steps: [{ wait: "role=button[name='Usage status: Last known usage'] >> nth=0" }] },
  { name: "limits-archived", url: "/limits?mock=archivedAccounts", viewport: { width: 1120, height: 1400 }, steps: [{ wait: `${ARCHIVED.claude} >> role=button[name='Archived (2)']` }] },
  { name: "limits-archived-open", url: "/limits?mock=archivedAccounts", viewport: { width: 1120, height: 1400 }, steps: ARCHIVED_OPEN },
  { name: "limits-archived-open-light", url: "/limits?mock=archivedAccounts", theme: "light", viewport: { width: 1120, height: 1400 }, steps: ARCHIVED_OPEN },
  { name: "limits-archive-menu", url: "/limits?mock=archivedAccounts", viewport: { width: 1120, height: 1400 }, steps: [
    { click: "role=button[name='More actions for person@acme.example']" },
    { wait: ARCHIVE_ACTION },
    { shot: "limits-archive-menu" },
    { click: ARCHIVE_ACTION },
    { click: `${ARCHIVED.codex} >> role=button[name='Archived (2)']` },
    { wait: "role=button[name='Unarchive person@acme.example']" },
    { shot: "limits-archived-after-archive" },
  ] },
  { name: "popover-archived", url: "/?surface=limits-popover&mock=archivedAccounts", viewport: { width: 350, height: 900 }, popoverHeaders: true, steps: [{ wait: "role=article[name='Codex limits · person@acme.example']" }] },
];

export async function chooseScenes(argv, available = SCENES) {
  const { values, positionals } = parseArgs({ args: argv, options: { layout: { type: "boolean" }, csp: { type: "boolean" } }, allowPositionals: true });
  if (values.layout && values.csp) throw new Error("--layout and --csp are separate runs");
  if (values.csp) return { csp: true, scenes: policyScenes(available, positionals) };
  const scenes = values.layout ? layoutScenes(available, positionals) : await fileScenes(available, positionals);
  scenes.forEach(validateScene);
  return { csp: false, scenes };
}

function policyScenes(available, files) {
  if (files.length > 0) throw new Error("--csp runs every built-in scene and takes no scene file");
  available.forEach(validateScene);
  return available.map(({ limitsColumns, popoverHeaders, ...scene }) => scene);
}

function layoutScenes(available, files) {
  if (files.length > 0) throw new Error("--layout runs the built-in header scenes and takes no scene file");
  const scenes = available.filter((scene) => scene.limitsColumns || scene.popoverHeaders);
  if (!scenes.some((scene) => scene.limitsColumns)) throw new Error("a layout run needs a Limits card scene");
  if (!scenes.some((scene) => scene.popoverHeaders)) throw new Error("a layout run needs a popover scene");
  return scenes;
}

async function fileScenes(available, files) {
  if (files.length > 1) throw new Error("ui-shots takes one scene file");
  const scenes = files.length === 0 ? available : JSON.parse(await readFile(files[0], "utf8"));
  if (!Array.isArray(scenes)) throw new Error("the scene file must hold an array of scenes");
  if (scenes.length === 0) throw new Error("there are no scenes to run");
  return scenes;
}

function validateScene(scene, index) {
  if (!scene || typeof scene.name !== "string" || typeof scene.url !== "string") {
    throw new Error(`scene ${index}: needs a string "name" and "url"`);
  }
  for (const [stepIndex, step] of (scene.steps ?? []).entries()) {
    const keys = Object.keys(step).filter((key) => STEP_KEYS.includes(key));
    if (keys.length !== 1) {
      throw new Error(
        `scene ${scene.name} step ${stepIndex}: expected exactly one of ${STEP_KEYS.join("/")}, got ${JSON.stringify(step)}`,
      );
    }
  }
}
