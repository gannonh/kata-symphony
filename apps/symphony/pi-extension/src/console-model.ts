import type {
  BlockedIssueResponse,
  CompletedIssueResponse,
  PendingEscalationResponse,
  RetryQueueEntryResponse,
  RunAttemptResponse,
  SymphonyEventEnvelope,
  SymphonyStateResponse,
  TriageSessionResponse,
} from "./http-client.ts";

export interface WorkerRow {
  issueId: string;
  issueIdentifier: string;
  title: string;
  trackerState: string;
  attempt: string;
  turnCount: string;
  maxTurns: string;
  lastActivity: string;
  workerHost: string;
  workspacePath: string;
  status: string;
  errorPreview: string;
}

export type IssueRowKind = "running" | "triage" | "retry" | "blocked" | "completed";

export interface IssueRow {
  kind: IssueRowKind;
  issueId: string;
  issueIdentifier: string;
  title: string;
  status: string;
  trackerState: string;
  attempt: string;
  turnCount: string;
  maxTurns: string;
  lastActivity: string;
  workerHost: string;
  workspacePath: string;
  errorPreview: string;
  blockers: string;
  completedAt: string;
}

export interface TriageRow {
  issueIdentifier: string;
  stageRunId: string;
  attempt: string;
  harness: string;
  model: string;
  status: string;
  lastEvent: string;
  message: string;
  lastActivity: string;
  tokens: string;
}

export interface EscalationRow {
  requestId: string;
  issueId: string;
  issueIdentifier: string;
  method: string;
  preview: string;
  createdAt: string;
  timeout: string;
}

export function buildWorkerRows(state: SymphonyStateResponse | undefined): WorkerRow[] {
  return Object.entries(state?.running ?? {})
    .map(([issueId, attempt]) => buildWorkerRow(issueId, attempt, state))
    .sort((left, right) => left.issueIdentifier.localeCompare(right.issueIdentifier));
}

export function buildIssueRows(state: SymphonyStateResponse | undefined): IssueRow[] {
  if (!state) return [];

  return [
    ...Object.entries(state.running ?? {})
      .map(([issueId, attempt]) => workerToIssueRow(issueId, attempt, state))
      .sort(compareIssueRows),
    ...(state.triage_sessions ?? []).map(triageToIssueRow).sort(compareIssueRows),
    ...(state.retry_queue ?? []).map(retryToIssueRow).sort(compareIssueRows),
    ...(state.blocked ?? []).map(blockedToIssueRow).sort(compareIssueRows),
    ...(state.completed ?? []).map(completedToIssueRow).sort(compareIssueRows),
  ];
}

export function buildTriageRows(state: SymphonyStateResponse | undefined): TriageRow[] {
  return (state?.triage_sessions ?? [])
    .slice()
    .sort((left, right) => left.issue_identifier.localeCompare(right.issue_identifier))
    .map((session) => ({
      issueIdentifier: session.issue_identifier,
      stageRunId: session.stage_run_id,
      attempt: String(session.attempt),
      harness: session.harness,
      model: session.model?.trim() || "-",
      status: `triage#${session.attempt}`,
      lastEvent: session.last_event?.trim() || "-",
      message: triageMessage(session),
      lastActivity: session.last_activity_at ?? session.started_at,
      tokens: session.total_tokens === undefined ? "-" : String(session.total_tokens),
    }));
}

export function buildEscalationRows(state: SymphonyStateResponse | undefined): EscalationRow[] {
  return (state?.pending_escalations ?? [])
    .slice()
    .sort(compareEscalations)
    .map((entry) => ({
      requestId: entry.request_id,
      issueId: entry.issue_id,
      issueIdentifier: entry.issue_identifier,
      method: entry.method,
      preview: truncateText(entry.preview, 120),
      createdAt: entry.created_at,
      timeout: formatDuration(entry.timeout_ms),
    }));
}

export function formatEventRows(events: SymphonyEventEnvelope[], limit = 8): string[] {
  return events
    .filter(
      (event) =>
        event.kind === "worker" ||
        event.kind === "runtime" ||
        event.kind === "triage" ||
        event.kind.startsWith("escalation_") ||
        event.event.startsWith("triage_"),
    )
    .slice(-limit)
    .reverse()
    .map((event) => [event.timestamp, event.severity, event.kind, event.issue ?? "-", event.event, eventSummary(event)].filter(Boolean).join(" "));
}

function triageToIssueRow(session: TriageSessionResponse): IssueRow {
  return {
    kind: "triage",
    issueId: session.stage_run_id,
    issueIdentifier: session.issue_identifier,
    title: triageMessage(session),
    status: `triage#${session.attempt}`,
    trackerState: "triage",
    attempt: String(session.attempt),
    turnCount: "1",
    maxTurns: "1",
    lastActivity: session.last_activity_at ?? session.started_at,
    workerHost: session.harness,
    workspacePath: session.session_id ?? "-",
    errorPreview: "-",
    blockers: "-",
    completedAt: "-",
  };
}

