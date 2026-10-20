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
