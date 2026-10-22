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
  // `phone` is a touch screen: the commands then sit in a bar at the bottom.
  const phone = options.phone ? { hasTouch: true, isMobile: true } : {};
  const context = await browser.newContext({ viewport: options.viewport ?? { width: 1000, height: 760 }, acceptDownloads: true, ...phone });
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
    await page.waitForFunction(() => document.body.innerText.includes('Step 1 of 13'), null, { timeout: 15000 });
    await page.locator('.tour-panel').getByRole('button', { name: 'Next' }).click();
    await page.waitForFunction(() => document.body.innerText.includes('Step 2 of 13'), null, { timeout: 15000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the app tour shows Simo building an example palace and walking its route', async () => {
  const { context, page, problems } = await openApp();
  try {
    const panel = page.locator('.tour-panel');
    const step = (n) => page.waitForFunction((n) => document.body.innerText.includes(`Step ${n} of 13`), n, { timeout: 15000 });
    await page.getByRole('button', { name: 'App tour', exact: true }).click();
    await step(1);
    // The page stays usable during the tour: the dimming lets clicks through.
    assert.equal(await page.locator('.tour-spot, .tour-scrim').evaluateAll((nodes) => nodes.every((n) => getComputedStyle(n).pointerEvents === 'none')), true);
    for (let n = 2; n <= 7; n += 1) {
      await panel.getByRole('button', { name: 'Next' }).click();
      await step(n);
    }
    // Step 7: Simo has placed locus 1 on the door and typed its place, item and memory aid.
    await page.waitForFunction(() => [...document.querySelectorAll('.editor-form input, .editor-form textarea')].some((field) => field.value === 'A giant golden key is stuck crosswise in the door.'), null, { timeout: 15000 });
    const values = await page.locator('.editor-form input[type="text"]').evaluateAll((fields) => fields.map((field) => field.value));
    assert.ok(values.includes('Door') && values.includes('Key'), values.join(' | '));
    for (let n = 8; n <= 12; n += 1) {
      await panel.getByRole('button', { name: 'Next' }).click();
      await step(n);
    }
    // Step 12: the route of the example, with the answer revealed and the grades highlighted.
    await page.locator('#tour-journey-grade[data-tour-active]').waitFor({ timeout: 15000 });
    await panel.getByRole('button', { name: 'Next' }).click();
    await step(13);
    await panel.getByRole('button', { name: 'Done' }).click();
    await page.getByRole('button', { name: /^Simo's example/ }).first().waitFor({ timeout: 15000 });
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

/**
 * Records about a second of WebM in the page with MediaRecorder, as a browser on a phone or laptop
 * does: a video (VP9 or VP8) of a changing canvas, or sound (Opus) of a tone. Returns its bytes.
 */
async function recordWebm(page, kind) {
  const base64 = await page.evaluate(async (kind) => {
    let stream;
    let stop = () => {};
    if (kind === 'video') {
      const canvas = document.createElement('canvas');
      canvas.width = 160;
      canvas.height = 120;
      const context = canvas.getContext('2d');
      let frame = 0;
      const draw = setInterval(() => {
        context.fillStyle = `hsl(${(frame++ * 30) % 360} 80% 50%)`;
        context.fillRect(0, 0, 160, 120);
      }, 40);
      stream = canvas.captureStream(25);
      stop = () => clearInterval(draw);
    } else {
      const audio = new AudioContext();
      const tone = audio.createOscillator();
      const output = audio.createMediaStreamDestination();
      tone.connect(output);
      tone.start();
      stream = output.stream;
      stop = () => audio.close();
    }
    const types = kind === 'video' ? ['video/webm;codecs=vp9', 'video/webm;codecs=vp8'] : ['audio/webm;codecs=opus'];
    const recorder = new MediaRecorder(stream, { mimeType: types.find((type) => MediaRecorder.isTypeSupported(type)) });
    const chunks = [];
    recorder.ondataavailable = (event) => chunks.push(event.data);
    const stopped = new Promise((resolve) => (recorder.onstop = resolve));
    recorder.start(200);
    await new Promise((resolve) => setTimeout(resolve, 1200));
    recorder.stop();
    await stopped;
    stop();
    const bytes = new Uint8Array(await new Blob(chunks).arrayBuffer());
    let text = '';
    for (const byte of bytes) text += String.fromCharCode(byte);
    return btoa(text);
  }, kind);
  return Buffer.from(base64, 'base64');
}

/** Remembers every conversion notice the page shows from now on (see shell/src/video.mjs). */
async function watchConversions(page) {
  await page.evaluate(() => {
    window.__conversions = [];
    new MutationObserver(() => {
      const text = document.body.innerText;
      if (/umgewandelt|converted/i.test(text)) window.__conversions.push(text.slice(0, 200));
    }).observe(document.body, { childList: true, subtree: true, characterData: true });
  });
}

/** The first bytes and size of what a player plays, as hex. */
function playedBytes(page, selector) {
  return page.evaluate(async (selector) => {
    const player = [...document.querySelectorAll(selector)].find((element) => element.readyState >= 1);
    const blob = await (await fetch(player.src)).blob();
    const head = new Uint8Array(await blob.slice(0, 4).arrayBuffer());
    return { head: [...head].map((byte) => byte.toString(16).padStart(2, '0')).join(''), size: blob.size };
  }, selector);
}

test('a browser recording (WebM) becomes a video palace as it is, without conversion, and plays', async () => {
  const { context, page, problems } = await openApp();
  try {
    const webm = await recordWebm(page, 'video');
    assert.equal(webm.subarray(0, 4).toString('hex'), '1a45dfa3', 'MediaRecorder wrote WebM');
    await watchConversions(page);
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: /Own palace from image/ }).click(),
    ]);
    await chooser.setFiles({ name: 'Recording.webm', mimeType: 'video/webm', buffer: webm });
    await page.waitForFunction(() => [...document.querySelectorAll('video')].some((video) => video.readyState >= 1), null, {
      timeout: 30000,
    });
    const stored = await playedBytes(page, 'video');
    assert.equal(stored.head, '1a45dfa3', 'stored as WebM, not converted to MP4');
    assert.equal(stored.size, webm.length, 'not re-encoded: metadata is voided in place');
    assert.deepEqual(await page.evaluate(() => window.__conversions), [], 'no conversion notice');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('a recorded WebM sound clip is kept as it is and plays at its locus', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    const weba = await recordWebm(page, 'audio');
    await watchConversions(page);
    await page.getByRole('checkbox', { name: 'Edit' }).check();
    await page.locator('.marker button').first().click();
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: 'Choose audio…' }).click(),
    ]);
    await chooser.setFiles({ name: 'voice.weba', mimeType: 'audio/webm', buffer: weba });
    await page.waitForFunction(() => [...document.querySelectorAll('.locus-audio audio')].some((audio) => audio.readyState >= 1), null, {
      timeout: 20000,
    });
    const stored = await playedBytes(page, '.locus-audio audio');
    assert.equal(stored.head, '1a45dfa3');
    assert.equal(stored.size, weba.length);
    assert.deepEqual(await page.evaluate(() => window.__conversions), []);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('an ⓘ button explains an editor field and hides the explanation again', async () => {
  const { context, page, problems } = await openApp();
  try {
    await openTutorial(page);
    await page.getByRole('checkbox', { name: 'Edit' }).check();
    await page.locator('.marker button').first().click();
    // Found by its label: the ⓘ inside must not take the label over from the field.
    const field = page.locator('label', { has: page.getByRole('textbox', { name: /^Memory aid/ }) });
    const info = field.getByRole('button', { name: 'Explanation' });
    await info.click();
    const box = field.getByRole('note');
    await box.waitFor({ timeout: 5000 });
    assert.match(await box.innerText(), /crazy picture/);
    assert.equal(await info.getAttribute('aria-expanded'), 'true');
    assert.notEqual(await page.evaluate(() => document.activeElement?.tagName), 'TEXTAREA', 'the field keeps out of it');
    await info.click();
    await box.waitFor({ state: 'detached', timeout: 5000 });
    assert.equal(await info.getAttribute('aria-expanded'), 'false');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('an ⓘ button in a settings checkbox explains it without toggling it', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.getByRole('button', { name: 'Settings & statistics' }).click();
    const checkbox = page.getByRole('checkbox', { name: /^Hide the current place after revealing/ });
    const before = await checkbox.isChecked();
    const field = page.locator('label', { has: checkbox });
    const info = field.getByRole('button', { name: 'Explanation' });
    await info.click();
    const box = field.getByRole('note');
    await box.waitFor({ timeout: 5000 });
    assert.match(await box.innerText(), /marker disappears/);
    // Tapping the explanation itself must not toggle the checkbox either.
    await box.click();
    assert.equal(await checkbox.isChecked(), before, 'the checkbox is untouched');
    // Escape on the ⓘ closes the explanation too.
    await info.focus();
    await page.keyboard.press('Escape');
    await box.waitFor({ state: 'detached', timeout: 5000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('on a phone the tour hides the commands bar and moves up while a field is typed in', async () => {
  const { context, page, problems } = await openApp({ phone: true, viewport: { width: 390, height: 760 } });
  try {
    const commands = page.getByRole('button', { name: 'Open the command palette' });
    const panel = page.locator('.tour-panel');
    const step = (n) => page.waitForFunction((n) => document.body.innerText.includes(`Step ${n} of 13`), n, { timeout: 15000 });
    await commands.waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'Start the app tour' }).click();
    await step(1);
    assert.equal(await commands.count(), 0, 'the bar makes room for the tour');
    for (let n = 2; n <= 7; n += 1) {
      await panel.getByRole('button', { name: 'Next' }).click();
      await step(n);
    }
    // The keyboard would cover the lower half: with a field focused the panel sits at the top.
    await page.locator('.editor-form textarea').first().focus();
    await page.waitForFunction(() => document.querySelector('.tour-panel').getBoundingClientRect().top < 200, null, { timeout: 5000 });
    await panel.getByRole('button', { name: 'Close', exact: true }).click();
    await commands.waitFor({ timeout: 15000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the first start shows only the language choice, not a tour left from earlier', async () => {
  const { context, page, problems } = await openApp({ chooseLanguage: true });
  try {
    await page.getByRole('button', { name: 'English' }).click();
    // A tour stopped halfway, then the language is forgotten, as after a reinstall that kept the
    // app's data (Android restores it from the backup).
    await page.getByRole('button', { name: 'Start the app tour' }).click();
    await page.getByRole('region', { name: 'App tour' }).waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'Next', exact: true }).click();
    await page.waitForFunction(() => document.body.innerText.includes('Step 2 of 13'), null, { timeout: 15000 });
    await page.evaluate(() => localStorage.removeItem('locora-language-v1'));
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('dialog').waitFor({ timeout: 15000 });
    assert.equal(await page.getByRole('region', { name: 'App tour' }).count(), 0, 'no tour behind the choice');
    await page.getByRole('button', { name: 'English' }).click();
    await page.getByRole('button', { name: 'App tour', exact: true }).waitFor({ timeout: 15000 });
    await page.waitForTimeout(1000);
    assert.equal(await page.getByRole('region', { name: 'App tour' }).count(), 0, 'the old tour does not resume');
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'App tour', exact: true }).waitFor({ timeout: 15000 });
    await page.waitForTimeout(1000);
    assert.equal(await page.getByRole('region', { name: 'App tour' }).count(), 0, 'and stays closed');
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
