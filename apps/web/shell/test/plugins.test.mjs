// The official plugins in the browser: the core runs with the real plugin components (transpiled to
// JavaScript), and the page forwards what the core and the plugins publish, as the page does.
import { test, before } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { MemFs } from '../src/wasi-memfs.mjs';
import { instantiateCore } from '../src/core.mjs';
import { host } from '../../plugin-host/src/host-core.mjs';

const PLUGINS = ['core', 'notify', 'review', 'tutorial', 'gamification'];
const unwrap = (result) => {
  if (result.tag === 'err') throw new Error(result.val);
  return result.val;
};

let core;
let tutorialPalace;

before(async () => {
  const wasm = await readFile(new URL('../../../../target/wasm32-wasip1/release/locora_web_core.wasm', import.meta.url));
  const raw = await instantiateCore(wasm, { fs: new MemFs(), bridge: host.asBridge() });
  // What the page does: forward the core's events to the plugins, and the plugins' calls to the core.
  core = (command, args) => {
    const reply = raw(command, args);
    for (const event of reply.events ?? []) host.publish(event.topic, new Uint8Array(event.payload));
    return reply;
  };
  host.provideHostService('core.palaces@1', (_plugin, request) => unwrap(raw.palaceService(request)));
  host.provideHostService('ui.highlight@1', (plugin, request) => unwrap(raw.highlight(plugin, request)));
  host.onPublish = (plugin, topic, payload) => raw.publish(plugin, topic, payload);
  for (const id of PLUGINS) {
    host.register(id, await import(`../../plugin-host/dist/${id}/${id}.js`));
    host.start(id);
  }
  assert.equal(core('open', '/locora').err, undefined);
  assert.equal(core('set_plugins', { plugins: PLUGINS.map((id) => host.require(id).manifest) }).err, undefined);
  tutorialPalace = core('tutorial', { lang: 'en' }).ok;
});

/** Answers the tutorial journey to its end: a rating for recall, the solution for guesses, a first option for quizzes. */
function completeJourney() {
  let question = core('journey_start', {
    start: { palace_id: tutorialPalace, due_only: false, reverse: false, numbers: false, shuffle: false, inputs: [], quiz_answers: null },
  }).ok;
  for (let i = 0; i < 12 && question?.type === 'question'; i++) {
    const revealed = core('journey_reveal', {}).ok;
    const reply =
      question.kind === 'recall' ? { type: 'recall', value: 3 }
      : question.kind === 'guess' ? { type: 'guess', value: revealed.solution ?? '' }
      : { type: 'quiz', value: (question.options ?? []).map((_, index) => index).slice(0, 1) };
    const answer = core('journey_answer', { reply });
    assert.equal(answer.err, undefined, answer.err);
    question = answer.ok.next ?? null;
  }
  return question;
}

test('the plugins that run here show their tiles on the home screen', () => {
  const home = core('home', {}).ok;
  const plugins = home.tiles.map((tile) => tile.plugin).sort();
  assert.ok(plugins.includes('notify'), JSON.stringify(plugins));
  assert.ok(plugins.includes('gamification'), JSON.stringify(plugins));
});

test('the app tour is shown by the tutorial plugin: each step is a highlight of the page', () => {
  const step = core('tour', { action: 'Start' }).ok;
  assert.equal(step.target, 'tour-home-media');
  assert.equal(step.step, 1);
  assert.equal(step.total, 9);
  assert.ok(step.title.length > 0 && step.text.length > 0);
});

test('answering the journey counts for review: the answers are counted and the cards are scheduled', () => {
  const before = core('settings', {}).ok.stats;
  const beforeHome = core('home', {}).ok;
  completeJourney();
  const after = core('settings', {}).ok.stats;
  const afterHome = core('home', {}).ok;
  assert.equal(after.answers - before.answers, 9, 'every question is counted');
  assert.ok(after.scheduled > before.scheduled, `scheduled ${before.scheduled} -> ${after.scheduled}`);
  assert.ok(afterHome.fresh < beforeHome.fresh, 'answered cards are no longer new');
});

test('finishing the journey grants XP, trophies and streak through the gamification plugin', () => {
  const status = core('gamification_status', {}).ok;
  assert.ok(status.xp > 0, JSON.stringify(status));
  assert.ok(status.daily_correct > 0);
  assert.ok(status.streak >= 1);
});

test('reminders are set through the notify plugin, and due messages are handed to the page once', () => {
  const set = core('settings_set', { change: { reminders: { enabled: true, start: '00:00', end: '23:59', every: 1 } } });
  assert.equal(set.err, undefined, set.err);
  assert.equal(set.ok.reminders.enabled, true);
  assert.ok(set.ok.reminders.times.length > 0);
  const due = core('notifications', {}).ok;
  assert.ok(due.length > 0, 'a reminder is due');
  for (const item of due) {
    assert.equal(core('notification_shown', { notification: item }).err, undefined);
  }
  assert.equal(core('notifications', {}).ok.length, 0, 'shown messages leave the queue');
});
