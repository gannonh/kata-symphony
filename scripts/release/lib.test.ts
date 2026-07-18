import { describe, expect, it } from "vitest";
import {
  bumpPatch,
  extractStableCoreFromNightlyTag,
  npmDistTagForVersion,
  parseSemverCore,
  stripVersionPrefix,
} from "./lib.ts";
import {
  resolveNightlyBaseVersion,
  resolveNightlyReleaseMetadata,
  resolveNightlyTargetVersion,
} from "./resolve-nightly-release.ts";
import {
  parsePrefixedNightlyTag,
  parsePrefixedStableTag,
  resolvePreviousReleaseTag,
} from "./resolve-previous-release-tag.ts";
import { resolveStableVersion } from "./resolve-stable-version.ts";

describe("semver helpers", () => {
  it("strips v prefix and bumps patch", () => {
    expect(stripVersionPrefix("v1.2.3")).toBe("1.2.3");
    expect(bumpPatch("1.2.3")).toBe("1.2.4");
    expect(parseSemverCore("9.9.9-beta.1")).toEqual({
      major: 9,
      minor: 9,
      patch: 9,
    });
  });

  it("derives npm dist tags", () => {
    expect(npmDistTagForVersion("1.2.3")).toBe("latest");
    expect(npmDistTagForVersion("1.2.3-alpha.0")).toBe("alpha");
    expect(npmDistTagForVersion("1.2.3-nightly.20260101.1")).toBe("nightly");
  });
});

describe("nightly resolution", () => {
  it("bumps patch before nightly prerelease", () => {
    expect(resolveNightlyBaseVersion("2.3.0")).toBe("2.3.0");
    expect(resolveNightlyTargetVersion("2.3.0")).toBe("2.3.1");
    expect(
      resolveNightlyReleaseMetadata("2.3.1", "20260718", 12, "abcdef1234567890", {
        product: "symphony",
        tagPrefix: "symphony-v",
      }),
    ).toEqual({
      base_version: "2.3.1",
      version: "2.3.1-nightly.20260718.12",
      tag: "symphony-v2.3.1-nightly.20260718.12",
      name: "Symphony Nightly 2.3.1-nightly.20260718.12 (abcdef123456)",
      short_sha: "abcdef123456",
    });
  });
});

describe("previous tag resolution", () => {
  const tags = [
    "symphony-v2.2.0",
    "symphony-v2.3.0",
    "symphony-v2.3.1-nightly.20260717.1",
    "symphony-v2.3.1-nightly.20260718.2",
    "cli-v0.17.0",
    "pi-symphony-v0.1.2",
  ];

  it("finds previous stable and nightly tags", () => {
    expect(
      resolvePreviousReleaseTag(
        "stable",
        "symphony-v2.3.0",
        tags,
        "symphony-v",
      ),
    ).toBe("symphony-v2.2.0");
    expect(
      resolvePreviousReleaseTag(
        "nightly",
        "symphony-v2.3.1-nightly.20260718.2",
        tags,
        "symphony-v",
      ),
    ).toBe("symphony-v2.3.1-nightly.20260717.1");
  });

  it("parses prefixed tags", () => {
    expect(parsePrefixedStableTag("cli-v0.17.0", "cli-v")).toEqual({
      major: 0,
      minor: 17,
      patch: 0,
      prerelease: [],
    });
    expect(
      parsePrefixedNightlyTag(
        "symphony-v2.3.1-nightly.20260718.2",
        "symphony-v",
      ),
    ).toEqual({
      major: 2,
      minor: 3,
      patch: 1,
      date: 20260718,
      runNumber: 2,
    });
  });
});

describe("stable version resolution", () => {
  it("uses explicit version and dual tags for symphony", () => {
    const resolved = resolveStableVersion({
      inputVersion: "v2.4.0",
      dryRun: false,
      runNumber: 1,
      product: "symphony",
      tags: [],
    });
    expect(resolved.version).toBe("2.4.0");
    expect(resolved.tag).toBe("symphony-v2.4.0");
    expect(resolved.secondary_tag).toBe("pi-symphony-v2.4.0");
    expect(resolved.npm_tag).toBe("latest");
  });

  it("derives from latest nightly when version omitted", () => {
    const resolved = resolveStableVersion({
      dryRun: false,
      runNumber: 1,
      product: "symphony",
      tags: [
        "symphony-v2.3.0",
        "symphony-v2.3.1-nightly.20260718.9",
        "symphony-v2.4.0-nightly.20260710.1",
      ],
    });
    expect(resolved.version).toBe("2.4.0");
    expect(resolved.tag).toBe("symphony-v2.4.0");
    expect(
      extractStableCoreFromNightlyTag(
        "symphony-v2.3.1-nightly.20260718.9",
        "symphony-v",
      ),
    ).toBe("2.3.1");
  });

  it("requires nightly or version for non-dry-run symphony stable", () => {
    expect(() =>
      resolveStableVersion({
        dryRun: false,
        runNumber: 3,
        product: "symphony",
        tags: ["symphony-v2.3.0"],
      }),
    ).toThrow(/Cut a nightly first/);
  });

  it("uses packageVersion for CLI and ignores nightlies", () => {
    const resolved = resolveStableVersion({
      dryRun: false,
      runNumber: 3,
      product: "cli",
      tags: ["cli-v0.17.0", "cli-v0.18.0-nightly.20260718.1"],
      packageVersion: "0.17.0",
    });
    expect(resolved.version).toBe("0.17.0");
    expect(resolved.tag).toBe("cli-v0.17.0");
  });
});
