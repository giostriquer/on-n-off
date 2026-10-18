import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { chooseScenes, SCENES } from "./ui-scenes.mjs";

const names = (scenes) => scenes.map((scene) => scene.name);

test("--layout runs exactly the Limits card and popover header scenes", async () => {
  const layout = await chooseScenes(["--layout"]);
  assert.deepEqual(names(layout), [
    "limits-sign-in-960",
    "limits-sign-in-1024",
    "limits-sign-in-1120",
    "limits-sign-in-1440",
    "limits-popover",
    "popover-archived",
  ]);
  for (const scene of SCENES.filter((candidate) => candidate.limitsColumns || candidate.popoverHeaders)) {
    assert.ok(names(layout).includes(scene.name), scene.name);
  }
});

test("without an argument every scene runs, and a scene file runs its own scenes", async () => {
  assert.deepEqual(names(await chooseScenes([])), names(SCENES));
  const file = join(mkdtempSync(join(tmpdir(), "ui-scenes-")), "scenes.json");
  writeFileSync(file, JSON.stringify([{ name: "one", url: "/limits?mock=ok" }]));
  assert.deepEqual(names(await chooseScenes([file])), ["one"]);
});

test("a run with nothing to check, an unknown flag or a file beside --layout is refused", async () => {
  const empty = join(mkdtempSync(join(tmpdir(), "ui-scenes-")), "empty.json");
  writeFileSync(empty, "[]");
  await assert.rejects(chooseScenes([empty]), /no scenes/);
  await assert.rejects(chooseScenes(["--layuot"]), /Unknown option/);
  await assert.rejects(chooseScenes(["--layout", empty]), /--layout/);
});

test("a layout run needs both a Limits card scene and a popover scene", async () => {
  const limitsOnly = [{ name: "limits", url: "/limits", limitsColumns: 3 }];
  const popoverOnly = [{ name: "popover", url: "/?surface=limits-popover", popoverHeaders: true }];
  await assert.rejects(chooseScenes(["--layout"], popoverOnly), /Limits card scene/);
  await assert.rejects(chooseScenes(["--layout"], limitsOnly), /popover scene/);
});
