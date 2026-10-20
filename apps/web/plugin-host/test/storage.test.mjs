import { test } from 'node:test';
import assert from 'node:assert/strict';
import { indexedDB } from 'fake-indexeddb';
import { openStorage, loadPluginStorage, flushPluginStorage } from '../src/storage-idb.mjs';

test('plugin storage survives a restart and stays separate per plugin', async () => {
  const db = await openStorage(indexedDB, 'test-' + Math.random());
  await flushPluginStorage(db, 'review', [['cards', new Uint8Array([1, 2, 3])]]);
  await flushPluginStorage(db, 'gamification', [['xp', new Uint8Array([9])]]);
  await flushPluginStorage(db, 'review', [['cards', undefined]]);
  const review = await loadPluginStorage(db, 'review');
  assert.equal(review.size, 0, 'a deleted key is gone');
  const game = await loadPluginStorage(db, 'gamification');
  assert.deepEqual([...game.keys()], ['xp']);
  assert.deepEqual([...game.get('xp')], [9]);
});
