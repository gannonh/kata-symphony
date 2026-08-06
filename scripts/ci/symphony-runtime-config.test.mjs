import assert from "node:assert/strict";
import test from "node:test";
import {
  resolveGithubCleanupConfig,
  sanitizeRuntimeConfig,
  validateGithubCleanupTargets,
} from "./symphony-runtime-config.mjs";

const githubIssue = (owner, repo, number) => ({
  number,
  title: `Symphony Backend UAT 20260725190000 #${number}`,
  // GitHub REST issue records: `url` points at api.github.com and `html_url`
  // at the browser issue page. Cleanup resolution must prefer `html_url`.
  url: `https://api.github.com/repos/${owner}/${repo}/issues/${number}`,
  html_url: `https://github.com/${owner}/${repo}/issues/${number}`,
});

test("sanitized GitHub evidence records coordinates and token env name only", () => {
  const config = sanitizeRuntimeConfig("github", {
    repoOwner: "gannonh",
    repoName: "uat-symphony",
    projectOwner: "gannonh",
    projectOwnerType: "user",
    projectNumber: 16,
    tokenEnv: "GH_TOKEN",
    token: "must-not-serialize",
  });

  assert.deepEqual(config, {
    repoOwner: "gannonh",
    repoName: "uat-symphony",
    projectOwner: "gannonh",
    projectOwnerType: "user",
    projectNumber: 16,
    tokenEnv: "GH_TOKEN",
  });
  assert.equal(JSON.stringify(config).includes("must-not-serialize"), false);
});

test("sanitized Linear evidence records slugs and token env name only", () => {
  assert.deepEqual(
    sanitizeRuntimeConfig("linear", {
      projectSlug: "kata",
      workspaceSlug: "kata-sh",
      tokenEnv: "LINEAR_API_KEY",
      token: "must-not-serialize",
    }),
    {
      projectSlug: "kata",
      workspaceSlug: "kata-sh",
      tokenEnv: "LINEAR_API_KEY",
    },
  );
});

test("cleanup resolves the repository from stored evidence config", () => {
  const evidence = {
    config: { repoOwner: "gannonh", repoName: "uat-symphony", projectNumber: 16 },
    created: { issue: githubIssue("gannonh", "uat-symphony", 21) },
  };

  const config = resolveGithubCleanupConfig({ evidence });
  assert.equal(config.repoOwner, "gannonh");
  assert.equal(config.repoName, "uat-symphony");
  assert.equal(config.projectNumber, 16);
});

test("legacy cleanup derives a unanimous repository from created issue URLs", () => {
  const evidence = {
    created: {
      issue: githubIssue("gannonh", "uat-symphony", 21),
      childIssue: githubIssue("gannonh", "uat-symphony", 22),
    },
  };

  assert.deepEqual(resolveGithubCleanupConfig({ evidence }), {
    repoOwner: "gannonh",
    repoName: "uat-symphony",
    projectOwner: null,
    projectOwnerType: null,
    projectNumber: null,
    tokenEnv: null,
  });
});

test("cleanup rejects created issues from conflicting repositories", () => {
  const evidence = {
    created: {
      issue: githubIssue("gannonh", "uat-symphony", 21),
      childIssue: githubIssue("gannonh", "kata-symphony", 22),
    },
  };

  assert.throws(
    () => resolveGithubCleanupConfig({ evidence }),
    /span conflicting repositories/,
  );
});

test("cleanup rejects explicit overrides that conflict with evidence", () => {
  const evidence = {
    config: { repoOwner: "gannonh", repoName: "uat-symphony" },
    created: { issue: githubIssue("gannonh", "uat-symphony", 21) },
  };

  assert.throws(
    () => resolveGithubCleanupConfig({
      args: { github_owner: "gannonh", github_repo: "kata-symphony" },
      evidence,
    }),
    /conflicts with evidence config/,
  );
});

test("all cleanup targets are validated before provider work can begin", () => {
  const evidence = {
    config: { repoOwner: "gannonh", repoName: "uat-symphony" },
    created: {
      issue: githubIssue("gannonh", "uat-symphony", 21),
      childIssue: githubIssue("someone", "elsewhere", 22),
    },
  };
  let providerCalls = 0;

  assert.throws(() => {
    const config = {
      repoOwner: evidence.config.repoOwner,
      repoName: evidence.config.repoName,
    };
    validateGithubCleanupTargets({ evidence, config });
    providerCalls += 1;
  }, /does not match cleanup repository/);
  assert.equal(providerCalls, 0);
});

test("cleanup rejects issue numbers that disagree with canonical URLs", () => {
  const evidence = {
    created: {
      issue: {
        ...githubIssue("gannonh", "uat-symphony", 21),
        number: 99,
      },
    },
  };
  const config = resolveGithubCleanupConfig({ evidence });

  assert.throws(
    () => validateGithubCleanupTargets({ evidence, config }),
    /does not match its URL number/,
  );
});

test("ambiguous evidence without config or created records fails clearly", () => {
  assert.throws(
    () => resolveGithubCleanupConfig({ evidence: { created: {} } }),
    /Unable to resolve GitHub cleanup repository/,
  );
});

test("dry-run evidence uses the same sanitized config shape", () => {
  const evidence = {
    mode: "dry-run",
    config: sanitizeRuntimeConfig("github", {
      repoOwner: "gannonh",
      repoName: "uat-symphony",
      projectOwner: "gannonh",
      projectOwnerType: "user",
      projectNumber: 16,
      tokenEnv: "GH_TOKEN",
    }),
  };

  assert.equal(evidence.mode, "dry-run");
  assert.equal(evidence.config.repoName, "uat-symphony");
  assert.deepEqual(Object.keys(evidence.config).sort(), [
    "projectNumber",
    "projectOwner",
    "projectOwnerType",
    "repoName",
    "repoOwner",
    "tokenEnv",
  ]);
});
