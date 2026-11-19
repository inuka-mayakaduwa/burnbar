'use strict';
const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { uninstall } = require('../unix/install.cjs');
const { parseArgs } = require('../bin/burnbar.cjs');

const touch = (p, body = 'x') => { fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, body); };
let canLink = true;
const link = (target, p) => { try { fs.symlinkSync(target, p); } catch (e) { if (e.code !== 'EPERM') throw e; canLink = false; } };
const home = () => fs.mkdtempSync(path.join(os.tmpdir(), 'bb-home-'));
const quiet = (fn) => { const log = console.log; console.log = () => {}; try { return fn(); } finally { console.log = log; } };

function seedLinux(h) {
  const dir = path.join(h, '.local', 'share', 'BurnBar');
  touch(path.join(dir, 'BurnBar.AppImage'));
  touch(path.join(dir, '.version'));
  touch(path.join(dir, 'burnbar.png'));
  fs.mkdirSync(path.join(h, '.local', 'bin'), { recursive: true });
  link(path.join(dir, 'BurnBar.AppImage'), path.join(h, '.local', 'bin', 'burnbar'));
  touch(path.join(h, '.local', 'share', 'applications', 'burnbar.desktop'));
  touch(path.join(h, '.config', 'autostart', 'BurnBar.desktop'));
  touch(path.join(h, '.config', 'BurnBar', 'settings.json'));
  return dir;
}

test('linux uninstall removes app, keeps settings by default', () => {
  const h = home();
  const dir = seedLinux(h);
  quiet(() => uninstall({}, { platform: 'linux', arch: 'x64', home: h, env: {} }));
  assert.ok(!fs.existsSync(dir));
  assert.ok(!fs.existsSync(path.join(h, '.local', 'share', 'applications', 'burnbar.desktop')));
  assert.ok(!fs.existsSync(path.join(h, '.config', 'autostart', 'BurnBar.desktop')));
  assert.ok(fs.existsSync(path.join(h, '.config', 'BurnBar', 'settings.json')));
  let present = true; try { fs.lstatSync(path.join(h, '.local', 'bin', 'burnbar')); } catch { present = false; }
  assert.ok(!present);
});

test('linux uninstall --purge removes settings; dry-run changes nothing', () => {
  const h = home();
  const dir = seedLinux(h);
  quiet(() => uninstall({ dryRun: true, purge: true }, { platform: 'linux', arch: 'x64', home: h, env: {} }));
  assert.ok(fs.existsSync(path.join(dir, 'BurnBar.AppImage')));
  assert.ok(fs.existsSync(path.join(h, '.config', 'BurnBar')));
  quiet(() => uninstall({ purge: true }, { platform: 'linux', arch: 'x64', home: h, env: {} }));
  assert.ok(!fs.existsSync(path.join(h, '.config', 'BurnBar')));
});

test('linux uninstall leaves unrelated files and foreign symlink alone', () => {
  const h = home();
  const dir = seedLinux(h);
  touch(path.join(dir, 'mine.txt'));
  fs.rmSync(path.join(h, '.local', 'bin', 'burnbar'), { force: true });
  link('/usr/bin/true', path.join(h, '.local', 'bin', 'burnbar'));
  quiet(() => uninstall({}, { platform: 'linux', arch: 'x64', home: h, env: {} }));
  assert.ok(fs.existsSync(path.join(dir, 'mine.txt')));
  if (canLink) assert.ok(fs.lstatSync(path.join(h, '.local', 'bin', 'burnbar')).isSymbolicLink());
});

test('macos uninstall removes app and launch agent, purge removes settings', () => {
  const h = home();
  touch(path.join(h, 'Applications', 'BurnBar.app', 'Contents', 'Info.plist'));
  touch(path.join(h, 'Library', 'LaunchAgents', 'BurnBar.plist'));
  touch(path.join(h, 'Library', 'Application Support', 'BurnBar', 'settings.json'));
  quiet(() => uninstall({}, { platform: 'darwin', arch: 'arm64', home: h, env: {} }));
  assert.ok(!fs.existsSync(path.join(h, 'Applications', 'BurnBar.app')));
  assert.ok(!fs.existsSync(path.join(h, 'Library', 'LaunchAgents', 'BurnBar.plist')));
  assert.ok(fs.existsSync(path.join(h, 'Library', 'Application Support', 'BurnBar')));
  quiet(() => uninstall({ purge: true }, { platform: 'darwin', arch: 'arm64', home: h, env: {} }));
  assert.ok(!fs.existsSync(path.join(h, 'Library', 'Application Support', 'BurnBar')));
});

test('uninstall with nothing installed succeeds', () => {
  assert.strictEqual(quiet(() => uninstall({}, { platform: 'darwin', arch: 'x64', home: home(), env: {} })), 0);
});

test('argument parsing', () => {
  assert.strictEqual(parseArgs(['uninstall', '--purge']).purge, true);
  assert.throws(() => parseArgs(['install', '--purge']));
});
