// Sign a disposable app with SwiftPM's flat resource layout; never reads provider data.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

const config = JSON.parse(readFileSync('src-tauri/tauri.macos.conf.json', 'utf8'));
const resource = Object.entries(config.bundle.macOS.files).find(([, source]) =>
  source === 'binaries/BrowserBilling_BrowserBilling.bundle');
assert.ok(resource, 'billing resources must be packaged');
const root = mkdtempSync(join(tmpdir(), 'on-n-off-resource-check-'));
try {
  const app = join(root, 'Fixture.app');
  const contents = join(app, 'Contents');
  const binary = join(contents, 'MacOS', 'fixture');
  mkdirSync(dirname(binary), { recursive: true });
  copyFileSync('/usr/bin/true', binary);
  chmodSync(binary, 0o755);
  writeFileSync(join(contents, 'Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>app.on-n-off.resource-check</string>
<key>CFBundleExecutable</key><string>fixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>`);
  const bundle = join(contents, resource[0]);
  mkdirSync(bundle, { recursive: true });
  writeFileSync(join(bundle, 'billing.js'), '// resource fixture\n');
  for (const args of [['--force', '--deep', '--sign', '-', app], ['--verify', '--deep', '--strict', app]]) {
    const result = spawnSync('/usr/bin/codesign', args, { encoding: 'utf8' });
    assert.equal(result.status, 0, `billing resource bundle must allow app signing: ${result.stderr}`);
  }
  console.log('PASS flat SwiftPM billing resources allow strict app signing');
} finally {
  rmSync(root, { recursive: true, force: true });
}
