#!/usr/bin/env node
import { join } from "node:path";
import {
  flagBool,
  flagString,
  isExecutedAsMain,
  parseArgs,
  printOrWriteOutputs,
  readJsonFile,
  repoRootFrom,
  writeJsonFile,
} from "./lib.ts";

export function updateCliVersion(
  version: string,
  rootDir = process.cwd(),
): { changed: boolean } {
  const pkgPath = join(rootDir, "apps/cli/package.json");
  const pkg = readJsonFile<Record<string, unknown>>(pkgPath);
  if (pkg.version === version) {
    return { changed: false };
  }
  writeJsonFile(pkgPath, { ...pkg, version });
  return { changed: true };
}

function main(): void {
  const { flags, positionals } = parseArgs(process.argv.slice(2));
  const version = flagString(flags, "version") ?? positionals[0];
  const root = flagString(flags, "root") ?? repoRootFrom(import.meta.url);
  const githubOutput = flagBool(flags, "github-output");

  if (!version) {
    throw new Error(
      "Usage: update-cli-version.ts <version> [--root DIR] [--github-output]",
    );
  }

  const { changed } = updateCliVersion(version, root);
  if (!changed) {
    console.log("CLI version already matches release version.");
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
