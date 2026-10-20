import { test } from 'node:test';
import assert from 'node:assert/strict';
import { host } from '../src/host-core.mjs';

test('the review plugin loads, identifies itself and activates', async () => {
  const module = await import('../dist/review/review.js');
  host.register('review', module);
  host.start('review');
  const plugin = host.require('review');
  assert.equal(plugin.manifest.id, 'review');
  assert.equal(plugin.active, true);
  assert.ok(plugin.subscriptions.size > 0, 'review subscribes to journey events');
});
