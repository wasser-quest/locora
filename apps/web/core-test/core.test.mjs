import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadCore } from './core.mjs';

const wasm = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'target', 'wasm32-wasip1', 'release', 'locora_web_core.wasm');

test('the browser core opens a profile in its folder and answers home', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'locora-core-'));
  const call = await loadCore(wasm, dir);
  assert.deepEqual(call('open', '/locora'), { ok: null });
  const home = call('home');
  assert.ok(home.ok, JSON.stringify(home));
  assert.deepEqual(home.ok.palaces, []);
  assert.equal(home.ok.welcome.kind, 'sply', 'an empty profile shows the welcome mascot');
});

test('a fresh core reports an unknown command and a missing profile as errors', async () => {
  const call = await loadCore(wasm, mkdtempSync(join(tmpdir(), 'locora-core-')));
  assert.match(call('nope').err, /unknown command/);
  assert.match(call('home').err, /not open/);
});

test('the tutorial palace builds in the browser core, once per language', async () => {
  const call = await loadCore(wasm, mkdtempSync(join(tmpdir(), 'locora-core-')));
  call('open', '/locora');
  const english = call('tutorial', 'en');
  assert.ok(english.ok, JSON.stringify(english));
  assert.equal(call('palace', english.ok).ok.name, 'Learn Locora');
  assert.equal(call('tutorial', 'en').ok, english.ok, 'an existing palace is reused');
  const german = call('tutorial', 'de');
  assert.ok(german.ok && german.ok !== english.ok, JSON.stringify(german));
});

// Grading runs the core plugin's Evaluate service in the JS plugin host, reached through the bridge.
test('a tutorial journey runs in the browser core: start, reveal, grade', async () => {
  const { host } = await import('../plugin-host/src/host-core.mjs');
  if (!host.plugins.has('core')) {
    host.register('core', await import('../plugin-host/dist/core/core.js'));
    host.start('core');
  }
  const call = await loadCore(wasm, mkdtempSync(join(tmpdir(), 'locora-core-')), host.asBridge());
  call('open', '/locora');
  const palaceId = call('tutorial', 'en').ok;
  const start = call('journey_start', {
    palace_id: palaceId, due_only: false, reverse: false, numbers: false, shuffle: false, inputs: [], quiz_answers: null,
  });
  assert.equal(start.ok?.type, 'question', JSON.stringify(start));
  assert.ok(call('journey_reveal').ok, 'the answer can be revealed');
  const feedback = call('journey_answer', { type: 'recall', value: 3 });
  assert.ok(feedback.ok, JSON.stringify(feedback));
  assert.equal(feedback.ok.correct, true, 'any recall rating above "again" counts as correct');
});
