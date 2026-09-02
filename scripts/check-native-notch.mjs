// Native protocol/lifetime checks plus an offscreen render of the rail, pill, and popovers.
// No provider reads or live settings writes.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, statSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { once } from 'node:events';

const executable = resolve(process.argv[2] ?? 'src-tauri/target/debug/on-n-off-notch');
const settings = { enabled: false, displayId: null, edge: 'right', size: 'standard', show: 'always', providers: ['claude', 'codex', 'antigravity', 'cursor'], pullRequests: { enabled: true, lists: ['mine'] } };
const snapshot = { version: 2, sequence: 1, snapshot: { settings, displays: [], error: null }, providers: [] };
async function check(name, input, expectedAck, args = []) {
  const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  // Generous: the check is that the helper exits on EOF, not how fast a busy machine starts it.
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
await check('oversize frame rejected', ' '.repeat(262145), false);
await check('parent closes before first snapshot', '', false);
await check('parent EOF exits with the main queue unresponsive', '', false, ['--check-unresponsive-main']);
await check('parent EOF exits while a snapshot awaits the main queue', JSON.stringify(snapshot) + '\n', false, ['--check-unresponsive-main']);

// `--render <message.json> <out-dir>` draws every surface from a fixture; look at the PNGs when
// the rail or popover changes. NOTCH_RENDER_DIR keeps them somewhere else than .tmp/notch-render.
const now = Date.now();
const at = offsetMs => new Date(now + offsetMs).toISOString();
const quota = (id, label, kind, usedPercent, resetHours) => ({ id, label, kind, usedPercent, resetsAt: at(resetHours * 3_600_000), observedAt: at(0) });
const session = (id, name, place, project, status, minutesAgo) => ({ id, name, place, project, status, lastActiveAt: at(-minutesAgo * 60_000) });
const pull = (number, title, ci, reviewDecision, mergeKind, isDraft) => ({
  id: `node-${number}`, number, title, url: `https://github.com/octo/tools/pull/${number}`, repo: 'octo/tools',
  author: 'octocat', isDraft, reviewDecision, ci, mergeKind, updatedAt: '2026-09-01T10:00:00Z',
});
const fixture = edge => ({
  version: 2, sequence: 1,
  snapshot: {
    settings: { enabled: true, displayId: 'fixture', edge, size: 'standard', show: 'onHover', providers: ['claude', 'codex', 'antigravity', 'cursor'], pullRequests: { enabled: true, lists: ['mine', 'reviewRequested'] } },
    displays: [{ id: 'fixture', name: 'Fixture', x: 0, y: 0, width: 1920, height: 1080, workY: 25, workHeight: 1055, scale: 2, mirrored: false }],
    error: null,
  },
  providers: [
    { provider: 'claude', status: 'ok', currentAccount: true, plan: 'max', windows: [quota('weekly_all', 'Weekly · all models', 'weekly', 7, 100), quota('session', '5 hour · all models', 'session', 32, 3), quota('weekly_scoped:Fable', 'Weekly · Fable', 'model', 13, 100)], sessions: [session('a', 'repo-28', 'Desktop', 'repo', 'idle', 0), session('b', 'tool-d2', 'Terminal', 'tool', 'working', 2)] },
    { provider: 'codex', status: 'ok', currentAccount: true, plan: 'pro', windows: [quota('weekly', 'Weekly', 'weekly', 49, 140), quota('session', '5 hour', 'session', 91, 4)], sessions: [session('c', 'tool-42', 'Desktop', 'tool', 'working', 0)] },
    { provider: 'antigravity', status: 'unsupported', currentAccount: true, message: 'Antigravity has no subscription limits to show.', windows: [], sessions: [] },
    { provider: 'cursor', status: 'unsupported', currentAccount: true, message: 'Cursor has no subscription limits to show.', windows: [], sessions: [] },
  ],
  pullRequests: {
    status: 'ok', hint: null, stale: false,
    lists: [
      { id: 'mine', total: 3, items: [
        pull(41, 'ci: give push runs on dev and main one concurrency group per commit', 'success', 'APPROVED', 'ready', false),
        pull(40, 'feat: pull requests in the side notch', 'pending', null, null, true),
        pull(39, 'release: corrected usage cost estimates', 'failure', 'CHANGES_REQUESTED', 'conflicts', false),
      ] },
      { id: 'reviewRequested', total: 1, items: [pull(98, 'Drive Spotify RLE desktop wiring', 'success', null, 'behind', false)] },
    ],
  },
  actionError: null,
});
const renderRoot = resolve(process.env.NOTCH_RENDER_DIR ?? '.tmp/notch-render');
for (const edge of ['right', 'top']) {
  const dir = resolve(renderRoot, edge);
  mkdirSync(dir, { recursive: true });
  const message = resolve(dir, 'message.json');
  writeFileSync(message, JSON.stringify(fixture(edge)));
  const result = spawnSync(executable, ['--render', message, dir], { encoding: 'utf8', timeout: 20000 });
  assert.equal(result.status, 0, `render ${edge}: ${result.stderr}`);
  for (const name of ['rail', 'rail-cap-hovered', 'pill', 'popover-claude', 'popover-codex', 'popover-antigravity', 'popover-cursor', 'popover-pull-requests']) {
    const size = statSync(resolve(dir, `${name}.png`)).size;
    // The pill is a plain capsule; every other surface carries text and glyphs.
    assert.ok(size > (name === 'pill' ? 100 : 2000), `render ${edge}: ${name}.png is empty (${size} bytes)`);
  }
  console.log(`PASS render ${edge} → ${dir}`);
}
