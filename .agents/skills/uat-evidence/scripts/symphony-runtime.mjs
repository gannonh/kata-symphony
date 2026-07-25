#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  resolveGithubCleanupConfig,
  sanitizeRuntimeConfig,
  validateGithubCleanupTargets,
} from "./symphony-runtime-config.mjs";

const SKILL_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PROMPT_FILES = [
  "apps/symphony/prompts/system.md",
  "apps/symphony/prompts/in-progress.md",
  "apps/symphony/prompts/rework.md",
  "apps/symphony/prompts/agent-review.md",
  "apps/symphony/prompts/merging.md",
];
const UAT_TITLE_MARKER = "Symphony Backend UAT";
const UAT_FOLLOWUP_MARKER = "Symphony follow-up";

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const command = args._[0] ?? "help";

  if (command === "help" || args.help) {
    printHelp();
    return;
  }
  if (command === "update") {
    updateGeneratedContract(args);
    return;
  }
  if (command === "cleanup") {
    await cleanupRun(args);
    return;
  }
  if (command === "test") {
    await testBackend(args);
    return;
  }

  throw new Error(`Unknown command: ${command}`);
}

function printHelp() {
  console.log(`uat-evidence symphony-runtime runner

Commands:
  test --backend github|linear [--workspace path] [--symphony-root path] [--output-dir path] [--dry-run]
  update [--workspace path] [--symphony-root path]
  cleanup --evidence /path/to/evidence.json
`);
}

function parseArgs(argv) {
  const result = { _: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (!arg.startsWith("--")) {
      result._.push(arg);
      continue;
    }
    const stripped = arg.slice(2);
    const eqIndex = stripped.indexOf("=");
    const rawKey = eqIndex === -1 ? stripped : stripped.slice(0, eqIndex);
    const inlineValue = eqIndex === -1 ? undefined : stripped.slice(eqIndex + 1);
    const key = rawKey.replaceAll("-", "_");
    if (inlineValue !== undefined) {
      result[key] = inlineValue;
    } else if (argv[index + 1] && !argv[index + 1].startsWith("--")) {
      result[key] = argv[index + 1];
      index += 1;
    } else {
      result[key] = true;
    }
  }
  return result;
}

async function testBackend(args) {
  const backend = String(args.backend ?? "").trim();
  if (backend !== "github" && backend !== "linear") {
    throw new Error("test requires --backend github or --backend linear");
  }

  const workspace = path.resolve(String(args.workspace ?? process.cwd()));
  const symphonyRoot = resolveSymphonyRoot(args, workspace);
  const helperContract = loadHelperContract({ workspace, symphonyRoot, allowGeneratedFallback: true });
  const stamp = timestamp();
  const runDir = path.resolve(String(args.output_dir ?? defaultUatRunDir({ workspace, runtime: "symphony-runtime", backend, stamp })));
  const env = loadEnv(workspace, { ...process.env });
  mkdirSync(path.join(runDir, "payloads"), { recursive: true });
  mkdirSync(path.join(runDir, "workspaces"), { recursive: true });

  const config = backend === "github"
    ? githubConfig(args, workspace, env)
    : await linearConfig(args, workspace, env);
  const workflowPath = path.join(runDir, "WORKFLOW.md");
  const binaryPath = args.binary
    ? path.resolve(String(args.binary))
    : defaultSymphonyBinaryPath({ workspace, symphonyRoot });

  const evidence = {
    runtime: "symphony-runtime",
    backend,
    mode: args.dry_run ? "dry-run" : "real",
    config: sanitizeRuntimeConfig(backend, config),
    stamp,
    workspace,
    symphonyRoot,
    runDir,
    binaryPath,
    gitCommit: gitCommit(workspace),
    workflowFixture: workflowPath,
    helperContractSource: helperContract.source,
    expectedOperations: expectedOperationsFor(backend, helperContract),
    observedOperations: [],
    helperPayloads: [],
    providerProofLinks: [],
    created: {},
    skips: [],
    health: { ok: false, skipped: Boolean(args.dry_run) },
    cleanup: { completed: false, attempted: false },
  };

  writeWorkflowFixture({ backend, config, workspace, runDir, workflowPath });

  try {
    if (args.dry_run) {
      evidence.health = { ok: true, dryRun: true };
      evidence.operationCoverage = {
        expected: evidence.expectedOperations,
        observed: [],
        missing: evidence.expectedOperations,
      };
      writeEvidence(runDir, evidence);
      console.log(JSON.stringify(resultSummary(evidence), null, 2));
      return;
    }

    const resolvedBinary = args.binary ? binaryPath : buildSymphonyBinary({ workspace, symphonyRoot });
    evidence.binaryPath = resolvedBinary;
    evidence.health = runDoctor({ binaryPath: resolvedBinary, workflowPath, workspace, env });
    if (!evidence.health.ok) {
      throw new Error(`symphony doctor failed: ${evidence.health.stderr || evidence.health.stdout}`);
    }

    const providerState = backend === "github"
      ? await createGithubProviderState({ env, config, evidence })
      : await createLinearProviderState({ env, config, evidence });
    evidence.created = { ...evidence.created, ...providerState.created };
    evidence.providerProofLinks.push(...providerState.links);

    const caller = createHelperCaller({ binaryPath: resolvedBinary, workflowPath, workspace, env, runDir, evidence });
    const parentIssueId = providerState.parentIssueId;
    const childIssueId = providerState.childIssueId;
    const docTitle = `Symphony Backend UAT ${evidence.stamp}`;

    await caller("issue.get", { issueId: parentIssueId, includeChildren: true, includeComments: true });
    await caller("issue.list-children", { issueId: parentIssueId });
    await caller("comment.upsert", {
      issueId: parentIssueId,
      marker: "## Symphony Backend UAT",
      body: `## Symphony Backend UAT\n\nProof run ${evidence.stamp}.`,
    });
    await caller("document.write", {
      issueId: parentIssueId,
      title: docTitle,
      content: `# ${docTitle}\n\nBackend: ${backend}\nChild: ${childIssueId ?? "none"}`,
    });
    await caller("document.read", { issueId: parentIssueId, title: docTitle });
    await caller("issue.update-state", { issueId: parentIssueId, state: "In Progress" });
    const followup = await caller("issue.create-followup", {
      parentIssueId,
      title: `Symphony follow-up ${evidence.stamp}`,
      description: `Follow-up generated by Symphony backend UAT ${evidence.stamp}.`,
    });
    recordCreatedFollowup(evidence, backend, followup);

    if (backend === "github") {
      await runGithubPrHelpers({ caller, evidence, env, operations: helperContract.githubOnlyHelperOperations });
    }

    const providerRead = backend === "github"
      ? await readGithubProviderState({ env, config, parentIssueId })
      : await readLinearProviderState({ env, parentIssueId });
    evidence.providerRead = providerRead;
    evidence.providerProofLinks.push(...providerRead.links);

    const observed = [...new Set(evidence.observedOperations.map((entry) => entry.operation))];
    const skipped = skippedOperationsFor({ backend, helperContract, evidence });
    const missing = evidence.expectedOperations.filter((operation) => !observed.includes(operation) && !skipped.includes(operation));
    evidence.operationCoverage = { expected: evidence.expectedOperations, observed, skipped, missing };
    if (missing.length > 0) {
      throw new Error(`Missing helper operation coverage: ${missing.join(", ")}`);
    }

    writeEvidence(runDir, evidence);
    console.log(JSON.stringify(resultSummary(evidence), null, 2));
  } catch (error) {
    evidence.failure = String(error instanceof Error ? error.message : error);
    evidence.operationCoverage ??= coverageForFailure({ backend, helperContract, evidence });
    writeEvidence(runDir, evidence);
    throw error;
  }
}

