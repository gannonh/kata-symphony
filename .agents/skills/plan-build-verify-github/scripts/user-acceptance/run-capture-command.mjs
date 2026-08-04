#!/usr/bin/env node
import { createWriteStream, readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { spawn } from 'node:child_process';

const dashDash = process.argv.indexOf('--');
const command = dashDash >= 0 ? process.argv.slice(dashDash + 1) : [];
function arg(name, fallback = '') {
  const limit = dashDash >= 0 ? dashDash : process.argv.length;
  const index = process.argv.slice(0, limit).indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}
function has(name) {
  const limit = dashDash >= 0 ? dashDash : process.argv.length;
  return process.argv.slice(0, limit).includes(`--${name}`);
}
function slug(input) {
  return input.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 80) || 'command';
}
function readManifest(dir) {
  const path = join(dir, 'evidence.json');
  return JSON.parse(readFileSync(path, 'utf8'));
}
function writeManifest(dir, manifest) {
  writeFileSync(join(dir, 'evidence.json'), `${JSON.stringify(manifest, null, 2)}\n`);
}

const evidenceDir = arg('evidence');
const name = arg('name', command[0] ?? 'command');
const cwd = arg('cwd', process.cwd());
const allowFail = has('allow-fail');
const timeoutMs = Number(arg('timeout-ms', '0'));
if (!evidenceDir || command.length === 0) {
  console.error('Usage: run-capture-command.mjs --evidence <dir> --name <name> [--cwd <cwd>] [--allow-fail] [--timeout-ms <ms>] -- <command> [args...]');
  process.exit(2);
}

const logPath = join(evidenceDir, 'logs', `${slug(name)}.log`);
const exitPath = join(evidenceDir, 'logs', `${slug(name)}.exit`);
mkdirSync(dirname(logPath), { recursive: true });
if (!existsSync(join(evidenceDir, 'evidence.json'))) {
  console.error(`Missing manifest: ${join(evidenceDir, 'evidence.json')}`);
  process.exit(2);
}

const log = createWriteStream(logPath, { flags: 'w' });
log.write(`$ ${command.map((part) => JSON.stringify(part)).join(' ')}\n`);
log.write(`cwd: ${cwd}\nstarted_at: ${new Date().toISOString()}\n\n`);

const child = spawn(command[0], command.slice(1), { cwd, shell: false, env: process.env });
let timedOut = false;
let timer = null;
if (Number.isFinite(timeoutMs) && timeoutMs > 0) {
  timer = setTimeout(() => {
    timedOut = true;
    log.write(`\ntimeout_ms: ${timeoutMs}\n`);
    child.kill('SIGTERM');
    setTimeout(() => child.kill('SIGKILL'), 2_000).unref();
  }, timeoutMs);
  timer.unref();
}
child.stdout.on('data', (chunk) => { process.stdout.write(chunk); log.write(chunk); });
child.stderr.on('data', (chunk) => { process.stderr.write(chunk); log.write(chunk); });

const exitCodeRaw = await new Promise((resolve) => child.on('close', resolve));
if (timer) clearTimeout(timer);
const exitCode = timedOut ? 124 : exitCodeRaw;
log.write(`\nexit_code: ${exitCode}\nfinished_at: ${new Date().toISOString()}\n`);
log.end();
writeFileSync(exitPath, `${exitCode}\n`);

const manifest = readManifest(evidenceDir);
manifest.commands.push({
  command: command.join(' '),
  name,
  cwd,
  exit_code: exitCode,
  output_path: logPath,
  exit_code_path: exitPath,
  timed_out: timedOut,
});
writeManifest(evidenceDir, manifest);

process.exit(exitCode === 0 || allowFail ? 0 : exitCode || 1);
