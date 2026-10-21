// End-to-end tests of the browser build (apps/web/dist) in Chromium: the app as a user sees it.
// Run after `node apps/web/build.mjs`: `npm test` in apps/web/e2e.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { handler } from '../serve.mjs';

const root = new URL('../dist', import.meta.url).pathname;
let server;
let origin;
let browser;

before(async () => {
  // LOCORA_URL tests a running deployment (the app's container) instead of the local build.
  if (process.env.LOCORA_URL) {
    origin = process.env.LOCORA_URL.replace(/\/$/, '');
  } else {
    server = createServer(handler(root));
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    origin = `http://127.0.0.1:${server.address().port}`;
  }
  browser = await chromium.launch({ args: ['--no-sandbox'] });
});

after(async () => {
  await browser?.close();
  server?.close();
});

/** A fresh browser profile: its own IndexedDB, so each test starts with no palaces. */
async function openApp(options = {}) {
  const context = await browser.newContext({ viewport: options.viewport ?? { width: 1000, height: 760 }, acceptDownloads: true });
  // The language is chosen on the very first start; the tests speak English unless they ask for that choice.
  if (!options.chooseLanguage) await context.addInitScript(() => localStorage.setItem('locora-language-v1', 'en'));
  const page = await context.newPage();
  const problems = [];
  const requests = [];
  page.on('pageerror', (error) => problems.push(`page error: ${error.message}`));
  page.on('dialog', (dialog) => {
    problems.push(`dialog: ${dialog.message()}`);
    dialog.dismiss();
  });
  page.on('console', (message) => {
    const text = message.text();
    // A missing favicon is the browser asking for one, not an app error.
    if (message.type() === 'error' && !/status of 404/.test(text)) problems.push(`console: ${text.slice(0, 200)}`);
    if (/Content Security Policy/.test(text)) problems.push(`policy: ${text.slice(0, 200)}`);
  });
  page.on('request', (request) => requests.push(request.url()));
  await page.goto(origin + '/', { waitUntil: 'load' });
  if (!options.chooseLanguage) await page.getByRole('button', { name: 'Start the app tour' }).waitFor({ timeout: 15000 });
  return { context, page, problems, requests };
}

/** The welcome page starts the app tour, which sets up the tutorial palace: close the tour, open the palace. */
async function openTutorial(page) {
  await page.getByRole('button', { name: 'Start the app tour' }).click();
  await page.getByRole('region', { name: 'App tour' }).waitFor({ timeout: 15000 });
  await page.getByRole('button', { name: 'Close', exact: true }).click();
  await page.getByRole('button', { name: /^Learn Locora/ }).first().click();
  await page.getByRole('button', { name: /Walk the route/ }).waitFor({ timeout: 15000 });
}

