#!/usr/bin/env node
import { join } from "node:path";
import {
  flagBool,
  flagString,
  isExecutedAsMain,
  parseArgs,
  printOrWriteOutputs,
  readCargoPackageVersion,
  readJsonFile,
  repoRootFrom,
  writeCargoPackageVersion,
  writeJsonFile,
} from "./lib.ts";

export const symphonyReleaseFiles = {
  cargoToml: "apps/symphony/Cargo.toml",
  piExtensionPackage: "apps/symphony/pi-extension/package.json",
} as const;

export function updateSymphonyVersions(
  version: string,
  rootDir = process.cwd(),
): { changed: boolean } {
  let changed = false;

  const cargoPath = join(rootDir, symphonyReleaseFiles.cargoToml);
  if (readCargoPackageVersion(cargoPath) !== version) {
    writeCargoPackageVersion(cargoPath, version);
    changed = true;
  }

  const pkgPath = join(rootDir, symphonyReleaseFiles.piExtensionPackage);
  const pkg = readJsonFile<Record<string, unknown>>(pkgPath);
  if (pkg.version !== version) {
    writeJsonFile(pkgPath, { ...pkg, version });
    changed = true;
  }

  return { changed };
}

function main(): void {
  const { flags, positionals } = parseArgs(process.argv.slice(2));
  const version = flagString(flags, "version") ?? positionals[0];
  const root = flagString(flags, "root") ?? repoRootFrom(import.meta.url);
  const githubOutput = flagBool(flags, "github-output");

  if (!version) {
    throw new Error(
      "Usage: update-symphony-versions.ts <version> [--root DIR] [--github-output]",
    );
  }

  const { changed } = updateSymphonyVersions(version, root);
  if (!changed) {
    console.log("Symphony versions already match release version.");
  }
  if (githubOutput) {
    printOrWriteOutputs({ changed: String(changed) }, true);
  }
}

if (isExecutedAsMain(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error((error as Error).message);
    process.exit(1);
  }
}
