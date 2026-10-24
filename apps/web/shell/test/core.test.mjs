import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MemFs } from '../src/wasi-memfs.mjs';
import { instantiateCore } from '../src/core.mjs';
import { host } from '../../plugin-host/src/host-core.mjs';
import { coreWasm } from './core-wasm.mjs';

const wasm = await coreWasm();

async function core(fs = new MemFs()) {
  return instantiateCore(wasm, { fs });
}

test('the browser core opens a profile in its folder and answers home', async () => {
  const call = await core();
  assert.deepEqual(call('open', '/locora'), { ok: null, events: [] });
  const home = call('home');
  assert.ok(home.ok, JSON.stringify(home));
  assert.deepEqual(home.ok.palaces, []);
  assert.equal(home.ok.welcome.kind, 'sply', 'an empty profile shows the welcome mascot');
});

test('a fresh core reports an unknown command and a missing profile as errors', async () => {
  const call = await core();
  assert.match(call('nope').err, /unknown command/);
  assert.match(call('home').err, /not open/);
});

test('the tutorial palace builds in the browser core, once per language', async () => {
  const call = await core();
  call('open', '/locora');
  const english = call('tutorial', { lang: 'en' });
  assert.ok(english.ok, JSON.stringify(english));
  assert.equal(call('palace', { id: english.ok }).ok.name, 'Learn Locora');
  assert.equal(call('tutorial', { lang: 'en' }).ok, english.ok, 'an existing palace is reused');
  const german = call('tutorial', { lang: 'de' });
  assert.ok(german.ok && german.ok !== english.ok, JSON.stringify(german));
});

test('a tutorial journey is graded by the core plugin in the JS host', async () => {
  if (!host.plugins.has('core')) {
    host.register('core', await import('../../plugin-host/dist/core/core.js'));
    host.start('core');
  }
  const bridged = await instantiateCore(wasm, { fs: new MemFs(), bridge: host.asBridge() });
  bridged('open', '/locora');
  const palaceId = bridged('tutorial', { lang: 'en' }).ok;
  const start = bridged('journey_start', {
    start: { palace_id: palaceId, due_only: false, reverse: false, numbers: false, shuffle: false, inputs: [], quiz_answers: null },
  });
  assert.equal(start.ok?.type, 'question', JSON.stringify(start));
  assert.ok(bridged('journey_reveal').ok, 'the answer can be revealed');
  const feedback = bridged('journey_answer', { reply: { type: 'recall', value: 3 } });
  assert.ok(feedback.ok, JSON.stringify(feedback));
  assert.equal(feedback.ok.correct, true, 'any recall rating above "again" counts as correct');
});

test('what the core writes survives a reload: a new file system with the saved changes', async () => {
  const saved = new Map();
  const first = new MemFs();
  const call = await core(first);
  call('open', '/locora');
  const palaceId = call('tutorial', { lang: 'en' }).ok;
  for (const [path, bytes] of first.takeChanges().writes) saved.set(path, bytes);
  assert.ok([...saved.keys()].some((path) => path.endsWith('.db')), 'the database is saved');

  const second = new MemFs();
  second.load([...saved]);
  const reopened = await core(second);
  reopened('open', '/locora');
  const palaces = reopened('home').ok.palaces.map((palace) => palace.id);
  assert.deepEqual(palaces, [palaceId], 'the palace is still there after a reload');
});
