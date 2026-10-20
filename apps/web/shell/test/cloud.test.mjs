// The cloud client in the browser: the core's cloud commands run with the page's cloud client
// (cloud.mjs) against a stub of the cloud server. The stub answers the requests the commands make and
// keeps the key bundle it is given.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { MemFs } from '../src/wasi-memfs.mjs';
import { instantiateCore } from '../src/core.mjs';
import { runCloud } from '../src/cloud.mjs';

let core;
let server;
let base;
let keys = null;
const seen = [];

before(async () => {
  const wasm = await readFile(new URL('../../../../target/wasm32-wasip1/release/locora_web_core.wasm', import.meta.url));
  core = await instantiateCore(wasm, { fs: new MemFs() });
  const opened = core('open', '/locora');
  assert.equal(opened.err, undefined, opened.err);
  server = createServer(stub);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  base = `http://127.0.0.1:${server.address().port}`;
});

after(() => server?.close());

function stub(request, response) {
  const chunks = [];
  request.on('data', (chunk) => chunks.push(chunk));
  request.on('end', () => {
    const path = request.url.split('?')[0];
    seen.push(`${request.method} ${path}`);
    const send = (status, json) => {
      response.writeHead(status, { 'content-type': 'application/json' });
      response.end(json === undefined ? '' : JSON.stringify(json));
    };
    if (request.method === 'POST' && path === '/v1/sessions') {
      return send(200, {
        token: 'session-token',
        account: { id: 'a1', email: 'learner@example.org', display_name: 'Learner', subscription: 'none', plan: null, period_end: null },
      });
    }
    if (request.method === 'GET' && path === '/v1/keys') return keys ? send(200, keys) : send(404, { error: 'no keys' });
    if (request.method === 'POST' && path === '/v1/keys') {
      keys = JSON.parse(Buffer.concat(chunks).toString());
      return send(200, {});
    }
    if (request.method === 'DELETE' && path === '/v1/sessions') return send(204);
    send(404, { error: 'no such route' });
  });
}

const run = (command, args = {}) => runCloud(core, () => {}, command, args);

test('there is no account before a sign-in', async () => {
  assert.deepEqual(await run('cloud_status'), { account: null, server: null, access: false });
});

test('a sign-in keeps the account, and the status shows it', async () => {
  const status = await run('cloud_connect', {
    server: base,
    email: 'learner@example.org',
    password: 'long enough password',
    register: false,
    guest: false,
  });
  assert.equal(status.account, 'learner@example.org');
  assert.equal(status.access, false, 'without a subscription there is no entitlement');
  assert.ok(seen.includes('POST /v1/sessions'));
  assert.equal((await run('cloud_status')).account, 'learner@example.org');
});

test('the encryption keys are set up once, and the status follows', async () => {
  assert.equal(await run('cloud_keys_status'), false);
  const created = await run('cloud_keys_setup', { passphrase: 'a long test passphrase' });
  assert.equal(created.stored, true);
  assert.equal(created.warning, null);
  assert.ok(created.recovery_key.length > 20);
  assert.equal(await run('cloud_keys_status'), true);
  await assert.rejects(run('cloud_keys_setup', { passphrase: 'a long test passphrase' }), /bereits eingerichtet/);
});

test('a sign-out forgets the account, even when the server cannot be told', async () => {
  await run('cloud_logout');
  assert.equal((await run('cloud_status')).account, null);
  assert.ok(seen.includes('DELETE /v1/sessions'));
});

test('a server that does not answer is reported as unreachable', async () => {
  await assert.rejects(
    run('cloud_connect', { server: 'http://127.0.0.1:1', email: 'a@example.org', password: 'long enough password', register: false }),
    /nicht erreichbar/,
  );
});

test('a sync without a valid subscription is refused before anything is sent', async () => {
  await run('cloud_connect', { server: base, email: 'learner@example.org', password: 'long enough password', register: false });
  const before = seen.length;
  await assert.rejects(run('cloud_sync', { passphrase: 'a long test passphrase' }), /Freischaltung/);
  assert.equal(seen.length, before, 'no request reached the server');
});
