import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { chooseScenes, SCENES } from "./ui-scenes.mjs";

const names = (scenes) => scenes.map((scene) => scene.name);
const chosen = async (argv) => (await chooseScenes(argv)).scenes;

function sceneFile(t, contents) {
  const dir = mkdtempSync(join(tmpdir(), "ui-scenes-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const file = join(dir, "scenes.json");
  writeFileSync(file, JSON.stringify(contents));
  return file;
}

test("--layout runs exactly the Limits card and popover header scenes", async () => {
  const layout = await chosen(["--layout"]);
  assert.deepEqual(names(layout), [
    "limits-sign-in-960",
    "limits-sign-in-1024",
    "limits-sign-in-1120",
    "limits-sign-in-1440",
    "limits-saved-logins-1280",
    "limits-saved-paused-1280",
    "limits-popover",
    "popover-saved-paused",
    "popover-archived",
  ]);
  for (const scene of SCENES.filter((candidate) => candidate.limitsColumns || candidate.popoverHeaders)) {
    assert.ok(names(layout).includes(scene.name), scene.name);
  }
});

test("without an argument every scene runs, and a scene file runs its own scenes", async (t) => {
  assert.deepEqual(names(await chosen([])), names(SCENES));
  assert.deepEqual(names(await chosen([sceneFile(t, [{ name: "one", url: "/limits?mock=ok" }])])), ["one"]);
});

test("a run with nothing to check, a malformed file, an unknown flag or an extra file is refused", async (t) => {
  const empty = sceneFile(t, []);
  const scene = { name: "one", url: "/limits?mock=ok" };
  await assert.rejects(chooseScenes([empty]), /no scenes/);
  await assert.rejects(chooseScenes([sceneFile(t, {})]), /array of scenes/);
  await assert.rejects(chooseScenes([sceneFile(t, [{ ...scene, steps: [{ click: "a", wait: "b" }] }])]), /exactly one of/);
  await assert.rejects(chooseScenes(["--layuot"]), /Unknown option/);
  await assert.rejects(chooseScenes(["--layout", empty]), /--layout/);
  await assert.rejects(chooseScenes([sceneFile(t, [scene]), sceneFile(t, [scene])]), /one scene file/);
});

test("a layout run needs both a Limits card scene and a popover scene", async () => {
  const limitsOnly = [{ name: "limits", url: "/limits", limitsColumns: 3 }];
  const popoverOnly = [{ name: "popover", url: "/?surface=limits-popover", popoverHeaders: true }];
  await assert.rejects(chooseScenes(["--layout"], popoverOnly), /Limits card scene/);
  await assert.rejects(chooseScenes(["--layout"], limitsOnly), /popover scene/);
});

test("--csp runs every built-in scene without the layout guards, and refuses a scene file or --layout beside it", async (t) => {
  const run = await chooseScenes(["--csp"]);
  assert.equal(run.csp, true);
  assert.equal((await chooseScenes([])).csp, false);
  assert.deepEqual(names(run.scenes), names(SCENES));
  for (const scene of run.scenes) {
    for (const guard of ["limitsColumns", "popoverHeaders", "checkMuseChart"]) assert.equal(scene[guard], undefined, `${scene.name} ${guard}`);
  }
  assert.ok(SCENES.some((scene) => scene.limitsColumns), "the stripped guards exist in the built-in scenes");
  await assert.rejects(chooseScenes(["--csp", sceneFile(t, [{ name: "one", url: "/limits?mock=ok" }])]), /takes no scene file/);
  await assert.rejects(chooseScenes(["--csp", "--layout"]), /separate runs/);
});
