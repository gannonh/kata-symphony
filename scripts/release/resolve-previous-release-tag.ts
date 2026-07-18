#!/usr/bin/env node
import {
  flagBool,
  flagString,
  isExecutedAsMain,
  listGitTags,
  parseArgs,
  printOrWriteOutputs,
  type ReleaseChannel,
} from "./lib.ts";

interface StableVersion {
  major: number;
  minor: number;
  patch: number;
  prerelease: readonly string[];
}

interface NightlyVersion {
  major: number;
  minor: number;
  patch: number;
  date: number;
  runNumber: number;
}

const parseNumericIdentifier = (identifier: string): number | undefined =>
  /^\d+$/.test(identifier) ? Number(identifier) : undefined;

const comparePrereleaseIdentifiers = (left: string, right: string): number => {
  const leftNumeric = parseNumericIdentifier(left);
  const rightNumeric = parseNumericIdentifier(right);
  if (leftNumeric !== undefined && rightNumeric !== undefined) {
    return leftNumeric - rightNumeric;
  }
  if (leftNumeric !== undefined) return -1;
  if (rightNumeric !== undefined) return 1;
  return left.localeCompare(right);
};

const compareStableVersions = (left: StableVersion, right: StableVersion): number => {
  if (left.major !== right.major) return left.major - right.major;
  if (left.minor !== right.minor) return left.minor - right.minor;
  if (left.patch !== right.patch) return left.patch - right.patch;

  const leftHas = left.prerelease.length > 0;
  const rightHas = right.prerelease.length > 0;
  if (!leftHas && !rightHas) return 0;
  if (!leftHas) return 1;
  if (!rightHas) return -1;

  const max = Math.max(left.prerelease.length, right.prerelease.length);
  for (let i = 0; i < max; i += 1) {
    const l = left.prerelease[i];
    const r = right.prerelease[i];
    if (l === undefined) return -1;
    if (r === undefined) return 1;
    const cmp = comparePrereleaseIdentifiers(l, r);
    if (cmp !== 0) return cmp;
  }
  return 0;
};

const compareNightlyVersions = (left: NightlyVersion, right: NightlyVersion): number => {
  if (left.major !== right.major) return left.major - right.major;
  if (left.minor !== right.minor) return left.minor - right.minor;
  if (left.patch !== right.patch) return left.patch - right.patch;
  if (left.date !== right.date) return left.date - right.date;
  return left.runNumber - right.runNumber;
};

export function parsePrefixedStableTag(
  tag: string,
  prefix: string,
): StableVersion | undefined {
  const match = new RegExp(
    `^${prefix}(\\d+)\\.(\\d+)\\.(\\d+)(?:-([0-9A-Za-z.-]+))?(?:\\+[0-9A-Za-z.-]+)?$`,
  ).exec(tag);
  if (!match) return undefined;
  const prerelease = match[4] ? match[4].split(".") : [];
  if (prerelease[0] === "nightly") return undefined;
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease,
  };
}

export function parsePrefixedNightlyTag(
  tag: string,
  prefix: string,
): NightlyVersion | undefined {
  const match = new RegExp(
    `^${prefix}(\\d+)\\.(\\d+)\\.(\\d+)-nightly\\.(\\d{8})\\.(\\d+)$`,
  ).exec(tag);
  if (!match) return undefined;
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    date: Number(match[4]),
    runNumber: Number(match[5]),
  };
}

export function resolvePreviousReleaseTag(
  channel: ReleaseChannel,
  currentTag: string,
  tags: readonly string[],
  prefix: string,
): string | undefined {
  if (channel === "stable") {
    const current = parsePrefixedStableTag(currentTag, prefix);
    if (!current) {
      throw new Error(`Invalid stable release tag '${currentTag}'.`);
    }
    const candidates = tags
      .map((tag) => ({ tag, parsed: parsePrefixedStableTag(tag, prefix) }))
      .filter(
        (entry): entry is { tag: string; parsed: StableVersion } =>
          entry.parsed !== undefined,
      )
      .filter((entry) => compareStableVersions(entry.parsed, current) < 0)
      .toSorted((a, b) => compareStableVersions(b.parsed, a.parsed));
    return candidates[0]?.tag;
  }

  const current = parsePrefixedNightlyTag(currentTag, prefix);
  if (!current) {
    throw new Error(`Invalid nightly release tag '${currentTag}'.`);
  }
  const candidates = tags
    .map((tag) => ({ tag, parsed: parsePrefixedNightlyTag(tag, prefix) }))
    .filter(
      (entry): entry is { tag: string; parsed: NightlyVersion } =>
        entry.parsed !== undefined,
    )
    .filter((entry) => compareNightlyVersions(entry.parsed, current) < 0)
    .toSorted((a, b) => compareNightlyVersions(b.parsed, a.parsed));
  return candidates[0]?.tag;
}

function main(): void {
  const { flags } = parseArgs(process.argv.slice(2));
  const channel = flagString(flags, "channel") as ReleaseChannel | undefined;
  const currentTag = flagString(flags, "current-tag");
  const prefix = flagString(flags, "prefix");
  const githubOutput = flagBool(flags, "github-output");

  if (!channel || !currentTag || !prefix) {
    throw new Error(
      "Usage: resolve-previous-release-tag.ts --channel stable|nightly --current-tag TAG --prefix PREFIX [--github-output]",
    );
  }
  if (channel !== "stable" && channel !== "nightly") {
    throw new Error(`Invalid channel '${channel}'.`);
  }

  const previous =
    resolvePreviousReleaseTag(channel, currentTag, listGitTags(), prefix) ?? "";
  printOrWriteOutputs({ previous_tag: previous }, githubOutput);
}

if (isExecutedAsMain(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error((error as Error).message);
    process.exit(1);
  }
}
