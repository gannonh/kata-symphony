#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  extractStableCoreFromNightlyTag,
  flagBool,
  flagString,
  isExecutedAsMain,
  latestMatchingTag,
  listGitTags,
  npmDistTagForVersion,
  parseArgs,
  printOrWriteOutputs,
  readCargoPackageVersion,
  repoRootFrom,
  stripVersionPrefix,
} from "./lib.ts";

export function resolveStableVersion(options: {
  inputVersion?: string;
  dryRun: boolean;
  runNumber: number;
  product: "symphony" | "cli";
  tags?: readonly string[];
  packageVersion?: string;
}): {
  version: string;
  tag: string;
  secondary_tag: string;
  name: string;
  secondary_name: string;
  npm_tag: string;
  is_prerelease: string;
  make_latest: string;
} {
  const prefix = options.product === "cli" ? "cli-v" : "symphony-v";
  const tags = options.tags ?? listGitTags();
  let version = options.inputVersion
    ? stripVersionPrefix(options.inputVersion)
    : undefined;

  if (!version) {
    const latestNightly = latestMatchingTag(tags, [
      new RegExp(`^${prefix}\\d+\\.\\d+\\.\\d+-nightly\\.\\d{8}\\.\\d+$`),
    ]);
    if (latestNightly) {
      version = extractStableCoreFromNightlyTag(latestNightly, prefix);
      if (!version) {
        throw new Error(
          `Could not parse stable core from nightly tag '${latestNightly}'.`,
        );
      }
    } else if (options.packageVersion) {
      // CLI has no scheduled nightly: fall back to the version currently in
      // package.json so dispatch can omit `version` when the tree is ready.
      version = stripVersionPrefix(options.packageVersion);
    } else if (options.dryRun) {
      version = `0.0.0-dryrun.${options.runNumber}`;
    } else {
      throw new Error(
        "No version input and no nightly tag to derive the stable version from. Cut a nightly first or pass version.",
      );
    }
  }

  if (!/^\d+\.\d+\.\d+([.-][0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`Invalid release version: ${version}`);
  }

  const isExactStable = /^\d+\.\d+\.\d+$/.test(version);
  const tag = `${prefix}${version}`;
  const secondaryTag =
    options.product === "symphony" ? `pi-symphony-v${version}` : "";
  const name =
    options.product === "cli"
      ? `Kata CLI v${version}`
      : `Symphony v${version}`;
  const secondaryName =
    options.product === "symphony"
      ? `Pi Symphony Extension v${version}`
      : "";

  return {
    version,
    tag,
    secondary_tag: secondaryTag,
    name,
    secondary_name: secondaryName,
    npm_tag: npmDistTagForVersion(version),
    is_prerelease: isExactStable ? "false" : "true",
    make_latest: isExactStable ? "true" : "false",
  };
}

function main(): void {
  const { flags } = parseArgs(process.argv.slice(2));
  const product = (flagString(flags, "product") ?? "symphony") as
    | "symphony"
    | "cli";
  const version = flagString(flags, "version");
  const dryRun = flagBool(flags, "dry-run");
  const runNumber = Number(flagString(flags, "run-number") ?? "1");
  const githubOutput = flagBool(flags, "github-output");
  const root = flagString(flags, "root") ?? repoRootFrom(import.meta.url);

  let packageVersion: string | undefined;
  if (!version) {
    if (product === "cli") {
      packageVersion = (
        JSON.parse(
          readFileSync(join(root, "apps/cli/package.json"), "utf8"),
        ) as { version: string }
      ).version;
    } else if (dryRun) {
      packageVersion = readCargoPackageVersion(
        join(root, "apps/symphony/Cargo.toml"),
      );
    }
  }

  const resolved = resolveStableVersion({
    inputVersion: version,
    dryRun,
    runNumber,
    product,
    packageVersion,
  });

  printOrWriteOutputs(
    {
      release_channel: "stable",
      ...resolved,
    },
    githubOutput,
  );
}

if (isExecutedAsMain(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error((error as Error).message);
    process.exit(1);
  }
}
