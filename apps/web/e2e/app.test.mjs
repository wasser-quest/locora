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
    // A missing favicon is the browser asking for one, not an app error. A test context is a
    // private window, whose small in-memory cache can refuse the core's module: the app then
    // fetches it again without the cache (boot.mjs), so the refusal is no app error either.
    if (message.type() === 'error' && !/status of 404|ERR_CACHE_WRITE_FAILURE/.test(text)) problems.push(`console: ${text.slice(0, 200)}`);
    if (/Content Security Policy/.test(text)) problems.push(`policy: ${text.slice(0, 200)}`);
  });
  page.on('request', (request) => requests.push(request.url()));
  await page.goto(origin + '/', { waitUntil: 'load' });
  if (!options.chooseLanguage) await page.getByRole('button', { name: 'Start the app tour' }).waitFor({ timeout: 15000 });
  return { context, page, problems, requests };
}

/**
 * Starts the app tour with `button`. While the lessons of „Learn mnemonics“ are open, the app asks
 * first; this answers „start the tour anyway“.
 */
async function startTour(page, button = page.getByRole('button', { name: 'Start the app tour' })) {
  await button.click();
  const ask = page.getByRole('dialog', { name: 'Have you practised the memory techniques yet?' });
  await ask.waitFor({ timeout: 15000 });
  await ask.getByRole('button', { name: 'I know this already – start the tour' }).click();
  await page.getByRole('region', { name: 'App tour' }).waitFor({ timeout: 15000 });
}

/** The welcome page starts the app tour, which sets up the tutorial palace: close the tour, open the palace. */
async function openTutorial(page) {
  await startTour(page);
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
      page.getByRole('button', { name: 'Choose image/video…' }).click(),
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
    await startTour(page, page.getByRole('button', { name: 'App tour', exact: true }));
    await page.waitForFunction(() => document.body.innerText.includes('Step 1 of 13'), null, { timeout: 15000 });
    await page.locator('.tour-panel').getByRole('button', { name: 'Next' }).click();
    await page.waitForFunction(() => document.body.innerText.includes('Step 2 of 13'), null, { timeout: 15000 });
    // Closing the tour early does not unlock „Tour completed“.
    await page.locator('.tour-panel').getByRole('button', { name: 'Close', exact: true }).click();
    await page.getByRole('region', { name: 'App tour' }).waitFor({ state: 'detached', timeout: 15000 });
    assert.equal(await page.locator('.celebration').count(), 0, 'nothing to celebrate');
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'Learn mnemonics', exact: true }).click();
    await page.locator('.medal[data-achievement="tour-done"].locked').waitFor({ timeout: 15000 });
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
    await startTour(page, page.getByRole('button', { name: 'App tour', exact: true }));
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
    // „Done“ on the last step completes the tour: its achievement is unlocked and celebrated.
    await page.locator('.celebration', { hasText: 'Tour completed' }).waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'Learn mnemonics', exact: true }).click();
    await page.locator('.medal[data-achievement="tour-done"]:not(.locked)').waitFor({ timeout: 15000 });
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
      page.getByRole('button', { name: 'Choose image/video…' }).click(),
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
    await startTour(page);
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
    await startTour(page);
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

/**
 * A Locora Cloud in the test: the page's requests to the cloud's address are answered here, so the
 * sync runs end to end in the browser (keys, Argon2 in its worker, sealing) without a server.
 */