function triageMessage(session: TriageSessionResponse): string {
  if (session.current_tool_name) {
    const args = session.current_tool_args_preview?.trim();
    return args ? `${session.current_tool_name}: ${truncateText(args, 80)}` : session.current_tool_name;
  }
  return session.last_event_message?.trim() || session.last_event?.trim() || "running";
}

function workerToIssueRow(issueId: string, attempt: RunAttemptResponse, state: SymphonyStateResponse | undefined): IssueRow {
  return {
    kind: "running",
    ...buildWorkerRow(issueId, attempt, state),
    blockers: "-",
    completedAt: "-",
  };
}

function retryToIssueRow(entry: RetryQueueEntryResponse): IssueRow {
  const errorPreview = entry.error ? truncateText(entry.error, 120) : "-";
  return {
    kind: "retry",
    issueId: entry.issue_id,
    issueIdentifier: entry.identifier,
    title: entry.error ?? "pending retry",
    status: `retry in ${formatDuration(entry.due_in_ms)}`,
    trackerState: "retry",
    attempt: String(entry.attempt),
    turnCount: "-",
    maxTurns: "-",
    lastActivity: "-",
    workerHost: entry.worker_host ?? "local",
    workspacePath: entry.workspace_path ?? "-",
    errorPreview,
    blockers: "-",
    completedAt: "-",
  };
}

function blockedToIssueRow(entry: BlockedIssueResponse): IssueRow {
  return {
    kind: "blocked",
    issueId: entry.issue_id,
    issueIdentifier: entry.identifier,
    title: entry.title,
    status: entry.state,
    trackerState: entry.state,
    attempt: "-",
    turnCount: "-",
    maxTurns: "-",
    lastActivity: "-",
    workerHost: "-",
    workspacePath: "-",
    errorPreview: "-",
    blockers: entry.blocker_identifiers.length > 0 ? entry.blocker_identifiers.join(", ") : "-",
    completedAt: "-",
  };
}

function completedToIssueRow(entry: CompletedIssueResponse): IssueRow {
  return {
    kind: "completed",
    issueId: entry.issue_id,
    issueIdentifier: entry.identifier,
    title: entry.title,
    status: "completed",
    trackerState: "completed",
    attempt: "-",
    turnCount: "-",
    maxTurns: "-",
    lastActivity: "-",
    workerHost: "-",
    workspacePath: "-",
    errorPreview: "-",
    blockers: "-",
    completedAt: entry.completed_at ?? "-",
  };
}

function buildWorkerRow(issueId: string, attempt: RunAttemptResponse, state: SymphonyStateResponse | undefined): WorkerRow {
  const session = state?.running_sessions?.[issueId];
  const info = state?.running_session_info?.[issueId];
  const turnCount = info?.turn_count ?? session?.turn_count;
  const lastError = attempt.error ?? info?.last_error ?? session?.last_error;

  return {
    issueId,
    issueIdentifier: attempt.issue_identifier,
    title: attempt.issue_title ?? "-",
    trackerState: attempt.tracker_state ?? "-",
    attempt: String(attempt.attempt ?? 1),
    turnCount: turnCount === undefined ? "-" : String(turnCount),
    maxTurns: info?.max_turns === undefined ? "-" : String(info.max_turns),
    lastActivity: formatLastActivity(info?.last_activity_ms, session?.last_activity_at),
    workerHost: attempt.worker_host ?? "local",
    workspacePath: attempt.workspace_path,
    status: attempt.status,
    errorPreview: lastError ? truncateText(lastError, 120) : "-",
  };
}

function formatLastActivity(lastActivityMs: number | null | undefined, lastActivityAt: string | null | undefined): string {
  if (typeof lastActivityMs === "number" && Number.isFinite(lastActivityMs)) {
    return new Date(lastActivityMs).toISOString();
  }
  return lastActivityAt ?? "-";
}

function formatDuration(durationMs: number): string {
  if (!Number.isFinite(durationMs) || durationMs <= 0) return "0s";
  const totalSeconds = Math.ceil(durationMs / 1000);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds}s`;
}

function compareIssueRows(left: IssueRow, right: IssueRow): number {
  return left.issueIdentifier.localeCompare(right.issueIdentifier);
}

function compareEscalations(left: PendingEscalationResponse, right: PendingEscalationResponse): number {
  return timestampMs(left.created_at) - timestampMs(right.created_at) || left.request_id.localeCompare(right.request_id);
}

function timestampMs(value: string): number {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function eventSummary(event: SymphonyEventEnvelope): string {
  if (!isRecord(event.payload)) return "";
  const preferred = event.payload.error_preview ?? event.payload.summary ?? event.payload.message ?? event.payload.reason ?? event.payload.error ?? event.payload.instruction_preview;
  if (typeof preferred !== "string") return "";
  const singleLine = preferred.replace(/\s+/g, " ").trim();
  return singleLine ? truncateText(singleLine, 120) : "";
}

function truncateText(value: string, maxLength: number): string {
  if (value.length <= maxLength) return value;
  return `${value.slice(0, maxLength - 1)}…`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
