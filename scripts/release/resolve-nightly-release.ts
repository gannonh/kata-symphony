#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  bumpPatch,
  flagBool,
  flagString,
  isExecutedAsMain,
  parseArgs,
  printOrWriteOutputs,
  readCargoPackageVersion,
  repoRootFrom,
} from "./lib.ts";

export function resolveNightlyBaseVersion(version: string): string {
  return version.replace(/[-+].*$/, "");
}

export function resolveNightlyTargetVersion(version: string): string {
  return bumpPatch(resolveNightlyBaseVersion(version));
}

export function resolveNightlyReleaseMetadata(
  baseVersion: string,
  date: string,
  runNumber: number,
  sha: string,
  options: { product: "symphony" | "cli"; tagPrefix: string },
) {
  if (!/^\d{8}$/.test(date)) {
    throw new Error(`Invalid nightly date '${date}'. Expected YYYYMMDD.`);
  }
  if (!Number.isInteger(runNumber) || runNumber < 1) {
    throw new Error(`Invalid run number '${runNumber}'.`);
  }
  if (!/^[0-9a-f]{7,40}$/i.test(sha)) {
    throw new Error(`Invalid sha '${sha}'.`);
  }

  const shortSha = sha.slice(0, 12);
  const version = `${baseVersion}-nightly.${date}.${runNumber}`;
  const productLabel = options.product === "cli" ? "Kata CLI" : "Symphony";
  return {
    base_version: baseVersion,
    version,
    tag: `${options.tagPrefix}${version}`,
    name: `${productLabel} Nightly ${version} (${shortSha})`,
    short_sha: shortSha,
  };
}

function main(): void {
  const { flags } = parseArgs(process.argv.slice(2));
  const date = flagString(flags, "date");
  const runNumberRaw = flagString(flags, "run-number");
  const sha = flagString(flags, "sha");
  const product = (flagString(flags, "product") ?? "symphony") as
    | "symphony"
    | "cli";
  const root = flagString(flags, "root") ?? repoRootFrom(import.meta.url);
  const githubOutput = flagBool(flags, "github-output");

  if (!date || !runNumberRaw || !sha) {
    throw new Error(
      "Usage: resolve-nightly-release.ts --date YYYYMMDD --run-number N --sha SHA [--product symphony|cli] [--github-output]",
    );
  }

  const runNumber = Number(runNumberRaw);
  let baseVersion: string;
  let tagPrefix: string;

  if (product === "cli") {
    const pkg = JSON.parse(
      readFileSync(join(root, "apps/cli/package.json"), "utf8"),
    ) as { version: string };
    baseVersion = resolveNightlyTargetVersion(pkg.version);
    tagPrefix = "cli-v";
  } else {
    baseVersion = resolveNightlyTargetVersion(
      readCargoPackageVersion(join(root, "apps/symphony/Cargo.toml")),
    );
    tagPrefix = "symphony-v";
  }

  const metadata = resolveNightlyReleaseMetadata(
    baseVersion,
    date,
    runNumber,
    sha,
    { product, tagPrefix },
  );
  printOrWriteOutputs(metadata, githubOutput);
}

if (isExecutedAsMain(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error((error as Error).message);
    process.exit(1);
  }
}