async function fakeCloud(page) {
  const cloud = { keys: null, ops: [], media: new Set(), verified: false, resent: 0, pushes: 0, log: [] };
  const account = () => ({
    id: 'account-1', email: 'learner@example.org', display_name: 'Learner',
    subscription: 'active', plan: 'trial', period_end: 4102444800, email_verified: cloud.verified,
  });
  const retention = { state: 'active', last_successful_sync_at: 0, expires_at: 4102444800, generation: 0 };
  await page.route('https://api.locora.cloud/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const json = (status, body) => route.fulfill({ status, contentType: 'application/json', body: body === undefined ? '' : JSON.stringify(body) });
    const key = `${request.method()} ${url.pathname}`;
    cloud.log.push(key);
    if (key === 'POST /v1/accounts') return json(200, { token: 'session', account: account() });
    if (key === 'GET /v1/me') return json(200, account());
    if (key === 'POST /v1/email/resend') {
      cloud.resent += 1;
      return json(204);
    }
    if (key === 'GET /v1/keys') return cloud.keys ? json(200, cloud.keys) : json(404, { error: 'none' });
    if (key === 'POST /v1/keys') {
      cloud.keys = request.postDataJSON();
      return json(200, {});
    }
    if (key === 'GET /v1/sync/retention' || key === 'POST /v1/sync/complete') return json(200, retention);
    if (key === 'GET /v1/sync/pending') return json(200, { count: 5 });
    if (key === 'POST /v1/media/missing') {
      return json(200, { missing: request.postDataJSON().ids.filter((id) => !cloud.media.has(id)) });
    }
    if (request.method() === 'PUT' && url.pathname.startsWith('/v1/media/')) {
      cloud.media.add(url.pathname.split('/').pop());
      return json(200, {});
    }
    if (key === 'POST /v1/sync/ops') {
      cloud.pushes += 1;
      cloud.ops.push(...request.postDataJSON().ops);
      return json(200, { stored: 0 });
    }
    if (key === 'GET /v1/sync/ops') return json(200, { ops: [], cursor: Number(url.searchParams.get('after')) });
    return json(404, { error: `not in the test cloud: ${key}` });
  });
  return cloud;
}

