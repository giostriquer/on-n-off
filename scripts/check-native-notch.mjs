import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { once } from 'node:events';
import { inflateSync } from 'node:zlib';

const executable = resolve(process.argv[2] ?? 'src-tauri/target/debug/on-n-off-notch');
const protocolVersion = Number(/PROTOCOL_VERSION: u64 = (\d+);/.exec(readFileSync(resolve('src-tauri/src/side_notch/protocol.rs'), 'utf8'))[1]);

const infoPlist = resolve('src-tauri/macos/SideNotch/Info.plist');
const embedded = spawnSync('/usr/bin/segedit', [executable, '-extract', '__TEXT', '__info_plist', '/dev/stdout'], { maxBuffer: 1 << 20 });
assert.equal(embedded.status, 0, `segedit could not read the helper's embedded Info.plist: ${embedded.stderr}`);
assert.ok(embedded.stdout.equals(readFileSync(infoPlist)), `${executable} embeds another Info.plist than ${infoPlist}: the helper was not relinked`);
console.log('PASS helper embeds the current Info.plist');
const settings = { enabled: false, displayId: null, edge: 'right', size: 'standard', show: 'always', providers: ['claude', 'codex', 'antigravity', 'cursor', 'muse'], pullRequests: { enabled: true, lists: ['mine'] }, popoverStyle: 'glass' };
const snapshot = { version: protocolVersion, sequence: 1, snapshot: { settings, displays: [], error: null }, providers: [] };
async function check(name, input, expectedAck, args = []) {
  const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  const timer = setTimeout(() => child.kill('SIGKILL'), 20000);
  const messages = [];
  let diagnostics = '';
  child.stderr.on('data', chunk => { diagnostics += chunk; });
  const lines = createInterface({ input: child.stdout });
  const exited = once(child, 'exit');
  child.stdin.on('error', () => {});
  lines.on('line', line => {
    const message = JSON.parse(line);
    messages.push(message);
    if (message.type === 'ready') {
      child.stdin.write(input);
      if (!expectedAck) child.stdin.end();
    }
    if (message.type === 'ack') child.stdin.end();
  });
  try {
    const [code, signal] = await exited;
    assert.equal(signal, null, `${name}: helper must exit on EOF without a kill`);
    assert.equal(code, 0, `${name}: unexpected exit ${code}: ${diagnostics}`);
    assert.ok(messages.some(m => m.type === 'ready'));
    assert.equal(messages.filter(m => m.type === 'ack').length, expectedAck ? 1 : 0);
    if (expectedAck) assert.equal(messages.find(m => m.type === 'ack').sequence, 1);
    console.log(`PASS ${name}`);
  } finally { clearTimeout(timer); child.kill(); }
}
await check('disabled snapshot acknowledged, parent EOF exits', JSON.stringify(snapshot) + '\n', true);
await check('unsupported protocol rejected', JSON.stringify({ ...snapshot, version: 1 }) + '\n', false);
await check('previous protocol rejected', JSON.stringify({ ...snapshot, version: protocolVersion - 1 }) + '\n', false);
await check('oversize frame rejected', ' '.repeat(262145), false);
await check('parent closes before first snapshot', '', false);
await check('parent EOF exits with the main queue unresponsive', '', false, ['--check-unresponsive-main']);
await check('parent EOF exits while a snapshot awaits the main queue', JSON.stringify(snapshot) + '\n', false, ['--check-unresponsive-main']);

