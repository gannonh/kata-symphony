#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';

function arg(name, fallback = '') {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

const evidenceDir = arg('evidence');
const cdp = arg('cdp', 'http://127.0.0.1:9222');
const title = arg('title');
const urlContains = arg('url-contains');
const screenshotPath = arg('screenshot');
const textPath = arg('text');

if (!evidenceDir || !screenshotPath) {
  console.error('Usage: cdp-capture-page.mjs --evidence <dir> --cdp http://127.0.0.1:9222 [--title <title>|--url-contains <text>] --screenshot <path> [--text <path>]');
  process.exit(2);
}

function readManifest(dir) {
  return JSON.parse(readFileSync(join(dir, 'evidence.json'), 'utf8'));
}
function writeManifest(dir, manifest) {
  writeFileSync(join(dir, 'evidence.json'), `${JSON.stringify(manifest, null, 2)}\n`);
}
function artifactPath(path) {
  return path.startsWith(evidenceDir) ? path : join(evidenceDir, path);
}

const list = await fetch(`${cdp.replace(/\/$/, '')}/json/list`).then((r) => r.json());
const target = list.find((item) => {
  if (title && item.title !== title) return false;
  if (urlContains && !String(item.url ?? '').includes(urlContains)) return false;
  return item.type === 'page';
}) ?? list.find((item) => item.type === 'page');

if (!target?.webSocketDebuggerUrl) {
  console.error(`No CDP page target found at ${cdp}`);
  process.exit(3);
}

const ws = new WebSocket(target.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
function send(method, params = {}) {
  const id = ++nextId;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
ws.addEventListener('message', (event) => {
  const message = JSON.parse(event.data);
  if (!message.id || !pending.has(message.id)) return;
  const { resolve, reject } = pending.get(message.id);
  pending.delete(message.id);
  message.error ? reject(new Error(JSON.stringify(message.error))) : resolve(message.result);
});
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve, { once: true });
  ws.addEventListener('error', reject, { once: true });
});

await send('Page.enable');
await send('Runtime.evaluate', { expression: 'document.fonts && document.fonts.ready', awaitPromise: true });
const screenshot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, fromSurface: true });
const resolvedScreenshotPath = artifactPath(screenshotPath);
mkdirSync(dirname(resolvedScreenshotPath), { recursive: true });
writeFileSync(resolvedScreenshotPath, Buffer.from(screenshot.data, 'base64'));

let resolvedTextPath = '';
if (textPath) {
  const text = await send('Runtime.evaluate', { expression: 'document.body?.innerText ?? document.documentElement.innerText ?? ""', returnByValue: true });
  resolvedTextPath = artifactPath(textPath);
  mkdirSync(dirname(resolvedTextPath), { recursive: true });
  writeFileSync(resolvedTextPath, `${text.result?.value ?? ''}\n`);
}
ws.close();

if (existsSync(join(evidenceDir, 'evidence.json'))) {
  const manifest = readManifest(evidenceDir);
  manifest.artifacts.push({ type: 'screenshot', path: resolvedScreenshotPath, description: `CDP screenshot for ${target.title}` });
  if (resolvedTextPath) manifest.artifacts.push({ type: 'log', path: resolvedTextPath, description: `CDP text snapshot for ${target.title}` });
  writeManifest(evidenceDir, manifest);
}

console.log(JSON.stringify({ target: { title: target.title, url: target.url }, screenshot: resolvedScreenshotPath, text: resolvedTextPath || null }, null, 2));
