import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

export type ReleaseChannel = "stable" | "nightly";

export interface SemverCore {
  major: number;
  minor: number;
  patch: number;
}

export function stripVersionPrefix(raw: string): string {
  return raw.trim().replace(/^v/i, "");
}

export function parseSemverCore(version: string): SemverCore {
  const core = version.replace(/[-+].*$/, "");
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(core);
  if (!match) {
    throw new Error(`Invalid semver core in '${version}'.`);
  }
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
  };
}

export function formatSemverCore(core: SemverCore): string {
  return `${core.major}.${core.minor}.${core.patch}`;
}

export function bumpPatch(version: string): string {
  const core = parseSemverCore(version);
  return formatSemverCore({ ...core, patch: core.patch + 1 });
}

/** Next free exact-semver under prefix, bumping patch while the git tag exists. */
export function nextAvailableExactVersion(
  startVersion: string,
  tags: readonly string[],
  prefix: string,
  maxBumps = 100,
): string {
  let version = stripVersionPrefix(startVersion);
  if (isPrerelease(version)) {
    return version;
  }

  const tagSet = new Set(tags);
  for (let i = 0; i <= maxBumps; i += 1) {
    const tag = `${prefix}${version}`;
    if (!tagSet.has(tag)) {
      return version;
    }
    version = bumpPatch(version);
  }

  throw new Error(
    `Could not find a free ${prefix}* version after ${maxBumps} patch bumps from ${startVersion}.`,
  );
}

export function isPrerelease(version: string): boolean {
  return /[-+]/.test(version);
}

export function npmDistTagForVersion(version: string): string {
  if (!isPrerelease(version)) return "latest";
  const prerelease = version.split("-")[1] ?? "";
  const id = prerelease.split(".")[0] || "next";
  if (id === "nightly") return "nightly";
  return id;
}

export function appendGithubOutput(entries: Record<string, string>): void {
  const path = process.env.GITHUB_OUTPUT;
  if (!path) {
    throw new Error("GITHUB_OUTPUT is not set.");
  }
  const body = Object.entries(entries)
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
  writeFileSync(path, `${body}\n`, { flag: "a" });
}

export function printOrWriteOutputs(
  entries: Record<string, string>,
  githubOutput: boolean,
): void {
  if (githubOutput) {
    appendGithubOutput(entries);
    return;
  }
  for (const [key, value] of Object.entries(entries)) {
    process.stdout.write(`${key}=${value}\n`);
  }
}

export function readJsonFile<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

export function writeJsonFile(path: string, value: unknown): void {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

export function readCargoPackageVersion(cargoTomlPath: string): string {
  const content = readFileSync(cargoTomlPath, "utf8");
  const match = /^version\s*=\s*"([^"]+)"/m.exec(content);
  if (!match) {
    throw new Error(`Could not find package version in ${cargoTomlPath}`);
  }
  return match[1];
}

export function writeCargoPackageVersion(
  cargoTomlPath: string,
  version: string,
): boolean {
  const content = readFileSync(cargoTomlPath, "utf8");
  const next = content.replace(
    /^version\s*=\s*"[^"]+"/m,
    `version = "${version}"`,
  );
  if (next === content) return false;
  writeFileSync(cargoTomlPath, next, "utf8");
  return true;
}

export function listGitTags(cwd = process.cwd()): string[] {
  const stdout = execFileSync("git", ["tag", "--list"], {
    cwd,
    encoding: "utf8",
  });
  return stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function nightlySortKey(tag: string): [number, number, number, number, number] | null {
  const match =
    /(?:^|-)v?(\d+)\.(\d+)\.(\d+)-nightly\.(\d{8})\.(\d+)$/.exec(tag);
  if (!match) return null;
  return [
    Number(match[1]),
    Number(match[2]),
    Number(match[3]),
    Number(match[4]),
    Number(match[5]),
  ];
}

function compareNightlyKeys(
  left: [number, number, number, number, number],
  right: [number, number, number, number, number],
): number {
  for (let i = 0; i < left.length; i += 1) {
    if (left[i] !== right[i]) return left[i] - right[i];
  }
  return 0;
}

export function latestMatchingTag(
  tags: readonly string[],
  patterns: readonly RegExp[],
): string | undefined {
  const matched = tags.filter((tag) => patterns.some((re) => re.test(tag)));
  if (matched.length === 0) return undefined;

  const nightlies = matched
    .map((tag) => ({ tag, key: nightlySortKey(tag) }))
    .filter(
      (
        entry,
      ): entry is {
        tag: string;
        key: [number, number, number, number, number];
      } => entry.key !== null,
    )
    .toSorted((a, b) => compareNightlyKeys(b.key, a.key));
  if (nightlies[0]) return nightlies[0].tag;

  // Prefer creator date ordering when available; fall back to lex sort.
  try {
    const listed = execFileSync(
      "git",
      [
        "for-each-ref",
        "--sort=-creatordate",
        "--format=%(refname:short)",
        ...matched.map((tag) => `refs/tags/${tag}`),
      ],
      { encoding: "utf8" },
    )
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    if (listed[0]) return listed[0];
  } catch {
    // ignore and fall through
  }

  return matched.toSorted().at(-1);
}

export function extractStableCoreFromNightlyTag(
  tag: string,
  prefix: string,
): string | undefined {
  const re = new RegExp(
    `^${prefix}(\\d+\\.\\d+\\.\\d+)-nightly\\.\\d{8}\\.\\d+$`,
  );
  const match = re.exec(tag);
  return match?.[1];
}

export function repoRootFrom(metaUrl: string): string {
  return resolve(dirname(fileURLToPath(metaUrl)), "../..");
}

export function isExecutedAsMain(metaUrl: string, argv1 = process.argv[1]): boolean {
  if (!argv1) return false;
  try {
    return fileURLToPath(metaUrl) === resolve(argv1);
  } catch {
    return false;
  }
}

export function parseArgs(argv: string[]): {
  flags: Record<string, string | boolean>;
  positionals: string[];
} {
  const flags: Record<string, string | boolean> = {};
  const positionals: string[] = [];

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) {
      positionals.push(arg);
      continue;
    }

    const body = arg.slice(2);
    if (body.includes("=")) {
      const [key, ...rest] = body.split("=");
      flags[key] = rest.join("=");
      continue;
    }

    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      flags[body] = next;
      i += 1;
    } else {
      flags[body] = true;
    }
  }

  return { flags, positionals };
}

export function flagString(
  flags: Record<string, string | boolean>,
  name: string,
): string | undefined {
  const value = flags[name];
  if (value === undefined || value === true) return undefined;
  return String(value);
}

export function flagBool(
  flags: Record<string, string | boolean>,
  name: string,
  defaultValue = false,
): boolean {
  const value = flags[name];
  if (value === undefined) return defaultValue;
  if (value === true) return true;
  if (value === false) return false;
  return value === "true" || value === "1";
}
