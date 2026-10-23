import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PluginHost } from '../src/host-core.mjs';

// The permission rule, checked without a plugin: the bindings refuse storage to a plugin without it.
test('storage needs the storage permission', () => {
  const host = new PluginHost();
  host.register('fake', { manifest: () => ({ id: 'fake', permissions: [] }), activate: () => ({ tag: 'ok' }), onEvent: () => ({ tag: 'ok' }) });
  host.require('fake').manifest = { id: 'fake', permissions: [] };
  const bound = host.bindingsFor('fake');
  assert.equal(bound.kvSet('k', new Uint8Array([1])).tag, 'err');
  host.require('fake').manifest = { id: 'fake', permissions: ['storage'] };
  assert.equal(bound.kvSet('k', new Uint8Array([1])).tag, 'ok');
  assert.deepEqual([...host.takeDirty('fake')[0][1]], [1]);
});

const listRequest = (prefix, afterRev) => {
  const name = new TextEncoder().encode(prefix);
  return new Uint8Array([name.length, ...name, afterRev]);
};

// storage.list@1 as the native host answers it: only the caller's keys, after a revision, in order.
test('storage.list@1 lists the own changed keys after a revision', () => {
  const host = new PluginHost();
  const manifest = { id: 'game', permissions: ['storage'] };
  host.register('game', { manifest: () => manifest }, new Map([['old', new Uint8Array([9])]]));
  host.require('game').manifest = manifest;
  host.register('other', { manifest: () => ({ id: 'other', permissions: ['storage'] }) });
  host.require('other').manifest = { id: 'other', permissions: ['storage'] };
  const game = host.bindingsFor('game');
  host.bindingsFor('other').kvSet('g/x', new Uint8Array([1]));
  game.kvSet('g/a', new Uint8Array([1]));
  game.kvSet('g/a', new Uint8Array([1])); // the same bytes: no change
  game.kvSet('g/b', new Uint8Array([]));
  const all = decodeListResponse(game.callHost('storage.list@1', listRequest('', 0)).val);
  assert.deepEqual(all.entries.map(([key]) => key), ['old', 'g/a', 'g/b']);
  assert.equal(all.more, false);
  const after = decodeListResponse(game.callHost('storage.list@1', listRequest('g/', 2)).val);
  assert.deepEqual(after.entries, [['g/b', []]], 'a blanked key is listed with no bytes');
  assert.equal(after.rev, 3);
  host.require('other').manifest = { id: 'other', permissions: [] };
  assert.equal(host.bindingsFor('other').callHost('storage.list@1', listRequest('', 0)).tag, 'err');
  // A plugin cannot take the service over.
  game.provide('storage.list@1');
  assert.equal(host.services.get('storage.list@1'), undefined);
});

/** The reply's postcard, read back for the test (one-byte lengths are enough here). */
function decodeListResponse(bytes) {
  let at = 0;
  const count = bytes[at++];
  const entries = [];
  for (let i = 0; i < count; i++) {
    const keyLength = bytes[at++];
    const key = new TextDecoder().decode(bytes.subarray(at, at + keyLength));
    at += keyLength;
    const valueLength = bytes[at++];
    entries.push([key, [...bytes.subarray(at, at + valueLength)]]);
    at += valueLength;
  }
  return { entries, rev: bytes[at], more: bytes[at + 1] === 1 };
}