function buildSymphonyBinary({ workspace, symphonyRoot }) {
  const manifestPath = path.relative(workspace, path.join(symphonyRoot, "Cargo.toml"));
  const result = spawnSync("cargo", ["build", "--manifest-path", manifestPath], {
    cwd: workspace,
    encoding: "utf8",
    maxBuffer: 1024 * 1024 * 16,
  });
  if (result.status !== 0) {
    throw new Error(`cargo build failed: ${result.stderr || result.stdout}`);
  }
  return defaultSymphonyBinaryPath({ workspace, symphonyRoot });
}

function defaultSymphonyBinaryPath({ workspace, symphonyRoot }) {
  const workspaceBinary = path.join(workspace, "target", "debug", "symphony");
  if (existsSync(workspaceBinary)) return workspaceBinary;
  return path.join(symphonyRoot, "target", "debug", "symphony");
}

function runDoctor({ binaryPath, workflowPath, workspace, env }) {
  const result = spawnSync(binaryPath, ["doctor", workflowPath], {
    cwd: workspace,
    env,
    encoding: "utf8",
    maxBuffer: 1024 * 1024 * 16,
  });
  return {
    ok: result.status === 0,
    status: result.status,
    stdout: result.stdout,
    stderr: result.stderr,
  };
}

function createHelperCaller({ binaryPath, workflowPath, workspace, env, runDir, evidence }) {
  return async function callHelper(operation, payload = {}) {
    const inputPath = path.join(
      runDir,
      "payloads",
      `${operation.replaceAll(".", "-")}-${Date.now()}-${Math.random().toString(16).slice(2)}.json`,
    );
    writeFileSync(inputPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
    evidence.helperPayloads.push({ operation, inputPath, payload });

    const result = spawnSync(binaryPath, [
      "helper",
      operation,
      "--workflow",
      workflowPath,
      "--input",
      inputPath,
    ], {
      cwd: workspace,
      env,
      encoding: "utf8",
      maxBuffer: 1024 * 1024 * 16,
    });
    const parsed = parseJson(result.stdout);
    if (result.status !== 0 || parsed?.ok !== true) {
      const message = parsed?.error?.message ?? result.stderr ?? result.stdout;
      throw new Error(`${operation} failed: ${String(message).slice(0, 1200)}`);
    }
    evidence.observedOperations.push({
      operation,
      inputPath,
      summary: summarizeHelperResult(parsed.data),
    });
    return parsed.data;
  };
}

async function createGithubProviderState({ env, config, evidence }) {
  const repo = `${config.repoOwner}/${config.repoName}`;
  const title = `${UAT_TITLE_MARKER} ${evidence.stamp}`;
  const parent = createGithubIssue({
    env,
    config,
    repo,
    title,
    body: `Parent issue for ${UAT_TITLE_MARKER} ${evidence.stamp}.`,
  });
  const child = createGithubIssue({
    env,
    config,
    repo,
    title: `${title} child`,
    body: `Child issue for ${UAT_TITLE_MARKER} ${evidence.stamp}.`,
  });

  const created = { issue: parent, childIssue: child };
  const links = [
    { label: "github-parent-issue", url: parent.url },
    { label: "github-child-issue", url: child.url },
  ];

  try {
    addGithubIssueToProject({ env, config, issue: parent, state: "Todo" });
    addGithubIssueToProject({ env, config, issue: child, state: "Todo" });
  } catch (error) {
    evidence.skips.push({ action: "github-project-add", reason: String(error instanceof Error ? error.message : error) });
  }

  try {
    const linked = ghApi(env, [
      "--method",
      "POST",
      `/repos/${config.repoOwner}/${config.repoName}/issues/${parent.number}/sub_issues`,
      "-F",
      `sub_issue_id=${child.id}`,
    ]);
    created.childLink = linked;
  } catch (error) {
    evidence.skips.push({ action: "github-sub-issue-link", reason: String(error instanceof Error ? error.message : error) });
  }

  return {
    parentIssueId: String(parent.number),
    childIssueId: String(child.number),
    created,
    links,
  };
}

function createGithubIssue({ env, config, repo, title, body }) {
  const output = ghText(env, ["issue", "create", "--repo", repo, "--title", title, "--body", body]).trim();
  const number = Number(output.match(/\/issues\/(\d+)/)?.[1]);
  if (!Number.isFinite(number)) {
    throw new Error(`Unable to parse issue number from gh issue create output: ${output}`);
  }
  const issue = ghApi(env, [`/repos/${config.repoOwner}/${config.repoName}/issues/${number}`]);
  return {
    id: issue.id,
    node_id: issue.node_id,
    number: issue.number,
    url: issue.html_url,
    apiUrl: issue.url,
    title: issue.title,
  };
}

function addGithubIssueToProject({ env, config, issue, state }) {
  if (!config.projectNumber) return;
  const owner = config.projectOwner || config.repoOwner;
  const project = ghJson(env, ["project", "view", String(config.projectNumber), "--owner", owner, "--format", "json"]);
  const added = ghJson(env, [
    "project",
    "item-add",
    String(config.projectNumber),
    "--owner",
    owner,
    "--url",
    issue.url,
    "--format",
    "json",
  ]);
  const fields = ghJson(env, ["project", "field-list", String(config.projectNumber), "--owner", owner, "--format", "json"]);
  const statusField = (fields.fields ?? fields.items ?? []).find((field) => field.name === "Status");
  const option = (statusField?.options ?? []).find((candidate) => candidate.name === state);
  const itemId = added.id || added.item?.id;
  const projectId = project.id || project.project?.id;
  if (!statusField?.id || !option?.id || !itemId || !projectId) return;
  ghJson(env, [
    "project",
    "item-edit",
    "--id",
    itemId,
    "--project-id",
    projectId,
    "--field-id",
    statusField.id,
    "--single-select-option-id",
    option.id,
    "--format",
    "json",
  ]);
}

async function createLinearProviderState({ env, config, evidence }) {
  const context = await resolveLinearProjectContext(env, config);
  const parent = await linearGraphql(env, `
    mutation SymphonyBackendUatCreateParent($teamId: String!, $projectId: String!, $title: String!, $description: String!) {
      issueCreate(input: { teamId: $teamId, projectId: $projectId, title: $title, description: $description }) {
        success
        issue { id identifier title url }
      }
    }
  `, {
    teamId: context.teamId,
    projectId: context.projectId,
    title: `${UAT_TITLE_MARKER} ${evidence.stamp}`,
    description: `Parent issue for ${UAT_TITLE_MARKER} ${evidence.stamp}.`,
  });
  const parentIssue = parent.issueCreate?.issue;
  if (!parent.issueCreate?.success || !parentIssue?.id) {
    throw new Error(`Linear parent issue creation failed: ${JSON.stringify(parent).slice(0, 1000)}`);
  }

  const child = await linearGraphql(env, `
    mutation SymphonyBackendUatCreateChild($teamId: String!, $projectId: String!, $parentId: String!, $title: String!, $description: String!) {
      issueCreate(input: { teamId: $teamId, projectId: $projectId, parentId: $parentId, title: $title, description: $description }) {
        success
        issue { id identifier title url }
      }
    }
  `, {
    teamId: context.teamId,
    projectId: context.projectId,
    parentId: parentIssue.id,
    title: `${UAT_TITLE_MARKER} child ${evidence.stamp}`,
    description: `Child issue for ${UAT_TITLE_MARKER} ${evidence.stamp}.`,
  });
  const childIssue = child.issueCreate?.issue;
  if (!child.issueCreate?.success || !childIssue?.id) {
    throw new Error(`Linear child issue creation failed: ${JSON.stringify(child).slice(0, 1000)}`);
  }

  return {
    parentIssueId: parentIssue.id,
    childIssueId: childIssue.id,
    created: { issue: parentIssue, childIssue },
    links: [
      { label: "linear-parent-issue", url: parentIssue.url },
      { label: "linear-child-issue", url: childIssue.url },
    ],
  };
}

async function resolveLinearProjectContext(env, config) {
  const data = await linearGraphql(env, `
    query SymphonyBackendUatProject($slug: String!) {
      projects(filter: { slugId: { eq: $slug } }, first: 1) {
        nodes {
          id
          slugId
          teams(first: 1) { nodes { id key name } }
        }
      }
    }
  `, { slug: config.projectSlug });
  const project = data.projects?.nodes?.[0];
  const team = project?.teams?.nodes?.[0];
  if (!project?.id || !team?.id) {
    throw new Error(`Unable to resolve Linear project/team for slug ${config.projectSlug}`);
  }
  return { projectId: project.id, teamId: team.id };
}

function coverageForFailure({ backend, helperContract, evidence }) {
  const observed = [...new Set(evidence.observedOperations.map((entry) => entry.operation))];
  const skipped = skippedOperationsFor({ backend, helperContract, evidence });
  return {
    expected: evidence.expectedOperations,
    observed,
    skipped,
    missing: evidence.expectedOperations.filter((operation) => !observed.includes(operation) && !skipped.includes(operation)),
  };
}

function skippedOperationsFor({ backend, helperContract, evidence }) {
  if (backend !== "github") return [];
  const prSkip = evidence.skips.some((skip) => skip.action === "github-pr-helpers");
  return prSkip ? helperContract.githubOnlyHelperOperations : [];
}

async function runGithubPrHelpers({ caller, evidence, env, operations }) {
  const prView = spawnSync("gh", ["pr", "view", "--json", "number,url"], {
    cwd: evidence.workspace,
    env,
    encoding: "utf8",
    maxBuffer: 1024 * 1024 * 4,
  });
  if (prView.status !== 0) {
    evidence.skips.push({
      action: "github-pr-helpers",
      reason: `gh pr view failed: ${String(prView.stderr || prView.stdout).trim() || "no PR in current checkout"}`,
    });
    return;
  }
  const pr = parseJson(prView.stdout);
  if (!pr?.number) {
    evidence.skips.push({ action: "github-pr-helpers", reason: "gh pr view did not return a PR number" });
    return;
  }
  for (const operation of operations) {
    await caller(operation, operation === "pr.inspect-checks"
      ? { pr: pr.number, includeLogs: false }
      : { pr: pr.number });
  }
  evidence.providerProofLinks.push({ label: "github-pr", url: pr.url });
}

async function readGithubProviderState({ env, config, parentIssueId }) {
  const issue = ghApi(env, [`/repos/${config.repoOwner}/${config.repoName}/issues/${parentIssueId}`]);
  const comments = ghApi(env, [`/repos/${config.repoOwner}/${config.repoName}/issues/${parentIssueId}/comments`]);
  let children = [];
  try {
    children = ghApi(env, [`/repos/${config.repoOwner}/${config.repoName}/issues/${parentIssueId}/sub_issues`]);
  } catch {
    children = [];
  }
  return {
    issue,
    commentsCount: Array.isArray(comments) ? comments.length : 0,
    childrenCount: Array.isArray(children) ? children.length : 0,
    links: [
      { label: "github-provider-read-issue", url: issue.html_url },
      ...comments
        .filter((comment) => String(comment.body ?? "").includes("Symphony Backend UAT") || String(comment.body ?? "").includes("symphony:document"))
        .map((comment) => ({ label: "github-provider-read-comment", url: comment.html_url })),
    ],
  };
}

async function readLinearProviderState({ env, parentIssueId }) {
  const data = await linearGraphql(env, `
    query SymphonyBackendUatRead($issueId: String!) {
      issue(id: $issueId) {
        id
        identifier
        title
        url
        children { nodes { id identifier url } }
        comments(first: 50) { nodes { id body url } }
      }
    }
  `, { issueId: parentIssueId });
  const issue = data.issue;
  const comments = issue?.comments?.nodes ?? [];
  return {
    issue,
    commentsCount: comments.length,
    childrenCount: issue?.children?.nodes?.length ?? 0,
    links: [
      { label: "linear-provider-read-issue", url: issue?.url },
      ...comments
        .filter((comment) => String(comment.body ?? "").includes("Symphony Backend UAT") || String(comment.body ?? "").includes("symphony:document"))
        .map((comment) => ({ label: "linear-provider-read-comment", url: comment.url })),
    ].filter((entry) => entry.url),
  };
}

async function cleanupRun(args) {
  const evidencePath = args.evidence ? path.resolve(String(args.evidence)) : null;
  if (!evidencePath || !existsSync(evidencePath)) {
    throw new Error("cleanup requires --evidence /path/to/evidence.json");
  }
  const evidence = JSON.parse(readFileSync(evidencePath, "utf8"));
  const workspace = path.resolve(String(args.workspace ?? evidence.workspace ?? process.cwd()));
  const env = loadEnv(workspace, { ...process.env });
  const cleanup = { attempted: true, completed: false, actions: [], failures: [], skipped: [], notes: [] };
  const createdIssues = [evidence.created?.followupIssue, evidence.created?.childIssue, evidence.created?.issue].filter(Boolean);
  if (createdIssues.length === 0) {
    cleanup.notes.push("no created state found");
    cleanup.completed = true;
    evidence.cleanup = cleanup;
    writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`, "utf8");
    const reportPath = path.join(path.dirname(evidencePath), "evidence.md");
    writeFileSync(reportPath, evidenceMarkdown(evidence, evidencePath), "utf8");
    console.log(JSON.stringify({ ok: true, evidence: evidencePath, cleanup }, null, 2));
    return;
  }

  if (evidence.backend === "github") {
    // Resolve and validate the complete target set before the first provider
    // read or mutation. Cleanup must never inherit an ambient workflow repo.
    const config = resolveGithubCleanupConfig({ args, evidence });
    const targets = validateGithubCleanupTargets({ evidence, config });
    cleanup.config = sanitizeRuntimeConfig("github", config);
    if (args.dry_run) {
      cleanup.actions = targets.map(({ number }) => ({
        provider: "github",
        issue: number,
        action: "would-close",
      }));
      cleanup.completed = true;
      evidence.cleanup = cleanup;
      writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`, "utf8");
      const reportPath = path.join(path.dirname(evidencePath), "evidence.md");
      writeFileSync(reportPath, evidenceMarkdown(evidence, evidencePath), "utf8");
      console.log(JSON.stringify({ ok: true, dryRun: true, evidence: evidencePath, cleanup }, null, 2));
      return;
    }
    for (const { issue, number } of targets) {
      const verified = verifyGithubCleanupIssue({ env, config, evidence, issue });
      if (!verified.ok) {
        cleanup.skipped.push({ provider: "github", issue: number, reason: verified.reason });
        continue;
      }
      try {
        const result = ghText(env, ["issue", "close", String(number), "--repo", `${config.repoOwner}/${config.repoName}`, "--comment", "Closed by Symphony backend UAT cleanup."]);
        cleanup.actions.push({ provider: "github", issue: number, result: result.trim() });
      } catch (error) {
        cleanup.failures.push({ provider: "github", issue, error: String(error instanceof Error ? error.message : error) });
      }
    }
  } else if (evidence.backend === "linear") {
    for (const issue of createdIssues) {
      const verified = await verifyLinearCleanupIssue({ env, evidence, issue });
      if (!verified.ok) {
        cleanup.skipped.push({ provider: "linear", issue: issue.id, reason: verified.reason });
        continue;
      }
      try {
        await completeLinearIssue(env, issue.id);
        cleanup.actions.push({ provider: "linear", issue: issue.id, state: "Done" });
      } catch (error) {
        cleanup.failures.push({ provider: "linear", issue, error: String(error instanceof Error ? error.message : error) });
      }
    }
  } else {
    throw new Error(`Unsupported evidence backend: ${evidence.backend}`);
  }

  cleanup.completed = cleanup.failures.length === 0 && cleanup.skipped.length === 0;
  evidence.cleanup = cleanup;
  writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`, "utf8");
  const reportPath = path.join(path.dirname(evidencePath), "evidence.md");
  writeFileSync(reportPath, evidenceMarkdown(evidence, evidencePath), "utf8");
  console.log(JSON.stringify({ ok: cleanup.completed, evidence: evidencePath, cleanup }, null, 2));
}

function verifyGithubCleanupIssue({ env, config, evidence, issue }) {
  const stampCheck = validCleanupStamp(evidence);
  if (!stampCheck.ok) return stampCheck;
  const evidenceCheck = createdIssueLooksLikeUat(issue, evidence);
  if (!evidenceCheck.ok) return evidenceCheck;
  try {
    const number = issue.number ?? issue.id;
    const providerIssue = ghApi(env, [`/repos/${config.repoOwner}/${config.repoName}/issues/${number}`]);
    return providerIssueLooksLikeUat(providerIssue, evidence);
  } catch (error) {
    return {
      ok: false,
      reason: `provider verification unavailable: ${String(error instanceof Error ? error.message : error)}`,
    };
  }
}

async function verifyLinearCleanupIssue({ env, evidence, issue }) {
  const stampCheck = validCleanupStamp(evidence);
  if (!stampCheck.ok) return stampCheck;
  const evidenceCheck = createdIssueLooksLikeUat(issue, evidence);
  if (!evidenceCheck.ok) return evidenceCheck;
  try {
    const data = await linearGraphql(env, `
      query SymphonyBackendUatCleanupIssue($issueId: String!) {
        issue(id: $issueId) {
          id
          identifier
          title
          description
          url
        }
      }
    `, { issueId: issue.id });
    return providerIssueLooksLikeUat(data.issue, evidence);
  } catch (error) {
    return {
      ok: false,
      reason: `provider verification unavailable: ${String(error instanceof Error ? error.message : error)}`,
    };
  }
}

function validCleanupStamp(evidence) {
  if (!/^\d{14}$/.test(String(evidence.stamp ?? ""))) {
    return { ok: false, reason: "evidence is missing a valid UAT stamp" };
  }
  return { ok: true };
}

function createdIssueLooksLikeUat(issue, evidence) {
  const text = [
    issue?.title,
    issue?.body,
    issue?.description,
    issue?.url,
    issue?.html_url,
  ].filter(Boolean).join("\n");
  return uatTextCheck(text, evidence, "created evidence record");
}

function providerIssueLooksLikeUat(issue, evidence) {
  if (!issue) return { ok: false, reason: "provider record was not found" };
  const text = [
    issue.title,
    issue.body,
    issue.description,
    issue.url,
    issue.html_url,
  ].filter(Boolean).join("\n");
  return uatTextCheck(text, evidence, "provider record");
}

function uatTextCheck(text, evidence, label) {
  if (!text.includes(String(evidence.stamp))) {
    return { ok: false, reason: `${label} does not include UAT stamp ${evidence.stamp}` };
  }
  if (!text.includes(UAT_TITLE_MARKER) && !text.includes(UAT_FOLLOWUP_MARKER)) {
    return { ok: false, reason: `${label} does not include a Symphony UAT marker` };
  }
  return { ok: true };
}

async function completeLinearIssue(env, issueId) {
  const data = await linearGraphql(env, `
    query SymphonyBackendUatState($issueId: String!, $stateName: String!) {
      issue(id: $issueId) {
        team { states(filter: { name: { eq: $stateName } }, first: 1) { nodes { id name } } }
      }
    }
  `, { issueId, stateName: "Done" });
  const stateId = data.issue?.team?.states?.nodes?.[0]?.id;
  if (!stateId) throw new Error(`Done state not found for Linear issue ${issueId}`);
  await linearGraphql(env, `
    mutation SymphonyBackendUatDone($issueId: String!, $stateId: String!) {
      issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { id url } }
    }
  `, { issueId, stateId });
}

function updateGeneratedContract(args) {
  const workspace = path.resolve(String(args.workspace ?? process.cwd()));
  const symphonyRoot = resolveSymphonyRoot(args, workspace);
  const contractFromSource = readHelperContractFromSource(symphonyRoot);
  const promptHelperOperations = {};
  for (const promptFile of PROMPT_FILES) {
    const fullPath = path.join(workspace, promptFile);
    if (!existsSync(fullPath)) {
      throw new Error(`Expected Symphony prompt file is missing: ${fullPath}`);
    }
    const content = readFileSync(fullPath, "utf8");
    promptHelperOperations[promptFile] = [...content.matchAll(/\$SYMPHONY_BIN"\s+helper\s+([a-z0-9.-]+)/g)].map((match) => match[1]);
  }
  const contract = {
    generatedAt: new Date().toISOString(),
    workspace,
    symphonyRoot,
    gitCommit: gitCommit(workspace),
    sharedHelperOperations: contractFromSource.sharedHelperOperations,
    githubOnlyHelperOperations: contractFromSource.githubOnlyHelperOperations,
    backends: ["github", "linear"],
    promptFiles: PROMPT_FILES,
    promptHelperOperations,
  };
  const outputPath = path.join(SKILL_ROOT, "references", "generated-symphony-contract.json");
  writeFileSync(outputPath, `${JSON.stringify(contract, null, 2)}\n`, "utf8");
  console.log(JSON.stringify({ ok: true, outputPath, contract }, null, 2));
}

function readHelperContractFromSource(symphonyRoot) {
  const helperPath = path.join(symphonyRoot, "src", "helper.rs");
  if (!existsSync(helperPath)) {
    throw new Error(`Expected Symphony helper source is missing: ${helperPath}`);
  }
  return {
    source: "helper.rs",
    helperPath,
    sharedHelperOperations: parseRustStringArray(helperPath, "SHARED_HELPER_OPERATIONS"),
    githubOnlyHelperOperations: parseRustStringArray(helperPath, "GITHUB_ONLY_HELPER_OPERATIONS"),
  };
}

function parseRustStringArray(filePath, constName) {
  const content = readFileSync(filePath, "utf8");
  const match = content.match(new RegExp(`pub\\s+const\\s+${constName}[^=]*=\\s*(?:&)?\\[([\\s\\S]*?)\\];`));
  if (!match) {
    throw new Error(`Unable to parse ${constName} in ${filePath}`);
  }
  const operations = [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
  if (operations.length === 0) {
    throw new Error(`${constName} in ${filePath} did not contain any operations`);
  }
  return operations;
}

function writeWorkflowFixture({ backend, config, workspace, runDir, workflowPath }) {
  const githubApiKeyLine = config.tokenEnv ? `  api_key: $${config.tokenEnv}\n` : "";
  const tracker = backend === "github"
    ? `tracker:
  kind: github