const now = Date.now();
const at = offsetMs => new Date(now + offsetMs).toISOString();
const quota = (id, label, kind, usedPercent, resetHours) => ({ id, label, kind, usedPercent, resetsAt: at(resetHours * 3_600_000), observedAt: at(0) });
const session = (id, name, place, project, status, minutesAgo) => ({ id, name, place, project, status, lastActiveAt: at(-minutesAgo * 60_000) });
const pull = (number, title, ci, reviewDecision, mergeKind, isDraft, context = 'work') => {
  const repo = context === 'personal' ? 'octocat/dotfiles' : 'acme/webapp';
  return {
    id: `node-${number}`, number, title, url: `https://github.com/${repo}/pull/${number}`, repo,
    author: 'octocat', isDraft, reviewDecision, ci, mergeKind, updatedAt: '2026-09-01T10:00:00Z', context,
  };
};
const fixture = edge => ({
  version: protocolVersion, sequence: 1,
  snapshot: {
    settings: { enabled: true, displayId: 'fixture', edge, size: 'standard', show: 'onHover', providers: ['claude', 'codex', 'antigravity', 'cursor', 'muse'], pullRequests: { enabled: true, lists: ['mine', 'reviewRequested'] }, popoverStyle: 'glass' },
    displays: [{ id: 'fixture', name: 'Fixture', x: 0, y: 0, width: 1920, height: 1080, workY: 25, workHeight: 1055, scale: 2, mirrored: false }],
    error: null,
  },
  providers: [
    { provider: 'claude', status: 'ok', currentAccount: true, windows: [quota('weekly_all', 'Weekly · all models', 'weekly', 7, 100), quota('session', '5 hour · all models', 'session', 32, 3), quota('weekly_scoped:Fable', 'Weekly · Fable', 'model', 13, 100)], headlineWindowId: 'weekly_all', innerRing: { kind: 'fable', windowId: 'weekly_scoped:Fable' }, sessions: [session('a', 'repo-28', 'Desktop', 'repo', 'idle', 0), session('b', 'tool-d2', 'Terminal', 'tool', 'working', 2)] },
    { provider: 'codex', status: 'ok', currentAccount: true, windows: [quota('primary', 'Weekly · all models', 'weekly', 49, 140)], headlineWindowId: 'primary', innerRing: { kind: 'workspaceShare' }, workspaceCredits: { usedPercent: 32, resetsAt: at(7 * 86_400_000), left: '17,000 of 25,000 left', renewed: '25,000 of 25,000 left' }, sessions: [session('c', 'tool-42', 'Desktop', 'tool', 'working', 0)] },
    { provider: 'antigravity', status: 'unsupported', currentAccount: true, message: 'Antigravity has no subscription limits to show.', windows: [], sessions: [] },
    { provider: 'muse', status: 'ok', currentAccount: true, windows: [quota('weekly', 'Weekly · all models', 'weekly', 22, 100), quota('session', '5 hour · all models', 'session', 41, 3)], headlineWindowId: 'weekly', sessions: [session('acme-session', 'webapp-ab', 'Terminal', 'webapp', 'working', 0)] },
    { provider: 'cursor', status: 'unsupported', currentAccount: true, message: 'Cursor has no subscription limits to show.', windows: [], sessions: [] },
  ],
  pullRequests: {
    status: 'ok', hint: null, stale: false,
    lists: [
      { id: 'mine', total: 4, items: [
        pull(42, 'fix: passing checks with merge conflicts', 'success', 'APPROVED', 'conflicts', false),
        pull(41, 'ci: give push runs on dev and main one concurrency group per commit', 'success', 'APPROVED', 'ready', false),
        pull(40, 'feat: pull requests in the side notch', 'pending', null, null, true, 'personal'),
        pull(39, 'release: corrected usage cost estimates', 'failure', 'CHANGES_REQUESTED', 'conflicts', false),
      ] },
      { id: 'reviewRequested', total: 1, items: [pull(98, 'Drive Spotify RLE desktop wiring', 'success', null, 'behind', false)] },
    ],
  },
  actionError: null,
});
function readPng(path) {
  const data = readFileSync(path);
  const chunks = [];
  let width;
  let height;
  for (let at = 8; at < data.length;) {
    const length = data.readUInt32BE(at);
    const type = data.toString('latin1', at + 4, at + 8);
    const body = data.subarray(at + 8, at + 8 + length);
    if (type === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      assert.deepEqual([...body.subarray(8, 13)], [8, 6, 0, 0, 0], `${path} is not 8-bit RGBA without interlacing`);
    } else if (type === 'IDAT') chunks.push(body);
    at += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(chunks));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    assert.ok(filter <= 4, `${path} uses an unknown PNG filter ${filter}`);
    for (let x = 0; x < stride; x++) {
      const a = x >= 4 ? pixels[y * stride + x - 4] : 0;
      const b = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const c = x >= 4 && y > 0 ? pixels[(y - 1) * stride + x - 4] : 0;
      const p = a + b - c;
      const paeth = Math.abs(p - a) <= Math.abs(p - b) && Math.abs(p - a) <= Math.abs(p - c) ? a : Math.abs(p - b) <= Math.abs(p - c) ? b : c;
      pixels[y * stride + x] = (raw[y * (stride + 1) + 1 + x] + [0, a, b, (a + b) >> 1, paeth][filter]) & 0xff;
    }
  }
  return { width, height, at: (x, y) => [...pixels.subarray((y * width + x) * 4, (y * width + x) * 4 + 4)] };
}
const railInk = [8, 8, 8, 255];
const popoverInk = [14, 14, 17, 255];
const popoverBorder = [40, 40, 43, 255];
const contextInk = { work: [96, 165, 250, 255], personal: [192, 132, 252, 255] };
const oppositeEdge = { right: 'left', left: 'right', top: 'bottom', bottom: 'top' };
const near = (pixel, ink) => pixel.length === 4 && pixel.every((value, i) => Math.abs(value - ink[i]) <= 3);
const edgeLine = (png, edge, inset) => {
  const vertical = edge === 'right' || edge === 'left';
  const line = { right: png.width - 1 - inset, left: inset, top: inset, bottom: png.height - 1 - inset }[edge];
  return Array.from({ length: vertical ? png.height : png.width }, (_, i) => png.at(vertical ? line : i, vertical ? i : line));
};
const railEdgeInk = (png, edge) => {
  const pixels = edgeLine(png, edge, 0);
  const middle = pixels.slice(Math.floor(pixels.length * 0.2), Math.floor(pixels.length * 0.8));
  return middle.filter((pixel) => near(pixel, railInk)).length / middle.length;
};
const awayFromTail = (png, edge, inset) => {
  const line = edgeLine(png, oppositeEdge[edge], inset);
  return line[Math.floor(line.length / 2)];
};
const glassHost = Number(spawnSync('/usr/bin/sw_vers', ['-productVersion'], { encoding: 'utf8' }).stdout.split('.')[0]) >= 26;
const renderRoot = resolve(process.env.NOTCH_RENDER_DIR ?? '.tmp/notch-render');
const popoverSizes = {};
for (const edge of ['right', 'left', 'top', 'bottom']) {
  const dir = resolve(renderRoot, edge);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const message = resolve(dir, 'message.json');
  writeFileSync(message, JSON.stringify(fixture(edge)));
  const result = spawnSync(executable, ['--render', message, dir], { encoding: 'utf8', timeout: 20000 });
  assert.equal(result.status, 0, `render ${edge}: ${result.stderr}`);
  for (const name of ['rail', 'rail-cap-hovered', 'rail-styled', 'pill', 'popover-claude', 'popover-claude-styled', 'popover-codex', 'popover-antigravity', 'popover-cursor', 'popover-muse', 'popover-pull-requests']) {
    const size = statSync(resolve(dir, `${name}.png`)).size;
    assert.ok(size > (name === 'pill' ? 100 : 2000), `render ${edge}: ${name}.png is empty (${size} bytes)`);
  }
  const { scale } = fixture(edge).snapshot.displays[0];
  assert.ok(!readFileSync(resolve(dir, 'rail.png')).equals(readFileSync(resolve(dir, 'rail-cap-hovered.png'))), `render ${edge}: rail-cap-hovered.png shows no hovered cap`);
  for (const name of ['rail', 'rail-cap-hovered']) {
    const ink = railEdgeInk(readPng(resolve(dir, `${name}.png`)), edge);
    assert.equal(ink, 1, `render ${edge}: ${name}.png is ${Math.round(ink * 100)}% solid rail ink along its screen edge`);
  }
  if (glassHost) {
    assert.ok(railEdgeInk(readPng(resolve(dir, 'rail-styled.png')), edge) < 0.5, `render ${edge}: rail-styled.png is solid rail ink, so the Liquid Glass rail is not drawn`);
    const card = awayFromTail(readPng(resolve(dir, 'popover-claude-styled.png')), edge, 6 * scale);
    assert.ok(!near(card, popoverInk), `render ${edge}: popover-claude-styled.png card is solid popover ink, so the Liquid Glass popover is not drawn`);
  }
  for (const name of ['popover-claude', 'popover-pull-requests']) {
    const png = readPng(resolve(dir, `${name}.png`));
    const pixel = awayFromTail(png, edge, 6 * scale);
    assert.ok(near(pixel, popoverInk), `render ${edge}: ${name}.png card is rgba(${pixel}), not solid popover ink`);
    for (let inset = 0; inset <= scale; inset++) {
      const rim = awayFromTail(png, edge, inset);
      assert.ok(near(rim, inset < scale ? popoverBorder : popoverInk), `render ${edge}: ${name}.png is rgba(${rim}) ${inset} px in from its far edge, not a 1 pt border then card ink`);
    }
    const line = edgeLine(png, edge, 4 * scale - 1);
    const opaque = line.flatMap((pixel, i) => (pixel[3] > 0 ? [i] : []));
    const [first, last] = [opaque[0], opaque.at(-1)];
    assert.ok(opaque.length >= 5 * scale && opaque.length <= 9 * scale && last - first + 1 === opaque.length, `render ${edge}: ${name}.png has ${opaque.length} opaque px 4 pt in from its tail side, not one tail`);
    assert.ok(Math.abs((first + last) / 2 - line.length / 2) <= 2, `render ${edge}: ${name}.png tail is centred at ${(first + last) / 2} px, not mid-popover at ${line.length / 2}`);
    (popoverSizes[name] ??= {})[edge] = [png.width, png.height];
  }
  const pulls = readPng(resolve(dir, 'popover-pull-requests.png'));
  for (const [context, ink] of Object.entries(contextInk)) {
    const run = Math.max(...Array.from({ length: pulls.width }, (_, x) =>
      Array.from({ length: pulls.height }, (_, y) => pulls.at(x, y)).filter((pixel) => near(pixel, ink)).length));
    assert.ok(run >= 20 * scale, `render ${edge}: popover-pull-requests.png has at most ${run} px of ${context} ink in a column, so no ${context} row carries its stripe`);
  }
  console.log(`PASS render ${edge} → ${dir}`);
}
const tailPx = 8 * fixture('right').snapshot.displays[0].scale;
for (const [name, size] of Object.entries(popoverSizes)) {
  assert.deepEqual(size.left, size.right, `${name}: the left and right popovers differ in size`);
  assert.deepEqual(size.bottom, size.top, `${name}: the top and bottom popovers differ in size`);
  assert.deepEqual([size.right[0] - size.top[0], size.top[1] - size.right[1]], [tailPx, tailPx], `${name}: a popover is not its card plus one 8 pt tail (right ${size.right}, top ${size.top})`);
}
console.log('PASS popovers are one card plus a tail on every edge');
