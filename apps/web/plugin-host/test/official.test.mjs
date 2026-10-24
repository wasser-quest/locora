import { test } from 'node:test';
import assert from 'node:assert/strict';
import { host } from '../src/host-core.mjs';

// Every official plugin must load, identify itself and activate in the browser host.
for (const id of ['core', 'notify', 'review', 'tutorial', 'gamification', 'competition']) {
  test(`${id} activates in the browser host`, async () => {
    const module = await import(`../dist/${id}/${id}.js`);
    host.register(id, module);
    host.start(id);
    assert.equal(host.require(id).manifest.id, id);
    assert.equal(host.require(id).active, true);
  });
}