${githubApiKeyLine}  repo_owner: ${config.repoOwner}
  repo_name: ${config.repoName}
  github_project_owner_type: ${config.projectOwnerType}
  github_project_number: ${config.projectNumber}
  active_states:
    - Todo
    - In Progress
    - Done
  terminal_states:
    - Done`
    : `tracker:
  kind: linear
  api_key: $${config.tokenEnv}
  project_slug: ${config.projectSlug}
  workspace_slug: ${config.workspaceSlug}
  active_states:
    - Todo
    - In Progress
    - Done
  terminal_states:
    - Done`;

  writeFileSync(workflowPath, `---
${tracker}
workspace:
  root: ${path.join(runDir, "workspaces")}
  repo: ${workspace}
  git_strategy: auto
  isolation: local
agent:
  name: pi
  command: "true"
  no_session: true
polling:
  interval_ms: 60000
---
# Symphony Backend UAT
`, "utf8");
}

function githubConfig(args, workspace, env) {
  const workflow = readWorkflowConfig(path.join(workspace, ".symphony", "WORKFLOW.md"));
  return {
    repoOwner: String(args.github_owner ?? workflow.repo_owner ?? "gannonh"),
    repoName: String(args.github_repo ?? workflow.repo_name ?? "kata"),
    projectOwner: String(args.github_project_owner ?? workflow.github_project_owner ?? workflow.repo_owner ?? "gannonh"),
    projectOwnerType: String(args.github_project_owner_type ?? workflow.github_project_owner_type ?? "user"),
    projectNumber: Number(args.github_project_number ?? workflow.github_project_number ?? 17),
    tokenEnv: env.GH_TOKEN ? "GH_TOKEN" : env.GITHUB_TOKEN ? "GITHUB_TOKEN" : null,
  };
}

async function linearConfig(args, workspace, env) {
  const workflow = readWorkflowConfig(path.join(workspace, ".symphony", "WORKFLOW.md"));
  const explicitSlug = args.linear_project_slug ?? env.LINEAR_PROJECT_SLUG ?? workflow.project_slug;
  return {
    projectSlug: String(explicitSlug ?? await resolveLinearProjectSlugFromEnv(env) ?? ""),
    workspaceSlug: String(args.linear_workspace_slug ?? env.LINEAR_WORKSPACE_SLUG ?? workflow.workspace_slug ?? "kata-sh"),
    tokenEnv: "LINEAR_API_KEY",
  };
}

async function resolveLinearProjectSlugFromEnv(env) {
  const projectId = env.LINEAR_PROJECT_ID;
  if (projectId) {
    try {
      const data = await linearGraphql(env, `
        query SymphonyBackendUatProjectById($id: String!) {
          project(id: $id) { slugId }
        }
      `, { id: projectId });
      if (data.project?.slugId) return data.project.slugId;
    } catch {
      // Fall through to single-project discovery below.
    }
  }

  const data = await linearGraphql(env, `
    query SymphonyBackendUatProjectDiscovery {
      projects(first: 2) { nodes { slugId } }
    }
  `, {});
  const projects = data.projects?.nodes ?? [];
  return projects.length === 1 ? projects[0].slugId : null;
}

function readWorkflowConfig(filePath) {
  if (!existsSync(filePath)) return {};
  const content = readFileSync(filePath, "utf8");
  const config = {};
  for (const key of [
    "repo_owner",
    "repo_name",
    "github_project_owner",
    "github_project_owner_type",
    "github_project_number",
    "project_slug",
    "workspace_slug",
  ]) {
    const match = content.match(new RegExp(`\\n\\s*${key}:\\s*([^\\n#]+)`));
    if (match) config[key] = match[1].trim().replace(/^["']|["']$/g, "");
  }
  return config;
}

function expectedOperationsFor(backend, contract) {
  return backend === "github"
    ? [...contract.sharedHelperOperations, ...contract.githubOnlyHelperOperations]
    : [...contract.sharedHelperOperations];
}

function loadHelperContract({ workspace, symphonyRoot, allowGeneratedFallback }) {
  const helperPath = path.join(symphonyRoot, "src", "helper.rs");
  if (existsSync(helperPath)) {
    return readHelperContractFromSource(symphonyRoot);
  }
  if (!allowGeneratedFallback) {
    throw new Error(`Expected Symphony helper source is missing: ${helperPath}`);
  }
  const contract = readGeneratedContractFallback(workspace);
  return {
    source: "generated-symphony-contract.json",
    sharedHelperOperations: contract.sharedHelperOperations,
    githubOnlyHelperOperations: contract.githubOnlyHelperOperations,
  };
}

function readGeneratedContractFallback(workspace) {
  const contractPath = path.join(SKILL_ROOT, "references", "generated-symphony-contract.json");
  if (!existsSync(contractPath)) {
    throw new Error(`Unable to load helper operations from source or generated contract: ${contractPath} is missing`);
  }
  const contract = JSON.parse(readFileSync(contractPath, "utf8"));
  const arraysValid = Array.isArray(contract.sharedHelperOperations)
    && contract.sharedHelperOperations.length > 0
    && Array.isArray(contract.githubOnlyHelperOperations)
    && contract.githubOnlyHelperOperations.length > 0;
  if (!arraysValid || !contract.generatedAt || !contract.gitCommit || !contract.symphonyRoot) {
    throw new Error(`Generated Symphony contract is not valid: ${contractPath}`);
  }
  if (contract.workspace && path.resolve(contract.workspace) !== path.resolve(workspace)) {
    throw new Error(`Generated Symphony contract workspace mismatch: ${contract.workspace}`);
  }
  return contract;
}

function defaultUatRunDir({ workspace, runtime, backend, stamp }) {
  return path.join(workspace, "uat-evidence", `${runtime}-${backend}-${stamp}-${process.pid}`);
}

function resolveSymphonyRoot(args, workspace) {
  return path.resolve(String(args.symphony_root ?? path.join(workspace, "apps", "symphony")));
}

function loadEnv(workspace, baseEnv) {
  const env = { ...baseEnv };
  for (const filePath of [path.join(workspace, ".env"), path.join(workspace, "apps", "symphony", ".env")]) {
    if (!existsSync(filePath)) continue;
    const content = readFileSync(filePath, "utf8");
    for (const line of content.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#") || !trimmed.includes("=")) continue;
      const [rawKey, ...rest] = trimmed.split("=");
      const key = rawKey.trim();
      const value = rest.join("=").trim().replace(/^["']|["']$/g, "");
      if (key && env[key] === undefined) env[key] = value;
    }
  }
  return env;
}

function ghJson(env, args) {
  const text = ghText(env, args);
  return parseJson(text) ?? {};
}

function ghApi(env, args) {
  return ghJson(env, ["api", ...args]);
}

function ghText(env, args) {
  const result = spawnSync("gh", args, {
    env,
    encoding: "utf8",
    maxBuffer: 1024 * 1024 * 16,
  });
  if (result.status !== 0) {
    throw new Error(`gh ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  }
  return result.stdout;
}

async function linearGraphql(env, query, variables) {
  const token = env.LINEAR_API_KEY || env.LINEAR_TOKEN;
  if (!token) throw new Error("Linear requires LINEAR_API_KEY or LINEAR_TOKEN");
  if (typeof fetch !== "function") throw new Error("Node fetch is required for Linear GraphQL");
  const response = await fetch("https://api.linear.app/graphql", {
    method: "POST",
    headers: { Authorization: token, "Content-Type": "application/json" },
    body: JSON.stringify({ query, variables }),
  });
  const payload = await response.json();
  if (!response.ok || payload.errors) {
    throw new Error(`Linear GraphQL failed: ${JSON.stringify(payload.errors ?? payload).slice(0, 1000)}`);
  }
  return payload.data;
}

function parseJson(text) {
  try {
    return JSON.parse(String(text ?? "").trim());
  } catch {
    return null;
  }
}

function summarizeHelperResult(data) {
  if (!data || typeof data !== "object") return data;
  const summary = {};
  if (data.issue) summary.issue = pick(data.issue, ["id", "identifier", "number", "title", "url", "html_url", "state"]);
  if (data.comment) summary.comment = pick(data.comment, ["id", "url", "html_url"]);
  if (data.comments) summary.comments = Array.isArray(data.comments) ? data.comments.length : undefined;
  if (data.children) summary.children = Array.isArray(data.children) ? data.children.length : undefined;
  if (data.documents) summary.documents = Array.isArray(data.documents) ? data.documents.length : undefined;
  if (data.title) summary.title = data.title;
  if (data.state) summary.state = data.state;
  if (data.pullRequest) summary.pullRequest = pick(data.pullRequest, ["number", "url", "state"]);
  if (data.failingCount !== undefined) summary.failingCount = data.failingCount;
  return summary;
}

function pick(source, keys) {
  const output = {};
  for (const key of keys) {
    if (source?.[key] !== undefined) output[key] = source[key];
  }
  return output;
}

function recordCreatedFollowup(evidence, backend, followup) {
  const issue = followup?.issue;
  if (!issue) return;
  evidence.created.followupIssue = issue;
  const url = issue.url ?? issue.html_url;
  if (url) evidence.providerProofLinks.push({ label: `${backend}-followup-issue`, url });
}

function writeEvidence(runDir, evidence) {
  const jsonPath = path.join(runDir, "evidence.json");
  const reportPath = path.join(runDir, "evidence.md");
  writeFileSync(jsonPath, `${JSON.stringify(evidence, null, 2)}\n`, "utf8");
  writeFileSync(reportPath, evidenceMarkdown(evidence, jsonPath), "utf8");
}

function evidenceMarkdown(evidence, jsonPath) {
  const links = (evidence.providerProofLinks ?? [])
    .map((entry) => `- ${entry.label}: ${entry.url ?? entry.id ?? "unknown"}`)
    .join("\n");
  const coverage = evidence.operationCoverage ?? {};
  return `# UAT Evidence

- Runtime: ${evidence.runtime ?? "symphony-runtime"}
- Backend: ${evidence.backend}
- Mode: ${evidence.mode}
- Stamp: ${evidence.stamp}
- Git commit: ${evidence.gitCommit}
- Binary: ${evidence.binaryPath}
- Workflow fixture: ${evidence.workflowFixture}
- Evidence JSON: ${jsonPath}
- Health: ${evidence.health?.ok ? "ok" : "failed"}
- Operation coverage: ${(coverage.observed ?? []).length}/${(coverage.expected ?? []).length}
- Missing operations: ${(coverage.missing ?? []).join(", ") || "none"}
- Cleanup completed: ${evidence.cleanup?.completed === true ? "yes" : "no"}

## Provider Proof Links

${links || "No provider proof links recorded."}
`;
}

function resultSummary(evidence) {
  return {
    ok: !evidence.failure,
    dryRun: evidence.mode === "dry-run",
    runtime: evidence.runtime ?? "symphony-runtime",
    backend: evidence.backend,
    health: evidence.health,
    operationCoverage: evidence.operationCoverage,
    evidence: path.join(evidence.runDir, "evidence.json"),
    report: path.join(evidence.runDir, "evidence.md"),
  };
}

function gitCommit(workspace) {
  const result = spawnSync("git", ["rev-parse", "HEAD"], { cwd: workspace, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : "unknown";
}

function timestamp() {
  return new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
}
