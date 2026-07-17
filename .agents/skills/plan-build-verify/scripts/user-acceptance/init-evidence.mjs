#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { execSync } from 'node:child_process';

function arg(name, fallback = '') {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

function stamp() {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}${pad(d.getUTCMonth() + 1)}${pad(d.getUTCDate())}-${pad(d.getUTCHours())}${pad(d.getUTCMinutes())}${pad(d.getUTCSeconds())}`;
}

const target = arg('target');
if (!target) {
  console.error('Usage: init-evidence.mjs --target <web|electron|cli|tui|api|sdk|mixed> --scope <scope> [--dir <path>]');
  process.exit(2);
}

const scope = arg('scope', 'validated behavior');
const dir = arg('dir', `uat-evidence/${target}-${stamp()}`);
const subdirs = ['screenshots', 'recordings', 'logs', 'responses', 'outputs', 'payloads', 'examples'];
mkdirSync(dir, { recursive: true });
for (const subdir of subdirs) mkdirSync(join(dir, subdir), { recursive: true });

let gitCommit = 'unknown';
try {
  gitCommit = execSync('git rev-parse --short HEAD', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
} catch {}

let gitignored = false;
try {
  const ignore = readFileSync('.gitignore', 'utf8');
  gitignored = /^\/?uat-evidence\/?$/m.test(ignore) || /^uat-evidence\//m.test(ignore) || /^\/uat-evidence\//m.test(ignore);
} catch {}

const manifest = {
  scope,
  target,
  timestamp: new Date().toISOString(),
  git_commit: gitCommit,
  gitignored,
  artifacts: [],
  commands: [],
  slices: [],
  notes: [],
};

const manifestPath = join(dir, 'evidence.json');
if (!existsSync(manifestPath)) {
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}

console.log(JSON.stringify({ dir, manifest: manifestPath, gitignored }, null, 2));
