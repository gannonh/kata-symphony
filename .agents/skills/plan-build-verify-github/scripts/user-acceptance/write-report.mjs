#!/usr/bin/env node
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

function arg(name, fallback = '') {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}
const evidenceDir = arg('evidence');
if (!evidenceDir) {
  console.error('Usage: write-report.mjs --evidence <dir>');
  process.exit(2);
}
const manifestPath = join(evidenceDir, 'evidence.json');
if (!existsSync(manifestPath)) {
  console.error(`Missing manifest: ${manifestPath}`);
  process.exit(2);
}
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
const reportPath = join(evidenceDir, 'evidence.md');
const lines = [];
lines.push(`# UAT Evidence: ${manifest.scope}`);
lines.push('');
lines.push(`UAT Scope: ${manifest.scope}`);
lines.push(`Target: ${manifest.target}`);
lines.push(`Timestamp: ${manifest.timestamp}`);
lines.push(`Git commit: ${manifest.git_commit}`);
lines.push('');
lines.push('## Slice-by-slice result');
if (manifest.slices?.length) {
  for (const slice of manifest.slices) {
    lines.push(`- ${slice.result}: ${slice.name}${slice.summary ? ` - ${slice.summary}` : ''}`);
  }
} else {
  lines.push('- Pending: no slices recorded yet.');
}
lines.push('');
lines.push('## Evidence');
if (manifest.artifacts?.length) {
  for (const artifact of manifest.artifacts) {
    lines.push(`- \`${artifact.path}\` - ${artifact.description ?? artifact.type}`);
  }
} else {
  lines.push('- No artifacts recorded yet.');
}
lines.push('');
lines.push('## Commands');
if (manifest.commands?.length) {
  for (const command of manifest.commands) {
    lines.push(`- Exit ${command.exit_code}: \`${command.command}\` -> \`${command.output_path}\``);
  }
} else {
  lines.push('- No commands recorded yet.');
}
if (manifest.notes?.length) {
  lines.push('');
  lines.push('## Notes');
  for (const note of manifest.notes) lines.push(`- ${note}`);
}
lines.push('');
lines.push('## Manual Run Instructions');
lines.push('1. Run the product using the documented local command for this target.');
lines.push('   Expected: the feature path is reachable without setup or launch errors.');
lines.push('2. Follow the same user path described in the acceptance slices above.');
lines.push('   Expected: each passing slice reaches the visible or inspectable outcome shown in the evidence.');
lines.push('');
lines.push('Recommendation: Pending user sign-off');
lines.push('Please reply: accept / reject');
lines.push('');
writeFileSync(reportPath, lines.join('\n'));
console.log(reportPath);