test('the app starts with its welcome page, a live graphic and no errors', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.waitForTimeout(1500);
    const text = await page.locator('body').innerText();
    assert.match(text, /Welcome to Locora/);
    assert.equal(await page.locator('.sply-inline svg').count() >= 1, true, 'the mascot is a graphic in the page');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the tutorial journey starts, reveals and grades an answer', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('button', { name: /Walk the route/ }).click();
    await page.waitForFunction(() => document.body.innerText.includes('1/9'), null, { timeout: 15000 });
    // Space reveals the answer; 1–4 grade it (3 = good). The keys work in every language.
    await page.keyboard.press('Space');
    // The grade buttons are in the interface language: English here, no German left behind.
    const labels = await page.locator('.grades button').allInnerTexts();
    assert.match(labels.join(' '), /Again.*Hard.*Good.*Easy/s, labels.join(' | '));
    await page.keyboard.press('3');
    await page.waitForTimeout(1500);
    assert.match(await page.locator('body').innerText(), /2\/9/, 'the next question follows the grade');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('palaces and progress survive a reload: everything is kept on this device', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('button', { name: /Walk the route/ }).click();
    await page.waitForFunction(() => document.body.innerText.includes('1/9'), null, { timeout: 15000 });
    await page.keyboard.press('Space');
    await page.keyboard.press('3');
    await page.waitForTimeout(1500);
    await page.reload({ waitUntil: 'load' });
    await page.waitForFunction(() => document.body.innerText.includes('Learn Locora'), null, { timeout: 15000 });
    assert.match(await page.locator('body').innerText(), /Learn Locora/, 'the tutorial palace is still there');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('nothing leaves the device: every request stays on this site or in local blob documents', async () => {
  const { context, page, requests } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('button', { name: /Walk the route/ }).click();
    await page.waitForFunction(() => document.body.innerText.includes('1/9'), null, { timeout: 15000 });
    await page.keyboard.press('Space');
    await page.waitForTimeout(1500);
    const foreign = requests.filter((url) => !url.startsWith(origin) && !url.startsWith('blob:'));
    assert.deepEqual(foreign, []);
  } finally {
    await context.close();
  }
});

test('an animated graphic cannot reach the network: it carries no script and the page blocks the request', async () => {
  const { context, page } = await openApp();
  try {
    await page.waitForTimeout(2000);
    assert.ok((await page.locator('.sply-inline svg').count()) >= 1, 'a graphic is shown in the page');
    assert.equal(await page.locator('.sply-inline script').count(), 0, 'the graphic carries no script');
    const outcome = await page.evaluate(() =>
      fetch('http://127.0.0.1:9/probe').then(
        () => 'reached the network',
        () => 'blocked',
      ),
    );
    assert.equal(outcome, 'blocked');
  } finally {
    await context.close();
  }
});

test('a file name that looks like markup is shown as text and never runs', async () => {
  const { context, page, problems } = await openApp();
  try {
    // A 1x1 PNG, named like an attack. The name becomes the palace's name.
    const png = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
      'base64',
    );
    const name = '<img src=x onerror="window.__xss=1">.png';
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: /Own palace from image/ }).click(),
    ]);
    await chooser.setFiles({ name, mimeType: 'image/png', buffer: png });
    await page.waitForTimeout(2500);
    const text = await page.locator('body').innerText();
    // The name is cleaned on the way in (only letters, digits and . _ - survive) and shown as text.
    assert.match(text, /_img_src_x_onerror/, 'the name is shown as text');
    assert.equal(await page.locator('img[src="x"]').count(), 0, 'no image was injected');
    assert.equal(await page.evaluate(() => window.__xss), undefined, 'the name did not run');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('a palace gets more rooms from pictures, also after its first one', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    const tabs = page.locator('.room-tab');
    const before = await tabs.count();
    await page.getByRole('checkbox', { name: 'Edit' }).check();
    const png = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
      'base64',
    );
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: '+ Room from a picture or video' }).click(),
    ]);
    await chooser.setFiles({ name: 'Kitchen.png', mimeType: 'image/png', buffer: png });
    await page.getByText('Room added').waitFor({ timeout: 15000 });
    assert.equal(await tabs.count(), before + 1);
    // The new room is the last one and opens at once.
    await page.getByRole('button', { name: 'Kitchen', exact: true }).and(page.locator('.on')).waitFor();
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('a palace exports as a .locora file that the app saves to the device', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('button', { name: /Export…/ }).waitFor({ timeout: 15000 });
    const [file] = await Promise.all([
      page.waitForEvent('download'),
      page.getByRole('button', { name: /Export…/ }).click(),
    ]);
    assert.match(file.suggestedFilename(), /\.locora$/);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the app reloads and opens offline: the service worker keeps the app and the palaces', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.waitForFunction(() => document.body.innerText.includes('Learn Locora'), null, { timeout: 15000 });
    // The worker must control the page before the network goes away.
    await page.waitForFunction(() => !!navigator.serviceWorker.controller, null, { timeout: 20000 });
    await context.setOffline(true);
    await page.reload({ waitUntil: 'load' });
    await page.waitForFunction(() => document.body.innerText.includes('Learn Locora'), null, { timeout: 20000 });
    // A new page in the same browser profile opens offline too.
    const second = await context.newPage();
    second.on('pageerror', (error) => problems.push(`offline page error: ${error.message}`));
    await second.goto(origin + '/', { waitUntil: 'load' });
    await second.waitForFunction(() => document.body.innerText.includes('Learn Locora'), null, { timeout: 20000 });
    await context.setOffline(false);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the app can be installed: manifest, icons and the offline worker are in place', async () => {
  const { context, page } = await openApp();
  try {
    const manifest = await page.evaluate(async () => (await fetch('manifest.webmanifest')).json());
    assert.equal(manifest.name, 'Locora');
    assert.equal(manifest.display, 'standalone');
    const sizes = manifest.icons.map((icon) => `${icon.sizes}:${icon.purpose}`);
    assert.ok(sizes.includes('192x192:any') && sizes.includes('512x512:maskable'), sizes.join(', '));
    for (const icon of manifest.icons) {
      assert.equal(await page.evaluate(async (src) => (await fetch(src)).ok, icon.src), true, icon.src);
    }
    assert.equal(await page.locator('link[rel="manifest"]').getAttribute('href'), 'manifest.webmanifest');
    await page.waitForFunction(() => navigator.serviceWorker.getRegistration().then((r) => !!r), null, { timeout: 20000 });
  } finally {
    await context.close();
  }
});