test('a cloud sync shows what it sends and asks before a palace deletion reaches the other devices', async () => {
  const { context, page, problems } = await openApp();
  try {
    const cloud = await fakeCloud(page);
    // The tour sets up the tutorial palace; the start page then shows it.
    await startTour(page);
    await page.getByRole('button', { name: 'Close', exact: true }).click();
    await page.getByRole('button', { name: /^Learn Locora/ }).first().waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'Settings & statistics' }).click();
    const panel = page.getByRole('region', { name: 'Locora Cloud' });
    await panel.getByLabel('Email').fill('learner@example.org');
    await panel.getByRole('checkbox', { name: /^Create a new account/ }).check();
    await panel.getByLabel(/^Display name/).fill('Learner');
    await panel.getByLabel(/^Cloud password/).fill('a long cloud password');
    await panel.getByLabel(/^Repeat cloud password/).fill('a long cloud password');
    await panel.getByRole('button', { name: 'Create account' }).click();
    // A new account confirms its address; the link can be sent again.
    await panel.getByText(/Please confirm your e-mail address: we sent you a link at learner@example.org/).waitFor({ timeout: 15000 });
    await panel.getByRole('button', { name: 'Send the link again' }).click();
    await panel.getByText('We sent the link again.').waitFor({ timeout: 5000 });
    assert.equal(cloud.resent, 1);
    cloud.verified = true;
    await panel.getByRole('button', { name: 'I have confirmed – check' }).click();
    await panel.getByText(/Please confirm your e-mail address/).waitFor({ state: 'detached', timeout: 5000 });

    await panel.getByLabel(/^New encryption password/).fill('a long encryption passphrase');
    await panel.getByLabel(/^Repeat encryption password/).fill('a long encryption passphrase');
    await panel.getByRole('button', { name: 'Set up encryption' }).click();
    await panel.getByRole('button', { name: 'I have saved the key' }).click({ timeout: 30000 });

    // The first sync names the new palace, and what waits from other devices.
    await panel.getByLabel(/^Encryption password/).fill('a long encryption passphrase');
    await panel.getByRole('button', { name: 'Sync now' }).click();
    await panel.getByText('This device sends:').waitFor({ timeout: 15000 });
    assert.match(await panel.innerText(), /“Learn Locora” – new/);
    assert.match(await panel.innerText(), /5 changes from your other devices are waiting in the cloud\./);
    await panel.getByText('Sync finished.').waitFor({ timeout: 30000 });
    assert.ok(cloud.ops.length > 0, 'the palace reached the cloud');
    assert.ok(cloud.media.size > 0, 'with its pictures');
    assert.match(await panel.innerText(), /Last synced: /);

    // Deleted on this device: nothing is sent before the person agrees.
    await page.evaluate(async () => {
      const home = await window.__TAURI__.core.invoke('home');
      const tutorial = home.palaces.find((card) => card.name === 'Learn Locora');
      await window.__TAURI__.core.invoke('palace_delete', { palaceId: tutorial.id });
    });
    const pushes = cloud.pushes;
    await panel.getByLabel(/^Encryption password/).fill('a long encryption passphrase');
    await panel.getByRole('button', { name: 'Sync now' }).click();
    await panel.getByText('Palace “Learn Locora” will also be deleted in the cloud and on your other devices.').waitFor({ timeout: 15000 });
    assert.match(await panel.innerText(), /“Learn Locora” – deleted/);
    await page.waitForTimeout(500);
    assert.equal(cloud.pushes, pushes, 'nothing was sent yet');
    await panel.getByRole('button', { name: 'Confirm deletion and sync' }).click();
    await panel.getByText('Sync finished.').waitFor({ timeout: 30000 });
    assert.ok(cloud.pushes > pushes, 'the confirmed deletion was sent');

    // Auto-sync keeps the key in this browser and then syncs by itself.
    const checks = () => cloud.log.filter((entry) => entry === 'GET /v1/sync/retention').length;
    const before = checks();
    await panel.getByLabel(/^Encryption password/).fill('a long encryption passphrase');
    await panel.getByRole('button', { name: 'Turn on auto-sync' }).click();
    await panel.getByText(/^Auto-sync is on/).waitFor({ timeout: 30000 });
    for (let wait = 0; wait < 30 && checks() === before; wait += 1) await page.waitForTimeout(500);
    assert.ok(checks() > before, 'an automatic sync ran without the password');
    await panel.getByRole('button', { name: 'Forget the key on this device' }).click();
    await panel.getByRole('button', { name: 'Turn on auto-sync' }).waitFor({ timeout: 10000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('„Learn mnemonics“ is on the start page and shows a tile for every lesson', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.getByRole('button', { name: 'Learn mnemonics', exact: true }).click();
    await page.getByRole('heading', { name: 'Learn mnemonics' }).waitFor({ timeout: 15000 });
    assert.equal(await page.locator('.learn-tile').count(), 17, 'one tile per lesson');
    const text = await page.locator('body').innerText();
    for (const name of ['Simo', 'Mina', 'Cami', 'Bruno', 'Hafiza']) assert.match(text, new RegExp(name));
    assert.match(text, /0 of 17 done/);
    // Every lesson can report a wrong translation as a prefilled issue, without personal data.
    const report = await page.getByRole('link', { name: 'Wrong translation? Report it' }).getAttribute('href');
    assert.match(report, /^https:\/\/github\.com\/wasser-quest\/locora\/issues\/new\?labels=translation&title=Translation%3A%20overview%20%28en%29/);
    assert.ok(report.length < 6000);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('a lesson opens with its character, its check unlocks an achievement, and next/back move between lessons', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.getByRole('button', { name: 'Learn mnemonics', exact: true }).click();
    await page.getByRole('button', { name: /What is the method of loci\?/ }).click();
    await page.getByRole('heading', { name: 'What is the method of loci?' }).waitFor({ timeout: 15000 });
    assert.equal(await page.locator('.bubble-name').innerText(), 'Simo');
    await page.locator('.lesson-figure svg').first().waitFor({ timeout: 15000 });
    // A wrong answer gives a hint and the achievement stays locked.
    await page.getByRole('button', { name: 'Writing everything down on notes' }).click();
    await page.getByText(/Not quite\. Almost!/).waitFor();
    assert.equal(await page.locator('.celebration').count(), 0);
    // The right answer completes the exercise and unlocks the lesson's achievement.
    await page.getByRole('button', { name: 'Putting pictures at familiar places and walking past them in order' }).click();
    await page.getByText('Right! The exercise is done.').waitFor();
    await page.locator('.celebration', { hasText: 'What is the method of loci?' }).waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'Next lesson ▶' }).click();
    await page.getByRole('heading', { name: 'Visualising: vivid pictures' }).waitFor();
    assert.equal(await page.locator('.bubble-name').innerText(), 'Cami');
    await page.getByRole('button', { name: '◀ Previous lesson' }).click();
    await page.getByRole('heading', { name: 'What is the method of loci?' }).waitFor();
    await page.getByRole('button', { name: '← All lessons' }).click();
    await page.getByText('1 of 17 done').waitFor();
    assert.equal(await page.locator('.learn-tile.done').count(), 1);
    // Kept in the store with the other achievements: still there after a reload.
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'Learn mnemonics', exact: true }).click();
    await page.getByText('1 of 17 done').waitFor({ timeout: 15000 });
    await page.locator('.medal[data-achievement="learn-basics"]:not(.locked)').waitFor({ state: 'attached', timeout: 15000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the app tour first offers the lessons while they are open', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.getByRole('button', { name: 'App tour', exact: true }).click();
    const ask = page.getByRole('dialog', { name: 'Have you practised the memory techniques yet?' });
    await ask.waitFor({ timeout: 15000 });
    await ask.getByRole('button', { name: 'Learn mnemonics first' }).click();
    await page.getByRole('heading', { name: 'Learn mnemonics' }).waitFor({ timeout: 15000 });
    assert.equal(await page.getByRole('region', { name: 'App tour' }).count(), 0, 'no tour yet');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('after the last lesson a closing card offers the app tour and starts it', async () => {
  const { context, page, problems } = await openApp();
  try {
    await page.getByRole('button', { name: 'Learn mnemonics', exact: true }).click();
    await page.getByRole('button', { name: /Memory sports/ }).click();
    await page.getByRole('heading', { name: 'Memory sports' }).waitFor({ timeout: 15000 });
    await page.getByRole('button', { name: 'To the finish ▶' }).click();
    const finish = page.locator('.learn-finish');
    await finish.waitFor();
    assert.match(await finish.innerText(), /You have reached the end/);
    await finish.getByRole('button', { name: 'Start the app tour' }).click();
    // Straight into the tour: no question in between.
    await page.getByRole('region', { name: 'App tour' }).waitFor({ timeout: 15000 });
    await page.waitForFunction(() => document.body.innerText.includes('Step 1 of 13'), null, { timeout: 15000 });
    assert.equal(await page.getByRole('dialog').count(), 0);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('the first own palace: a photo, 5 places with a shopping list, one walk, an achievement', async () => {
  const { context, page, problems } = await openApp();
  try {
    const png = Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
      'base64',
    );
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: 'Choose a photo…' }).click(),
    ]);
    await chooser.setFiles({ name: 'Kitchen.png', mimeType: 'image/png', buffer: png });
    const guide = page.getByRole('region', { name: 'Your first own palace' });
    await guide.getByText('Step 1 of 3: set places').waitFor({ timeout: 15000 });
    const items = ['Milk', 'Bread', 'Apples', 'Cheese', 'Coffee'];
    const stage = page.locator('#tour-editor-stage');
    for (let i = 0; i < 5; i += 1) {
      const box = await stage.boundingBox();
      await stage.click({ position: { x: box.width * (0.12 + i * 0.18), y: box.height * 0.5 } });
      await guide.getByText(`${i + 1} of 5 places set`).or(guide.getByText('Step 2 of 3: put down the shopping list')).waitFor({ timeout: 15000 });
      await page.locator('#tour-editor-content input').fill(items[i]);
      await page.waitForTimeout(700);
    }
    await guide.getByText('Step 3 of 3: walk the route').waitFor({ timeout: 15000 });
    await guide.getByRole('button', { name: 'Walk the route now ▶' }).click();
    await page.waitForFunction(() => document.body.innerText.includes('1/5'), null, { timeout: 15000 });
    for (let n = 2; n <= 6; n += 1) {
      await page.keyboard.press('Space');
      await page.keyboard.press('3');
      if (n <= 5) await page.waitForFunction((n) => document.body.innerText.includes(`${n}/5`), n, { timeout: 15000 });
    }
    // The walk finished the exercise: the achievement is unlocked and celebrated, and the guide
    // says when the first review comes.
    await page.locator('.celebration', { hasText: 'First own palace' }).waitFor({ timeout: 20000 });
    await page.getByText('Your first review is due tomorrow.', { exact: false }).waitFor({ timeout: 20000 });
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

// ---------------------------------------------------------------- the game (gamification plugin)

/** Answers the question on screen: `grade` (1–4) for a recall, the first option for a quiz, a word for a guess. */
async function answerQuestion(page, grade) {
  const kind = await page.evaluate(() => {
    if (document.querySelector('.panel input[type=text]')) return 'guess';
    if (document.querySelector('.options input[type=checkbox]')) return 'quiz';
    return 'recall';
  });
  if (kind === 'guess') {
    const input = page.locator('.panel input[type=text]');
    await input.fill('something');
    await input.press('Enter');
  } else if (kind === 'quiz') {
    await page.keyboard.press('1');
    await page.keyboard.press('Enter');
  } else {
    await page.keyboard.press('Space');
    await page.locator('.grades').waitFor({ timeout: 15000 });
    await page.keyboard.press(grade);
  }
  return kind;
}

/** Walks the tutorial palace (9 loci) from its card; returns the end card. With `pause`, waits at each answer. */
async function walkTutorial(page, grade = '3', pause = null) {
  await page.getByRole('button', { name: /Walk the route/ }).first().click();
  for (let n = 1; n <= 9; n += 1) {
    await page.waitForFunction((n) => document.body.innerText.includes(`${n}/9`), n, { timeout: 20000 });
    const kind = await answerQuestion(page, grade);
    const next = page.getByRole('button', { name: 'Next (Space)' });
    if (await next.isVisible().catch(() => false) || pause) {
      await next.waitFor({ timeout: 15000 });
      if (pause) await pause(n, kind);
      await next.click();
    }
  }
  const done = page.locator('.journey-done');
  await done.waitFor({ timeout: 20000 });
  return done;
}

test('a journey earns XP for new loci whatever the grade, a mistake costs nothing, and the same walk again earns no more', async () => {
  const { context, page, problems } = await openApp();
  try {
    // Quick mode off: the line under each answer stays until „Next“.
    await page.evaluate(() => localStorage.setItem('locora-locus-display-v1', JSON.stringify({ quick_next: false })));
    await page.reload({ waitUntil: 'load' });
    await openTutorial(page);
    const lines = [];
    const done = await walkTutorial(page, '1', async (n) => {
      if (n === 1) {
        const bar = page.getByRole('complementary', { name: 'Learning progress' });
        await bar.waitFor({ timeout: 15000 });
        assert.doesNotMatch(await page.locator('body').innerText(), /♥|hearts?\b/i, 'no hearts');
        assert.equal(await page.locator('.game-hearts').count(), 0);
        lines.push(await page.locator('.game-line').innerText());
        // The one-time explanation sits under the bar; its buttons keep Space and Enter to themselves.
        await page.getByRole('note').getByText('How XP works now').waitFor({ timeout: 5000 });
      }
    });
    assert.equal(lines[0], 'Comes back soon – that is how it sticks.');
    const card = await done.innerText();
    assert.match(card, /of 9 recalled · \+54 XP/, card);
    assert.match(card, /0 due reviews · 9 new loci/, card);
    assert.match(card, /Next review: (today|tomorrow|\d{4}-\d\d-\d\d) · \d+ loc/, card);
    assert.equal(await done.getAttribute('role'), 'status');
    // The first finished journey is celebrated as a trophy.
    const celebration = page.locator('.celebration', { hasText: 'First journey' });
    await celebration.waitFor({ timeout: 20000 });
    assert.equal(await celebration.getAttribute('role'), 'status');
    await page.getByRole('button', { name: 'Back to overview' }).click();

    // The palace card says how much is consolidated (nothing yet: that takes reviews over days).
    await page.getByText('Consolidated: 0 of 9 loci').first().waitFor({ timeout: 15000 });
    // The same walk again today: practice before the loci are due earns nothing.
    await page.getByRole('button', { name: /^Learn Locora/ }).first().click();
    const again = await walkTutorial(page, '3');
    const second = await again.innerText();
    assert.match(second, /\+0 XP/, second);
    assert.match(second, /9 loci were not due yet, so they earned no XP\./, second);
    await page.getByRole('button', { name: 'Back to overview' }).click();

    // „Progress“: the tile and the trophy with the day it was earned.
    await page.locator('nav.pages button', { hasText: 'Progress' }).click();
    await page.locator('.tile', { hasText: '54 XP · this week 1/4 days' }).waitFor({ timeout: 15000 });
    const trophy = page.locator('.medal[data-achievement="game-first-journey"]');
    await trophy.waitFor({ timeout: 15000 });
    assert.match(await trophy.innerText(), /First journey\s+Since \d{4}-\d\d-\d\d/);
    assert.ok(await page.locator('.medal.locked[data-achievement="game-hundred-xp"]').count() === 1, 'locked with its hint');
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

test('with reduced motion a new medal is still; without, it moves', async () => {
  for (const reducedMotion of ['reduce', 'no-preference']) {
    const { context, page, problems } = await openApp();
    try {
      await page.emulateMedia({ reducedMotion });
      await openTutorial(page);
      await walkTutorial(page, '3');
      await page.getByRole('button', { name: 'Back to overview' }).click();
      await page.locator('nav.pages button', { hasText: 'Progress' }).click();
      const art = page.locator('.medal[data-achievement="game-first-journey"] svg').first();
      await art.waitFor({ timeout: 15000 });
      await page.waitForTimeout(500);
      const before = await art.evaluate((svg) => svg.innerHTML);
      await page.waitForTimeout(900);
      const after = await art.evaluate((svg) => svg.innerHTML);
      if (reducedMotion === 'reduce') assert.equal(after, before, 'still');
      else assert.notEqual(after, before, 'it breathes');
      assert.deepEqual(problems, []);
    } finally {
      await context.close();
    }
  }
});

test('an install from before the sync keeps its progress: the old IndexedDB blob is taken over', async () => {
  const { context, page, problems } = await openApp();
  try {
    // The plugin storage of an older version: { xp: 340, trophies: [FirstJourney], … } as postcard.
    await page.evaluate(async () => {
      const db = await new Promise((resolve, reject) => {
        const request = indexedDB.open('locora-plugins', 1);
        request.onupgradeneeded = () => request.result.createObjectStore('plugin-storage');
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      await new Promise((resolve, reject) => {
        const tx = db.transaction('plugin-storage', 'readwrite');
        tx.objectStore('plugin-storage').put(new Uint8Array([0xd4, 0x02, 0, 1, 0, 0, 0, 0, 0]), 'gamification\u0000progress');
        tx.oncomplete = resolve;
        tx.onerror = () => reject(tx.error);
      });
      db.close();
      localStorage.removeItem('locora-synced-storage-gamification-v1');
    });
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'Start the app tour' }).waitFor({ timeout: 15000 });
    const status = await page.evaluate(() => window.__TAURI__.core.invoke('gamification_status', { full: true }));
    assert.equal(status.xp, 340);
    assert.deepEqual(status.trophies.map((t) => t.id), ['first-journey', 'hundred-xp']);
    // Taken over once: a reload changes nothing.
    await page.reload({ waitUntil: 'load' });
    await page.getByRole('button', { name: 'Start the app tour' }).waitFor({ timeout: 15000 });
    const again = await page.evaluate(() => window.__TAURI__.core.invoke('gamification_status', { full: false }));
    assert.equal(again.xp, 340);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});

/** A Locora Cloud for several devices of one account: what one pushes, the others pull. */
function sharedCloud() {
  return { keys: null, ops: [], seq: 0, media: new Map(), log: [], gameDeletes: [], gameDeleteStatus: 204 };
}

async function serveCloud(page, cloud) {
  const account = {
    id: 'account-1', email: 'learner@example.org', display_name: 'Learner',
    subscription: 'active', plan: 'trial', period_end: 4102444800, email_verified: true,
  };
  const retention = { state: 'active', last_successful_sync_at: 0, expires_at: 4102444800, generation: 0 };
  await page.route('https://api.locora.cloud/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const json = (status, body) => route.fulfill({ status, contentType: 'application/json', body: body === undefined ? '' : JSON.stringify(body) });
    const key = `${request.method()} ${url.pathname}`;
    cloud.log.push(key);
    if (key === 'POST /v1/accounts' || key === 'POST /v1/sessions') return json(200, { token: 'session', account });
    if (key === 'GET /v1/me') return json(200, account);
    if (key === 'GET /v1/keys') return cloud.keys ? json(200, cloud.keys) : json(404, { error: 'none' });
    if (key === 'POST /v1/keys') {
      cloud.keys = request.postDataJSON();
      return json(200, {});
    }
    if (key === 'GET /v1/sync/retention' || key === 'POST /v1/sync/complete') return json(200, retention);
    if (key === 'GET /v1/sync/pending') return json(200, { count: 0 });
    if (key === 'POST /v1/media/missing') {
      return json(200, { missing: request.postDataJSON().ids.filter((id) => !cloud.media.has(id)) });
    }
    if (url.pathname.startsWith('/v1/media/')) {
      const id = url.pathname.split('/').pop();
      if (request.method() === 'PUT') {
        cloud.media.set(id, request.postDataBuffer());
        return json(200, {});
      }
      if (!cloud.media.has(id)) return json(404, { error: 'missing' });
      return route.fulfill({ status: 200, contentType: 'application/octet-stream', body: cloud.media.get(id) });
    }
    if (key === 'POST /v1/sync/ops') {
      const push = request.postDataJSON();
      let stored = 0;
      for (const op of push.ops) {
        if (cloud.ops.some((known) => known.op_id === op.op_id)) continue;
        cloud.seq += 1;
        cloud.ops.push({ seq: cloud.seq, device: push.device, ...op });
        stored += 1;
      }
      return json(200, { stored });
    }
    if (key === 'GET /v1/sync/ops') {
      const after = Number(url.searchParams.get('after'));
      const ops = cloud.ops.filter((op) => op.seq > after).slice(0, 1000);
      return json(200, { ops, cursor: ops.length ? ops[ops.length - 1].seq : after });
    }
    if (key === 'DELETE /v1/game/data') {
      cloud.gameDeletes.push(request.headers().authorization ?? '');
      if (cloud.gameDeleteStatus === 204) return route.fulfill({ status: 204 });
      return json(cloud.gameDeleteStatus, { error: 'not on this server' });
    }
    if (key === 'DELETE /v1/sessions') return route.fulfill({ status: 204 });
    return json(404, { error: `not in the test cloud: ${key}` });
  });
}

const CLOUD_PASSWORD = 'a long cloud password';
const PASSPHRASE = 'a long encryption passphrase';

/** Signs this device in (or up, the first one) and sets up the encryption when it is new. */
async function joinCloud(page, register) {
  await page.getByRole('button', { name: 'Settings & statistics' }).click();
  const panel = page.getByRole('region', { name: 'Locora Cloud' });
  await panel.getByLabel('Email').fill('learner@example.org');
  if (register) await panel.getByRole('checkbox', { name: /^Create a new account/ }).check();
  if (register) await panel.getByLabel(/^Display name/).fill('Learner');
  await panel.getByLabel(/^Cloud password/).fill(CLOUD_PASSWORD);
  if (register) await panel.getByLabel(/^Repeat cloud password/).fill(CLOUD_PASSWORD);
  await panel.getByRole('button', { name: register ? 'Create account' : 'Sign in' }).click();
  if (register) {
    await panel.getByLabel(/^New encryption password/).fill(PASSPHRASE);
    await panel.getByLabel(/^Repeat encryption password/).fill(PASSPHRASE);
    await panel.getByRole('button', { name: 'Set up encryption' }).click();
    await panel.getByRole('button', { name: 'I have saved the key' }).click({ timeout: 30000 });
  }
  return panel;
}

async function syncNow(panel) {
  await panel.getByLabel(/^Encryption password/).fill(PASSPHRASE);
  await panel.getByRole('button', { name: 'Sync now' }).click();
  await panel.getByText('Sync finished.').waitFor({ timeout: 60000 });
}

/** The game's line in the settings: „N XP · N medals · N learning days“. */
async function gameLine(page) {
  const line = page.getByRole('region', { name: 'Game progress & achievements' }).locator('p.progress');
  await line.waitFor({ timeout: 15000 });
  return line.innerText();
}

test('game progress is the same on two devices, and deleting it reaches both and the rankings', async () => {
  const cloud = sharedCloud();
  const a = await openApp();
  const b = await openApp();
  try {
    await serveCloud(a.page, cloud);
    await serveCloud(b.page, cloud);
    // Device A learns, then syncs.
    await openTutorial(a.page);
    await walkTutorial(a.page, '3');
    await a.page.getByRole('button', { name: 'Back to overview' }).click();
    const panelA = await joinCloud(a.page, true);
    await syncNow(panelA);
    const learned = await gameLine(a.page);
    assert.match(learned, /^54 XP · 1 medal · 1 learning day$/, learned);
    // Device B signs in to the same account and syncs: the same numbers.
    const panelB = await joinCloud(b.page, false);
    await syncNow(panelB);
    await b.page.getByRole('button', { name: '←' }).first().click();
    await b.page.getByRole('button', { name: 'Settings & statistics' }).click();
    assert.equal(await gameLine(b.page), learned);
    // What arrived by sync appears quietly: no celebration on B.
    assert.equal(await b.page.locator('.celebration').count(), 0);

    // A deletes the game progress. The confirmation says what goes and what stays.
    const game = a.page.getByRole('region', { name: 'Game progress & achievements' });
    await game.getByRole('button', { name: 'Delete game progress…' }).click();
    const warning = game.getByRole('alert');
    await warning.waitFor({ timeout: 5000 });
    assert.equal(await a.page.evaluate(() => document.activeElement?.getAttribute('role')), 'alert', 'the warning has the focus');
    assert.match(await game.innerText(), /Your ranking entries are also deleted from the Locora Cloud server\./);
    await a.page.keyboard.press('Escape');
    await game.getByRole('button', { name: 'Delete game progress…' }).waitFor({ timeout: 5000 });
    assert.equal(cloud.gameDeletes.length, 0, 'Escape deleted nothing');
    await game.getByRole('button', { name: 'Delete game progress…' }).click();
    await game.getByRole('button', { name: 'Yes, delete game progress on all devices' }).click();
    await game.getByText('Game progress deleted. Ranking entries deleted from the server.').waitFor({ timeout: 15000 });
    assert.deepEqual(cloud.gameDeletes, ['Bearer session']);
    assert.match(await gameLine(a.page), /^0 XP · 0 medals · /);
    await syncNow(panelA);
    // B gets the deletion with its next sync, and later facts of the old progress never count.
    await syncNow(panelB);
    await b.page.getByRole('button', { name: '←' }).first().click();
    await b.page.getByRole('button', { name: 'Settings & statistics' }).click();
    assert.match(await gameLine(b.page), /^0 XP · 0 medals · /);

    // An older server does not know the request: the progress is deleted anyway, and it says so.
    cloud.gameDeleteStatus = 404;
    await game.getByRole('button', { name: 'Delete game progress…' }).click();
    await game.getByRole('button', { name: 'Yes, delete game progress on all devices' }).click();
    await game.getByText(/The server cannot delete ranking entries yet; they stay until it is updated\./).waitFor({ timeout: 15000 });
    assert.deepEqual(a.problems, []);
    assert.deepEqual(b.problems, []);
  } finally {
    await a.context.close();
    await b.context.close();
  }
});

test('without an account, deleting the game progress stays on this device and says so', async () => {
  const { context, page, problems, requests } = await openApp();
  try {
    await openTutorial(page);
    await walkTutorial(page, '3');
    await page.getByRole('button', { name: 'Back to overview' }).click();
    await page.getByRole('button', { name: 'Settings & statistics' }).click();
    const game = page.getByRole('region', { name: 'Game progress & achievements' });
    assert.match(await gameLine(page), /^54 XP/);
    assert.match(await game.innerText(), /Stored on this device only\./);
    await game.getByRole('button', { name: 'Delete game progress…' }).click();
    assert.match(await game.innerText(), /This device is not signed in to Locora Cloud, so ranking entries on the server are not touched\./);
    await game.getByRole('button', { name: 'Yes, delete game progress on all devices' }).click();
    await game.getByText('Game progress deleted.', { exact: true }).waitFor({ timeout: 15000 });
    assert.match(await gameLine(page), /^0 XP · 0 medals · /);
    // The review schedule stays: the palace still has its cards (nothing is new again).
    const home = await page.evaluate(() => window.__TAURI__.core.invoke('home'));
    assert.equal(home.palaces.find((p) => p.name === 'Learn Locora').fresh, 0);
    assert.deepEqual(requests.filter((url) => url.includes('api.locora.cloud')), []);
    assert.deepEqual(problems, []);
  } finally {
    await context.close();
  }
});
