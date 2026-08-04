#!/usr/bin/env node
import { readFileSync, existsSync, statSync } from 'node:fs';
import { join } from 'node:path';

function arg(name, fallback = '') {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}
function fail(message) {
  console.error(`FAIL: ${message}`);
  failures += 1;
}
let failures = 0;
const evidenceDir = arg('evidence');
if (!evidenceDir) {
  console.error('Usage: verify-evidence.mjs --evidence <dir>');
  process.exit(2);
}
const manifestPath = join(evidenceDir, 'evidence.json');
if (!existsSync(manifestPath)) fail(`missing ${manifestPath}`);
let manifest = null;
try {
  manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
} catch (error) {
  fail(`invalid evidence.json: ${error.message}`);
}
if (manifest) {
  for (const field of ['scope', 'target', 'timestamp', 'git_commit']) {
    if (!manifest[field]) fail(`missing manifest field ${field}`);
  }
  if (!Array.isArray(manifest.artifacts)) fail('artifacts must be an array');
  if (!Array.isArray(manifest.commands)) fail('commands must be an array');
  if (!Array.isArray(manifest.slices)) fail('slices must be an array');
  for (const artifact of manifest.artifacts ?? []) {
    if (!artifact.path) {
      fail('artifact missing path');
      continue;
    }
    if (!existsSync(artifact.path)) fail(`artifact path does not exist: ${artifact.path}`);
  }
  const uiTarget = ['web', 'electron', 'tui', 'mixed'].includes(manifest.target);
  if (uiTarget) {
    const hasVisible = (manifest.artifacts ?? []).some((artifact) => ['video', 'screenshot'].includes(artifact.type));
    const hasVideoSkipped = (manifest.notes ?? []).some((note) => /video.*skipp|screenshot/i.test(note));
    if (!hasVisible && !hasVideoSkipped) fail('UI target needs a video/screenshot artifact or an explicit note explaining why visual capture was skipped');
  }
}
const reportPath = join(evidenceDir, 'evidence.md');
if (!existsSync(reportPath)) {
  fail(`missing ${reportPath}`);
} else if (statSync(reportPath).size === 0) {
  fail(`${reportPath} is empty`);
}

if (failures > 0) process.exit(1);
console.log(`Evidence OK: ${evidenceDir}`);
