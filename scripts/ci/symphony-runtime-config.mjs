function nonEmpty(value) {
  const text = String(value ?? "").trim();
  return text || null;
}

function githubCreatedIssues(evidence) {
  return [
    evidence.created?.followupIssue,
    evidence.created?.childIssue,
    evidence.created?.issue,
  ].filter(Boolean);
}

export function sanitizeRuntimeConfig(backend, config) {
  if (backend === "github") {
    const projectNumber = nonEmpty(config.projectNumber);
    return {
      repoOwner: nonEmpty(config.repoOwner),
      repoName: nonEmpty(config.repoName),
      projectOwner: nonEmpty(config.projectOwner),
      projectOwnerType: nonEmpty(config.projectOwnerType),
      projectNumber: projectNumber && Number.isFinite(Number(projectNumber))
        ? Number(projectNumber)
        : null,
      tokenEnv: nonEmpty(config.tokenEnv),
    };
  }
  if (backend === "linear") {
    return {
      projectSlug: nonEmpty(config.projectSlug),
      workspaceSlug: nonEmpty(config.workspaceSlug),
      tokenEnv: nonEmpty(config.tokenEnv),
    };
  }
  throw new Error(`Unsupported evidence backend: ${backend}`);
}

export function parseGithubIssueRepository(url) {
  const value = nonEmpty(url);
  if (!value) return null;
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    return null;
  }
  if (parsed.hostname.toLowerCase() !== "github.com") return null;
  const match = parsed.pathname.match(/^\/([^/]+)\/([^/]+)\/issues\/(\d+)\/?$/);
  if (!match) return null;
  return {
    repoOwner: decodeURIComponent(match[1]),
    repoName: decodeURIComponent(match[2]),
    number: Number(match[3]),
  };
}

function explicitGithubRepository(args) {
  const owner = nonEmpty(args.github_owner);
  const name = nonEmpty(args.github_repo);
  if (Boolean(owner) !== Boolean(name)) {
    throw new Error("GitHub cleanup overrides require both --github-owner and --github-repo");
  }
  return owner && name ? { repoOwner: owner, repoName: name } : null;
}

function evidenceGithubRepository(evidence) {
  const owner = nonEmpty(evidence.config?.repoOwner);
  const name = nonEmpty(evidence.config?.repoName);
  if (Boolean(owner) !== Boolean(name)) {
    throw new Error("Evidence config must contain both repoOwner and repoName");
  }
  return owner && name ? { repoOwner: owner, repoName: name } : null;
}

function createdGithubRepositories(evidence) {
  const issues = githubCreatedIssues(evidence);
  const repositories = new Map();
  for (const issue of issues) {
    // GitHub REST issue records carry an api.github.com `url` and a browser
    // `html_url`; only the latter resolves to a canonical github.com issue URL.
    const url = issue.html_url ?? issue.url;
    const parsed = parseGithubIssueRepository(url);
    if (!parsed) {
      throw new Error("Every created GitHub issue must have a canonical github.com issue URL");
    }
    const key = `${parsed.repoOwner.toLowerCase()}/${parsed.repoName.toLowerCase()}`;
    repositories.set(key, {
      repoOwner: parsed.repoOwner,
      repoName: parsed.repoName,
    });
  }
  if (repositories.size > 1) {
    throw new Error(
      `Created GitHub issues span conflicting repositories: ${[...repositories.keys()].join(", ")}`,
    );
  }
  return repositories.values().next().value ?? null;
}

function sameRepository(left, right) {
  return left.repoOwner.toLowerCase() === right.repoOwner.toLowerCase()
    && left.repoName.toLowerCase() === right.repoName.toLowerCase();
}

export function resolveGithubCleanupConfig({ args = {}, evidence }) {
  const explicit = explicitGithubRepository(args);
  const stored = evidenceGithubRepository(evidence);
  const created = createdGithubRepositories(evidence);
  const resolved = explicit ?? stored ?? created;
  if (!resolved) {
    throw new Error(
      "Unable to resolve GitHub cleanup repository; provide overrides or evidence config",
    );
  }

  for (const [label, candidate] of [["evidence config", stored], ["created issue URLs", created]]) {
    if (candidate && !sameRepository(resolved, candidate)) {
      throw new Error(
        `GitHub cleanup repository ${resolved.repoOwner}/${resolved.repoName} conflicts with ${label} ${candidate.repoOwner}/${candidate.repoName}`,
      );
    }
  }

  return {
    ...sanitizeRuntimeConfig("github", evidence.config ?? {}),
    repoOwner: resolved.repoOwner,
    repoName: resolved.repoName,
  };
}

export function validateGithubCleanupTargets({ evidence, config }) {
  return githubCreatedIssues(evidence).map((issue) => {
    const parsed = parseGithubIssueRepository(issue.html_url ?? issue.url);
    if (!parsed) {
      throw new Error("Every created GitHub issue must have a canonical github.com issue URL");
    }
    if (!sameRepository(config, parsed)) {
      throw new Error(
        `Created issue ${parsed.repoOwner}/${parsed.repoName}#${parsed.number} does not match cleanup repository ${config.repoOwner}/${config.repoName}`,
      );
    }
    const recordedNumber = Number(issue.number ?? issue.id);
    if (Number.isFinite(recordedNumber) && recordedNumber !== parsed.number) {
      throw new Error(
        `Created issue number ${recordedNumber} does not match its URL number ${parsed.number}`,
      );
    }
    return { issue, number: parsed.number };
  });
}