test('the palace exports as a ZIP of its source and media (the folder export)', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('button', { name: /Export TOML/ }).waitFor({ timeout: 15000 });
    const [file] = await Promise.all([
      page.waitForEvent('download'),
      page.getByRole('button', { name: /Export TOML/ }).click(),
    ]);
    assert.match(file.suggestedFilename(), /\.zip$/);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the app tour is shown: the tutorial plugin highlights the controls step by step', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.getByRole('button', { name: 'App tour', exact: true }).click();
    await page.waitForFunction(() => document.body.innerText.includes('Step 1 of 9'), null, { timeout: 15000 });
    await page.locator('.tour-panel').getByRole('button', { name: 'Next' }).click();
    await page.waitForFunction(() => document.body.innerText.includes('Step 2 of 9'), null, { timeout: 15000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the plugins show their tiles on the start page (notify and gamification run in the browser)', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.waitForTimeout(1500);
    const text = await page.locator('body').innerText();
    assert.match(text, /Gamification|Erfolge|Achievements/i, 'the gamification tile or its section is shown');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('reminders reach the browser notifications once, and the page does not show them twice', async () => {
  const { context, page, problems } = await openApp();
  try {
    // The browser API is recorded here; the real notification is the browser's own.
    await page.addInitScript(() => {
      window.__notified = [];
      const record = (title, options) => window.__notified.push({ title, body: options?.body ?? '' });
      Notification.permission = 'granted';
      Notification.requestPermission = () => Promise.resolve('granted');
      window.Notification = function Notification(title, options) { record(title, options); };
      window.Notification.permission = 'granted';
      window.Notification.requestPermission = () => Promise.resolve('granted');
      // Frames of the graphics have no service worker: only the page's own registrations are patched.
      if (window.ServiceWorkerRegistration) {
        ServiceWorkerRegistration.prototype.showNotification = function (title, options) {
          record(title, options);
          return Promise.resolve();
        };
      }
    });
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'Start the app tour' }).waitFor({ timeout: 15000 });
    await page.waitForFunction(() => window.__TAURI__, null, { timeout: 15000 });
    await page.evaluate(() => window.__TAURI__.core.invoke('settings_set', {
      change: { reminders: { enabled: true, start: '00:00', end: '23:59', every: 1 } },
    }));
    await page.evaluate(() => window.__TAURI__.core.invoke('notifications', {}));
    const shown = await page.evaluate(() => window.__notified.length);
    assert.ok(shown > 0, 'a due reminder was shown');
    await page.evaluate(() => window.__TAURI__.core.invoke('notifications', {}));
    assert.equal(await page.evaluate(() => window.__notified.length), shown, 'shown reminders leave the queue');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the layout fits the screen: a phone folds the modes, the desktop shows them, and nothing scrolls sideways', async () => {
  const phone = await openApp({ viewport: { width: 390, height: 844 } });
  try {
    await openTutorial(phone.page);
    await phone.page.waitForTimeout(1200);
    assert.equal(await phone.page.locator('details.modes').evaluate((el) => el.open), false, 'folded on a phone');
    assert.equal(await phone.page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 0);
  } finally {
    await phone.context.close();
  }
  const desktop = await openApp({ viewport: { width: 1440, height: 900 } });
  try {
    await openTutorial(desktop.page);
    await desktop.page.waitForTimeout(1200);
    assert.equal(await desktop.page.locator('details.modes').evaluate((el) => el.open), true, 'open on a desktop');
    assert.equal(await desktop.page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 0);
  } finally {
    await desktop.context.close();
  }
});

test('a locora.toml imports with its pictures, chosen together, and becomes a palace', async () => {
  const { context, page, problems } = await openApp();
  try {
    const png = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
      'base64',
    );
    const toml = 'name = "Folder import"\n[[room]]\nname = "Room"\nimage = "room.png"\nitems = ["One", "Two"]\n';
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: 'Import…' }).click(),
    ]);
    await chooser.setFiles([
      { name: 'locora.toml', mimeType: 'text/plain', buffer: Buffer.from(toml) },
      { name: 'room.png', mimeType: 'image/png', buffer: png },
    ]);
    await page.waitForFunction(() => document.body.innerText.includes('Folder import'), null, { timeout: 20000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

/** A tenth of a second of silence as WAV (8 kHz, 8 bit, mono), with a title the app removes. */
function tinyWav() {
  const samples = 800;
  const title = Buffer.from('LIST\x0c\0\0\0INFOINAM\0\0\0\0', 'latin1');
  const wav = Buffer.alloc(44 + samples, 0x80);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(36 + samples + title.length, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(8000, 24);
  wav.writeUInt32LE(8000, 28);
  wav.writeUInt16LE(1, 32);
  wav.writeUInt16LE(8, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(samples, 40);
  return Buffer.concat([wav, title]);
}

test('a locus gets a sound clip in the editor and shows a player for it', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('checkbox', { name: 'Edit' }).check();
    await page.locator('.marker button').first().click();
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: 'Choose audio…' }).click(),
    ]);
    await chooser.setFiles({ name: 'bell.wav', mimeType: 'audio/wav', buffer: tinyWav() });
    const player = page.locator('.locus-audio audio');
    await player.waitFor({ state: 'attached', timeout: 20000 });
    assert.match(await player.getAttribute('src'), /^blob:/);
    await page.getByRole('button', { name: 'Remove audio' }).waitFor({ timeout: 5000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the first start asks for the language and then speaks it', async () => {
  const { context, page, problems } = await openApp({ chooseLanguage: true });
  try {
    await page.getByRole('dialog').waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'Deutsch' }).click();
    await page.getByRole('button', { name: 'App-Tour starten' }).waitFor({ timeout: 15000 });
    assert.equal(await page.getByRole('dialog').count(), 0, 'the choice is gone');
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'App-Tour starten' }).waitFor({ timeout: 15000 });
    assert.equal(await page.getByRole('dialog').count(), 0, 'the choice is remembered');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});
