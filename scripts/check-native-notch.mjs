// Native protocol/lifetime checks. No provider reads or live settings writes.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { once } from 'node:events';

const executable = resolve(process.argv[2] ?? 'src-tauri/target/debug/on-n-off-notch');
const snapshot = { version: 1, sequence: 1, snapshot: { settings: { enabled: false, edge: 'right' }, displays: [], error: null }, providers: [] };
async function check(name, input, expectedAck, args = []) {
  const child = spawn(executable, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
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
await check('unsupported protocol rejected', JSON.stringify({ ...snapshot, version: 2 }) + '\n', false);
await check('oversize frame rejected', ' '.repeat(262145), false);
await check('parent closes before first snapshot', '', false);
await check('parent EOF exits with the main queue unresponsive', '', false, ['--check-unresponsive-main']);
await check('parent EOF exits while a snapshot awaits the main queue', JSON.stringify(snapshot) + '\n', false, ['--check-unresponsive-main']);
