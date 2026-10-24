// The cloud client in the browser: the core's cloud commands run with the page's cloud client
// (cloud.mjs) against a stub of the cloud server. The stub answers the requests the commands make and
// keeps the key bundle it is given. Its friends routes can answer as for an account without access
// (402) or as an older server (404 „Nicht gefunden“, 405).
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { MemFs } from '../src/wasi-memfs.mjs';
import { instantiateCore } from '../src/core.mjs';
import { runCloud } from '../src/cloud.mjs';
import { coreWasm } from './core-wasm.mjs';

let core;
let server;
let base;
let keys = null;
const seen = [];
/** How the stub's friends routes answer, and what it saw of them. */
const social = { mode: 'ok', verified: null, me: true, headers: {}, bodies: {} };

before(async () => {
  const wasm = await coreWasm();
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
    const account = { id: 'a1', email: 'learner@example.org', display_name: 'Learner', subscription: 'none', plan: null, period_end: null, email_verified: social.verified };
    if (request.method === 'POST' && path === '/v1/sessions') return send(200, { token: 'session-token', account });
    if (request.method === 'GET' && path === '/v1/me') return send(200, { ...account, email_verified: social.me });
    // The friends routes; an invite code or a friendship's id is the last part of the path.
    if (path.startsWith('/v1/friends') || path.startsWith('/v1/ranking') || path.startsWith('/v1/reports')) {
      const route = `${request.method} ${path.replace(/^(\/v1\/friends\/invites\/)[^/]+/, '$1{code}').replace(/^(\/v1\/friends\/)(?!invites)[^/]+$/, '$1{id}')}`;
      social.headers[route] = request.headers;
      social.bodies[route] = Buffer.concat(chunks).toString();
      const code = decodeURIComponent(path.split('/')[4] ?? '');
      if (social.mode === 'older') return path === '/v1/friends/invites' ? send(405, { error: 'Diese Methode ist hier nicht erlaubt' }) : send(404, { error: 'Nicht gefunden' });
      const paid = !['GET /v1/friends/invites', 'GET /v1/friends/invites/{code}', 'DELETE /v1/friends/invites/{code}', 'DELETE /v1/friends/{id}', 'POST /v1/reports'].includes(route);
      if (social.mode === 'pay' && paid) return send(402, { error: 'Für diese Funktion ist ein aktives Abo nötig' });
      switch (route) {
        case 'GET /v1/friends':
          return send(200, [
            { display_name: 'Ada', accepted: true, id: 'f1' },
            { display_name: 'Ben', accepted: false, id: 'f2' },
            { display_name: '', accepted: false, id: 'f3', since: 1700000000 },
          ]);
        case 'GET /v1/friends/invites':
          return send(200, [{ code: 'ABCD-EFGH-JKLM-NPQR', url: 'https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR', expires_at: 4102444800 }]);
        case 'GET /v1/friends/invites/{code}':
          return code === 'ABCD-EFGH-JKMN-PQRS' ? send(200, { display_name: 'Ada', expires_at: 4102444800, ref: 'n1' }) : send(404, { error: 'Diese Einladung gibt es nicht (mehr): Sie ist abgelaufen, wurde schon angenommen oder zurückgezogen.' });
        case 'POST /v1/friends/invites/{code}/accept':
          return send(200, { display_name: 'Ada', accepted: true, id: 'f9' });
        case 'POST /v1/reports':
          return send(204);
        default:
          return send(404, { error: `not in the stub: ${route}` });
      }
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
  assert.deepEqual(await run('cloud_status'), {
    account: null,
    server: null,
    access: false,
    email_verified: null,
    auto_sync: false,
    last_sync_at: null,
    display_name: null,
  });
});

test('signed out, the friends commands answer without the network', async () => {
  const before = seen.length;
  assert.deepEqual(await run('cloud_friends'), { state: 'signed_out' });
  assert.deepEqual(await run('cloud_ranking', { friends: true }), { state: 'signed_out' });
  assert.deepEqual(await run('cloud_invite_preview', { code: 'ABCD-EFGH-JKMN-PQRS' }), { state: 'signed_out' });
  assert.equal(seen.length, before, 'nothing was sent');
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

test('the summary before a sync names the changes of this device; an older server tells nothing more', async () => {
  const pending = await run('cloud_pending');
  assert.equal(pending.remote, null, 'the stub has no pending count');
  assert.equal(typeof pending.local.total, 'number');
  assert.ok(seen.includes('GET /v1/sync/pending'));
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

test('the friends page arrives in one command with its two calls side by side', async () => {
  social.mode = 'ok';
  const page = await run('cloud_friends');
  assert.equal(page.state, 'ready');
  assert.deepEqual(page.value.friends, [{ id: 'f1', name: 'Ada' }]);
  assert.deepEqual(page.value.incoming, [{ id: 'f2', name: 'Ben' }]);
  assert.deepEqual(page.value.sent, [{ id: 'f3', since: 1700000000 }]);
  assert.equal(page.value.invites.state, 'ready');
  assert.equal(page.value.invites.value[0].url, 'https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR');
  assert.ok(seen.includes('GET /v1/friends') && seen.includes('GET /v1/friends/invites'));
  assert.equal((await run('cloud_status')).display_name, 'Learner');
});

test('an account without access and an older server are states, and their words stay on the server', async () => {
  social.mode = 'pay';
  const paid = await run('cloud_friends');
  assert.equal(paid.state, 'ready', 'the open links stay, to be revoked');
  assert.equal(paid.value.access, false);
  assert.deepEqual(paid.value.friends, []);
  assert.deepEqual(await run('cloud_ranking', { friends: false }), { state: 'not_for_account' });
  assert.deepEqual(await run('cloud_friend_ask', { email: 'ben@example.org' }), { state: 'not_for_account' });
  assert.doesNotMatch(JSON.stringify(paid), /Abo/);
  social.mode = 'older';
  assert.deepEqual(await run('cloud_friends'), { state: 'server_too_old' });
  assert.deepEqual(await run('cloud_invite_revoke', { code: 'ABCD-EFGH-JKLM-NPQR' }), { state: 'server_too_old' });
  social.mode = 'ok';
});

test('a pasted invite link is checked before the network, shown without the session and accepted with it', async () => {
  const before = seen.length;
  await assert.rejects(run('cloud_invite_preview', { code: 'hello' }), /kein Einladungscode/);
  assert.equal(seen.length, before, 'nothing was sent for junk');
  const preview = await run('cloud_invite_preview', { code: 'https://web.locora.cloud/einladung#abcd-efgh-jkmn-pqrs' });
  assert.deepEqual(preview, { state: 'ready', value: { code: 'ABCD-EFGH-JKMN-PQRS', name: 'Ada', expires_at: 4102444800, reference: 'n1' } });
  assert.equal(social.headers['GET /v1/friends/invites/{code}'].authorization, undefined);
  const accepted = await run('cloud_invite_accept', { code: preview.value.code });
  assert.deepEqual(accepted, { state: 'ready', value: { id: 'f9', name: 'Ada' } });
  assert.equal(social.headers['POST /v1/friends/invites/{code}/accept'].authorization, 'Bearer session-token');
  await assert.rejects(run('cloud_invite_preview', { code: 'ZZZZ-EFGH-JKMN-PQRS' }), /gibt es nicht/);
});

test('a report counts once the address is confirmed', async () => {
  const report = { reference: 'n1', reason: ' rude ' };
  assert.deepEqual(await run('cloud_report', report), { state: 'ready', value: { counts_now: true } });
  assert.deepEqual(JSON.parse(social.bodies['POST /v1/reports']), { ref: 'n1', reason: 'rude' });
  // An account whose address the stored account says is unconfirmed asks once more.
  await run('cloud_logout');
  social.verified = false;
  social.me = false;
  await run('cloud_connect', { server: base, email: 'learner@example.org', password: 'long enough password', register: false });
  assert.deepEqual(await run('cloud_report', { reference: 'n1', reason: null }), { state: 'ready', value: { counts_now: false } });
  social.me = true;
  assert.deepEqual(await run('cloud_report', { reference: 'n1', reason: null }), { state: 'ready', value: { counts_now: true } });
  assert.ok(seen.includes('GET /v1/me'));
});
