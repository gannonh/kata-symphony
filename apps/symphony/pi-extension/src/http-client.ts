import { SymphonyExtensionError } from "./errors.ts";
import { normalizeBaseUrl, type LastKnownSymphonyState } from "./state.ts";

export interface RunAttemptResponse {
  issue_id: string;
  issue_identifier: string;
  issue_title?: string | null;
  attempt?: number | null;
  workspace_path: string;
  started_at: string;
  status: string;
  error?: string | null;
  worker_host?: string | null;
  model?: string | null;
  tracker_state?: string | null;
  issue_url?: string | null;
}

export interface RunningSessionSnapshotResponse {
  turn_count?: number;
  last_activity_at?: string | null;
  total_tokens?: number;
  last_event?: string | null;
  last_event_message?: string | null;
  session_id?: string | null;
  current_tool_name?: string | null;
  current_tool_args_preview?: string | null;
  last_error?: string | null;
}

export interface WorkerSessionInfoResponse {
  turn_count?: number;
  max_turns?: number;
  last_activity_ms?: number | null;
  session_tokens?: {
    input_tokens?: number;
    output_tokens?: number;
    total_tokens?: number;
  };
  current_tool_name?: string | null;
  current_tool_args_preview?: string | null;
  last_error?: string | null;
}

export interface RetryQueueEntryResponse {
  issue_id: string;
  identifier: string;
  attempt: number;
  due_in_ms: number;
  error?: string | null;
  worker_host?: string | null;
  workspace_path?: string | null;
}

export interface BlockedIssueResponse {
  issue_id: string;
  identifier: string;
  title: string;
  state: string;
  blocker_identifiers: string[];
}

export interface CompletedIssueResponse {
  issue_id: string;
  identifier: string;
  title: string;
  completed_at?: string | null;
  issue_url?: string | null;
}

export interface PendingEscalationResponse {
  request_id: string;
  issue_id: string;
  issue_identifier: string;
  method: string;
  preview: string;
  created_at: string;
  timeout_ms: number;
}

export interface EscalationListResponse {
  pending: PendingEscalationResponse[];
}

export interface EscalationRespondResponse {
  ok: boolean;
}

export type ContextScopeResponse =
  | { type: "project" }
  | { type: "milestone"; value: string }
  | { type: "label"; value: string };

export interface SharedContextEntryResponse {
  id: string;
  author_issue: string;
  scope: ContextScopeResponse;
  content: string;
  created_at: string;
  ttl_ms: number;
}

export interface SharedContextSummaryResponse {
  total_entries: number;
  entries_by_scope: Record<string, number>;
  oldest_entry_at: string | null;
  newest_entry_at: string | null;
}

export interface SharedContextListResponse {
  entries: SharedContextEntryResponse[];
  summary: SharedContextSummaryResponse;
}

export interface SharedContextCreateInput {
  authorIssue: string;
  scope: string;
  content: string;
  ttlMs?: number;
}

export interface SharedContextWriteResponse {
  id: string;
  created_at: string;
}

export interface SharedContextDeleteResponse {
  deleted: number;
}

export interface SupervisorSnapshotResponse {
  active?: boolean;
  status?: string;
  steers_issued: number;
  conflicts_detected: number;
  patterns_detected: number;
  escalations_created: number;
}

export interface CodexTotalsResponse {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  event_count: number;
  seconds_running: number;
}

export interface TriageSessionResponse {
  issue_identifier: string;
  run_id: string;
  stage_run_id: string;
  attempt: number;
  harness: string;
  model?: string | null;
  started_at: string;
  last_activity_at?: string | null;
  last_event?: string | null;
  last_event_message?: string | null;
  session_id?: string | null;
  current_tool_name?: string | null;
  current_tool_args_preview?: string | null;
  total_tokens?: number;
}

export interface SymphonyStateResponse {
  tracker_project_url?: string | null;
  running?: Record<string, RunAttemptResponse>;
  running_sessions?: Record<string, RunningSessionSnapshotResponse>;
  running_session_info?: Record<string, WorkerSessionInfoResponse>;
  retry_queue: RetryQueueEntryResponse[];
  blocked: BlockedIssueResponse[];
  pending_escalations?: PendingEscalationResponse[];
  completed: CompletedIssueResponse[];
  /** In-flight A1 triage attempts (optional for older Symphony builds). */
  triage_sessions?: TriageSessionResponse[];
  polling?: {
    checking?: boolean;
    next_poll_in_ms?: number;
    poll_interval_ms?: number;
    poll_count?: number;
    last_poll_at?: string | null;
  };
  shared_context: SharedContextSummaryResponse;
  supervisor: SupervisorSnapshotResponse;
  codex_totals: CodexTotalsResponse;
  codex_rate_limits: Record<string, unknown> | null;
}

export interface RefreshResponse {
  queued: boolean;
  coalesced: boolean;
  pendingRequests: number;
}

export interface SteerResponse {
  ok: boolean;
  issueId: string;
  issueIdentifier: string;
  delivered: boolean;
  instructionPreview: string;
}

export interface SymphonyEventEnvelope {
  version: string;
  sequence: number;
  timestamp: string;
  kind: string;
  severity: string;
  issue?: string;
  event: string;
  payload: unknown;
}

interface ApiErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    status?: number;
    details?: unknown;
  };
}

function normalizeOptionalContextScope(scope: string | undefined): string | undefined {
  if (scope === undefined) return undefined;
  const trimmedScope = scope.trim();
  if (!trimmedScope) throw new TypeError("Shared context scope must not be empty");
  return trimmedScope;
}

export class SymphonyHttpClient {
  readonly baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = normalizeBaseUrl(baseUrl);
  }

  async getState(signal?: AbortSignal): Promise<SymphonyStateResponse> {
    const path = "/api/v1/state";
    const json = await this.requestJson(path, { method: "GET", signal });
    return validateSymphonyStateResponse(json, { baseUrl: this.baseUrl, path });
  }

  async verify(signal?: AbortSignal): Promise<SymphonyStateResponse> {
    return this.getState(signal);
  }

  async refresh(signal?: AbortSignal): Promise<RefreshResponse> {
    const path = "/api/v1/refresh";
    const json = await this.requestJson(path, { method: "POST", signal });
    return validateRefreshResponse(json, { baseUrl: this.baseUrl, path });
  }

  async steer(issueIdentifier: string, instruction: string, signal?: AbortSignal): Promise<SteerResponse> {
    const path = "/api/v1/steer";
    const json = await this.requestJson(path, {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ issue_identifier: issueIdentifier, instruction }),
    });
    return validateSteerResponse(json, { baseUrl: this.baseUrl, path, issueIdentifier });
  }

  async getEscalations(signal?: AbortSignal): Promise<EscalationListResponse> {
    const path = "/api/v1/escalations";
    const json = await this.requestJson(path, { method: "GET", signal });
    return validateEscalationListResponse(json, { baseUrl: this.baseUrl, path });
  }

  async respondEscalation(requestId: string, response: unknown, responderId = "pi-dashboard", signal?: AbortSignal): Promise<EscalationRespondResponse> {
    const path = `/api/v1/escalations/${encodeURIComponent(requestId)}/respond`;
    const json = await this.requestJson(path, {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ response, responder_id: responderId }),
    });
    return validateEscalationRespondResponse(json, { baseUrl: this.baseUrl, path, requestId });
  }

  async getContext(scope?: string, signal?: AbortSignal): Promise<SharedContextListResponse> {
    const trimmedScope = normalizeOptionalContextScope(scope);
    const path = trimmedScope ? `/api/v1/context?scope=${encodeURIComponent(trimmedScope)}` : "/api/v1/context";
    const json = await this.requestJson(path, { method: "GET", signal });
    return validateSharedContextListResponse(json, { baseUrl: this.baseUrl, path });
  }

  async createContext(input: SharedContextCreateInput, signal?: AbortSignal): Promise<SharedContextWriteResponse> {
    const path = "/api/v1/context";
    const body: Record<string, unknown> = {
      author_issue: input.authorIssue,
      scope: input.scope,
      content: input.content,
    };
    if (input.ttlMs !== undefined) body.ttl_ms = input.ttlMs;
    const json = await this.requestJson(path, {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    return validateSharedContextWriteResponse(json, { baseUrl: this.baseUrl, path });
  }

  async deleteContext(scope?: string, signal?: AbortSignal): Promise<SharedContextDeleteResponse> {
    const trimmedScope = normalizeOptionalContextScope(scope);
    const path = trimmedScope ? `/api/v1/context?scope=${encodeURIComponent(trimmedScope)}` : "/api/v1/context";
    const json = await this.requestJson(path, { method: "DELETE", signal });
    return validateSharedContextDeleteResponse(json, { baseUrl: this.baseUrl, path });
  }

  async deleteContextEntry(entryId: string, signal?: AbortSignal): Promise<SharedContextDeleteResponse> {
    const path = `/api/v1/context/${encodeURIComponent(entryId)}`;
    const json = await this.requestJson(path, { method: "DELETE", signal });
    return validateSharedContextDeleteResponse(json, { baseUrl: this.baseUrl, path, entryId });
  }

  toHealthSummary(state: SymphonyStateResponse): LastKnownSymphonyState {
    return {
      baseUrl: this.baseUrl,
      trackerProjectUrl: state.tracker_project_url ?? undefined,
      runningCount: Object.keys(state.running ?? {}).length,
      retryCount: state.retry_queue?.length ?? 0,
      blockedCount: state.blocked?.length ?? 0,
      completedCount: state.completed?.length ?? 0,
      pollingChecking: Boolean(state.polling?.checking),
      nextPollInMs: state.polling?.next_poll_in_ms ?? 0,
      updatedAt: new Date().toISOString(),
    };
  }

  private async requestJson(path: string, init: RequestInit): Promise<unknown> {
    const url = `${this.baseUrl}${path}`;
    let response: Response;
    try {
      response = await fetch(url, { ...init, headers: { accept: "application/json", ...(init.headers ?? {}) } });
    } catch (error) {
      if (isAbortError(error, init.signal)) throw error;
      throw new SymphonyExtensionError("attach_unreachable", "Could not reach Symphony HTTP API", {
        url,
        cause: error instanceof Error ? error.message : String(error),
      });
    }

    let text: string;
    try {
      text = await response.text();
    } catch (error) {
      if (isAbortError(error, init.signal)) throw error;
      throw new SymphonyExtensionError("non_symphony_response", "Could not read Symphony HTTP API response body", {
        url,
        status: response.status,
        cause: error instanceof Error ? error.message : String(error),
      });
    }

    let json: unknown;
    try {
      json = text ? JSON.parse(text) : {};
    } catch (error) {
      throw new SymphonyExtensionError("invalid_json", "Symphony HTTP API returned invalid JSON", {
        url,
        status: response.status,
        bodyPreview: text.slice(0, 200),
        cause: error instanceof Error ? error.message : String(error),
      });
    }

    if (!response.ok) {
      if (isRecord(json) && typeof json.error === "string") {
        throw new SymphonyExtensionError("api_error", json.error, {
          url,
          status: response.status,
          code: json.error,
        });
      }
      const envelope = parseApiErrorEnvelope(json);
      if (envelope?.error?.message) {
        throw new SymphonyExtensionError("api_error", envelope.error.message, {
          url,
          status: response.status,
          code: envelope.error.code,
          details: envelope.error.details,
        });
      }
      throw new SymphonyExtensionError("non_symphony_response", "Symphony HTTP API returned an unexpected error response", {
        url,
        status: response.status,
        body: json,
      });
    }

    return json;
  }
}

function validateSymphonyStateResponse(value: unknown, details: Record<string, unknown>): SymphonyStateResponse {
  if (!isRecord(value)) {
    throwNonSymphonyState(details, "state response was not an object");
  }

  const missingFields = [
    "running",
    "retry_queue",
    "blocked",
    "completed",
    "polling",
    "shared_context",
    "supervisor",
    "codex_totals",
    "codex_rate_limits",
  ].filter((field) => !(field in value));
  if (missingFields.length > 0) {
    throwNonSymphonyState(details, "state response was missing Symphony state fields", { missingFields });
  }

  validateOptionalStringOrNull(value, "tracker_project_url", details);
  validateOptionalRecord(value, "running", details);
  validateRunningAttempts(value.running, details);
  validateRetryQueueEntries(value, "retry_queue", details);
  validateBlockedIssues(value, "blocked", details);
  validateCompletedIssues(value, "completed", details);
  validateOptionalNumber(value, "poll_interval_ms", details);
  validateOptionalNumber(value, "max_concurrent_agents", details);
  validateOptionalRecord(value, "running_sessions", details);
  validateOptionalRecord(value, "running_session_info", details);
  validateOptionalArray(value, "claimed", details);
  validateOptionalTriageSessions(value, details);
  validatePendingEscalations(value, "pending_escalations", details);
  validateSharedContextSummary(value.shared_context, details, "shared_context", throwNonSymphonyState);
  validateSupervisorSnapshot(value.supervisor, details, "supervisor", throwNonSymphonyState);
  validateCodexTotals(value.codex_totals, details, "codex_totals", throwNonSymphonyState);
  if (value.codex_rate_limits !== null && !isRecord(value.codex_rate_limits)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: "codex_rate_limits", expected: "object or null" });
  }

  if (!isRecord(value.polling)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: "polling", expected: "object" });
  }
  validateRequiredBoolean(value.polling, "checking", details, "polling.checking");
  validateRequiredNumber(value.polling, "next_poll_in_ms", details, "polling.next_poll_in_ms");
  validateRequiredNumber(value.polling, "poll_interval_ms", details, "polling.poll_interval_ms");
  validateOptionalNumber(value.polling, "poll_count", details, "polling.poll_count");
  validateOptionalStringOrNull(value.polling, "last_poll_at", details, "polling.last_poll_at");

  return value as unknown as SymphonyStateResponse;
}

function validateEscalationListResponse(value: unknown, details: Record<string, unknown>): EscalationListResponse {
  if (!isRecord(value)) {
    throwNonSymphonyEscalationList(details, "escalation list response was not an object");
  }
  if (!Array.isArray(value.pending)) {
    throwNonSymphonyEscalationList(details, "escalation list response field had an invalid shape", { field: "pending", expected: "array" });
  }
  value.pending.forEach((entry, index) => validatePendingEscalation(entry, details, `pending.${index}`, throwNonSymphonyEscalationList));
  return { pending: value.pending as PendingEscalationResponse[] };
}

function validateEscalationRespondResponse(value: unknown, details: Record<string, unknown>): EscalationRespondResponse {
  if (!isRecord(value)) {
    throwNonSymphonyEscalationRespond(details, "escalation respond response was not an object");
  }
  if (typeof value.ok !== "boolean") {
    throwNonSymphonyEscalationRespond(details, "escalation respond response field had an invalid shape", { field: "ok", expected: "boolean" });
  }
  return { ok: value.ok };
}

function validateSharedContextListResponse(value: unknown, details: Record<string, unknown>): SharedContextListResponse {
  if (!isRecord(value)) throwNonSymphonyContext(details, "shared context response was not an object");
  if (!Array.isArray(value.entries)) {
    throwNonSymphonyContext(details, "shared context response field had an invalid shape", { field: "entries", expected: "array" });
  }
  value.entries.forEach((entry, index) => validateSharedContextEntry(entry, details, `entries.${index}`, throwNonSymphonyContext));
  validateSharedContextSummary(value.summary, details, "summary", throwNonSymphonyContext);
  return value as unknown as SharedContextListResponse;
}

function validateSharedContextWriteResponse(value: unknown, details: Record<string, unknown>): SharedContextWriteResponse {
  if (!isRecord(value)) throwNonSymphonyContext(details, "shared context write response was not an object");
  validateRequiredString(value, "id", details, "id", throwNonSymphonyContext);
  validateRequiredString(value, "created_at", details, "created_at", throwNonSymphonyContext);
  return { id: value.id as string, created_at: value.created_at as string };
}

function validateSharedContextDeleteResponse(value: unknown, details: Record<string, unknown>): SharedContextDeleteResponse {
  if (!isRecord(value)) throwNonSymphonyContext(details, "shared context delete response was not an object");
  validateRequiredNumber(value, "deleted", details, "deleted", throwNonSymphonyContext);
  return { deleted: value.deleted as number };
}

function validateSharedContextEntry(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "shared context entry had an invalid shape", { field: detailField, expected: "object" });
  validateRequiredString(value, "id", details, `${detailField}.id`, thrower);
  validateRequiredString(value, "author_issue", details, `${detailField}.author_issue`, thrower);
  validateContextScope(value.scope, details, `${detailField}.scope`, thrower);
  validateRequiredString(value, "content", details, `${detailField}.content`, thrower);
  validateRequiredString(value, "created_at", details, `${detailField}.created_at`, thrower);
  validateRequiredNumber(value, "ttl_ms", details, `${detailField}.ttl_ms`, thrower);
}

function validateContextScope(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "shared context scope had an invalid shape", { field: detailField, expected: "object" });
  if (value.type !== "project" && value.type !== "milestone" && value.type !== "label") {
    thrower(details, "shared context scope had an invalid shape", { field: `${detailField}.type`, expected: "project | milestone | label" });
  }
  if ((value.type === "milestone" || value.type === "label") && typeof value.value !== "string") {
    thrower(details, "shared context scope had an invalid shape", { field: `${detailField}.value`, expected: "string" });
  }
}

function validateSharedContextSummary(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "shared context summary had an invalid shape", { field: detailField, expected: "object" });
  validateRequiredNumber(value, "total_entries", details, `${detailField}.total_entries`, thrower);
  if (!isRecord(value.entries_by_scope) || Object.values(value.entries_by_scope).some((entry) => !isFiniteNumber(entry))) {
    thrower(details, "shared context summary had an invalid shape", { field: `${detailField}.entries_by_scope`, expected: "Record<string, number>" });
  }
  validateRequiredStringOrNull(value, "oldest_entry_at", details, `${detailField}.oldest_entry_at`, thrower);
  validateRequiredStringOrNull(value, "newest_entry_at", details, `${detailField}.newest_entry_at`, thrower);
}

function validateSupervisorSnapshot(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "supervisor snapshot had an invalid shape", { field: detailField, expected: "object" });
  if (value.active !== undefined && typeof value.active !== "boolean") {
    thrower(details, "supervisor snapshot had an invalid shape", { field: `${detailField}.active`, expected: "boolean" });
  }
  validateOptionalStringOrNull(value, "status", details, `${detailField}.status`, thrower);
  validateRequiredNumber(value, "steers_issued", details, `${detailField}.steers_issued`, thrower);
  validateRequiredNumber(value, "conflicts_detected", details, `${detailField}.conflicts_detected`, thrower);
  validateRequiredNumber(value, "patterns_detected", details, `${detailField}.patterns_detected`, thrower);
  validateRequiredNumber(value, "escalations_created", details, `${detailField}.escalations_created`, thrower);
}

function validateCodexTotals(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "codex totals had an invalid shape", { field: detailField, expected: "object" });
  validateRequiredNumber(value, "input_tokens", details, `${detailField}.input_tokens`, thrower);
  validateRequiredNumber(value, "output_tokens", details, `${detailField}.output_tokens`, thrower);
  validateRequiredNumber(value, "total_tokens", details, `${detailField}.total_tokens`, thrower);
  validateRequiredNumber(value, "event_count", details, `${detailField}.event_count`, thrower);
  validateRequiredNumber(value, "seconds_running", details, `${detailField}.seconds_running`, thrower);
}

function throwNonSymphonyContext(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony shared context response", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function validateRefreshResponse(value: unknown, details: Record<string, unknown>): RefreshResponse {
  if (!isRecord(value)) {
    throwNonSymphonyRefresh(details, "refresh response was not an object");
  }
  if (typeof value.queued !== "boolean") {
    throwNonSymphonyRefresh(details, "refresh response field had an invalid shape", { field: "queued", expected: "boolean" });
  }
  if (typeof value.coalesced !== "boolean") {
    throwNonSymphonyRefresh(details, "refresh response field had an invalid shape", { field: "coalesced", expected: "boolean" });
  }
  if (!isFiniteNumber(value.pending_requests)) {
    throwNonSymphonyRefresh(details, "refresh response field had an invalid shape", { field: "pending_requests", expected: "number" });
  }

  return {
    queued: value.queued,
    coalesced: value.coalesced,
    pendingRequests: value.pending_requests,
  };
}

function validateSteerResponse(value: unknown, details: Record<string, unknown>): SteerResponse {
  if (!isRecord(value)) {
    throwNonSymphonySteer(details, "steer response was not an object");
  }
  if (typeof value.ok !== "boolean") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "ok", expected: "boolean" });
  }
  if (typeof value.issue_id !== "string") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "issue_id", expected: "string" });
  }
  if (typeof value.issue_identifier !== "string") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "issue_identifier", expected: "string" });
  }
  if (typeof value.delivered !== "boolean") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "delivered", expected: "boolean" });
  }
  if (typeof value.instruction_preview !== "string") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "instruction_preview", expected: "string" });
  }

  return {
    ok: value.ok,
    issueId: value.issue_id,
    issueIdentifier: value.issue_identifier,
    delivered: value.delivered,
    instructionPreview: value.instruction_preview,
  };
}

function throwNonSymphonyRefresh(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony refresh response", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function throwNonSymphonySteer(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony steer response", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function throwNonSymphonyEscalationList(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony escalation list response", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function throwNonSymphonyEscalationRespond(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony escalation respond response", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function parseApiErrorEnvelope(value: unknown): ApiErrorEnvelope | undefined {
  if (!isRecord(value) || !isRecord(value.error)) return undefined;
  return {
    error: {
      code: typeof value.error.code === "string" ? value.error.code : undefined,
      message: typeof value.error.message === "string" ? value.error.message : undefined,
      status: isFiniteNumber(value.error.status) ? value.error.status : undefined,
      details: value.error.details,
    },
  };
}

function validateRunningAttempts(value: unknown, details: Record<string, unknown>): void {
  if (value === undefined) return;
  if (!isRecord(value)) return;
  for (const [key, attempt] of Object.entries(value)) {
    validateRunAttemptResponse(attempt, details, `running.${key}`);
  }
}

function validateRunAttemptResponse(value: unknown, details: Record<string, unknown>, detailField: string): void {
  if (!isRecord(value)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "object" });
  }
  validateRequiredString(value, "issue_id", details, `${detailField}.issue_id`);
  validateRequiredString(value, "issue_identifier", details, `${detailField}.issue_identifier`);
  validateRequiredString(value, "workspace_path", details, `${detailField}.workspace_path`);
  validateRequiredString(value, "started_at", details, `${detailField}.started_at`);
  validateRequiredString(value, "status", details, `${detailField}.status`);
  validateOptionalStringOrNull(value, "issue_title", details, `${detailField}.issue_title`);
  validateOptionalNumberOrNull(value, "attempt", details, `${detailField}.attempt`);
  validateOptionalStringOrNull(value, "error", details, `${detailField}.error`);
  validateOptionalStringOrNull(value, "worker_host", details, `${detailField}.worker_host`);
  validateOptionalStringOrNull(value, "model", details, `${detailField}.model`);
  validateOptionalStringOrNull(value, "tracker_state", details, `${detailField}.tracker_state`);
  validateOptionalStringOrNull(value, "issue_url", details, `${detailField}.issue_url`);
}

type NonSymphonyThrower = (details: Record<string, unknown>, reason: string, extraDetails?: Record<string, unknown>) => never;

function validateRetryQueueEntries(value: Record<string, unknown>, field: string, details: Record<string, unknown>): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!Array.isArray(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "array" });
  }
  fieldValue.forEach((entry, index) => validateRetryQueueEntry(entry, details, `${field}.${index}`));
}

function validateRetryQueueEntry(value: unknown, details: Record<string, unknown>, detailField: string): void {
  if (!isRecord(value)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "object" });
  }
  validateRequiredString(value, "issue_id", details, `${detailField}.issue_id`);
  validateRequiredString(value, "identifier", details, `${detailField}.identifier`);
  validateRequiredNumber(value, "attempt", details, `${detailField}.attempt`);
  validateRequiredNumber(value, "due_in_ms", details, `${detailField}.due_in_ms`);
  validateOptionalStringOrNull(value, "error", details, `${detailField}.error`);
  validateOptionalStringOrNull(value, "worker_host", details, `${detailField}.worker_host`);
  validateOptionalStringOrNull(value, "workspace_path", details, `${detailField}.workspace_path`);
}

function validateBlockedIssues(value: Record<string, unknown>, field: string, details: Record<string, unknown>): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!Array.isArray(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "array" });
  }
  fieldValue.forEach((entry, index) => validateBlockedIssue(entry, details, `${field}.${index}`));
}

function validateBlockedIssue(value: unknown, details: Record<string, unknown>, detailField: string): void {
  if (!isRecord(value)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "object" });
  }
  validateRequiredString(value, "issue_id", details, `${detailField}.issue_id`);
  validateRequiredString(value, "identifier", details, `${detailField}.identifier`);
  validateRequiredString(value, "title", details, `${detailField}.title`);
  validateRequiredString(value, "state", details, `${detailField}.state`);
  validateRequiredStringArray(value, "blocker_identifiers", details, `${detailField}.blocker_identifiers`);
}

function validateCompletedIssues(value: Record<string, unknown>, field: string, details: Record<string, unknown>): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!Array.isArray(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "array" });
  }
  fieldValue.forEach((entry, index) => validateCompletedIssue(entry, details, `${field}.${index}`));
}

function validateCompletedIssue(value: unknown, details: Record<string, unknown>, detailField: string): void {
  if (!isRecord(value)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "object" });
  }
  validateRequiredString(value, "issue_id", details, `${detailField}.issue_id`);
  validateRequiredString(value, "identifier", details, `${detailField}.identifier`);
  validateRequiredString(value, "title", details, `${detailField}.title`);
  validateOptionalStringOrNull(value, "completed_at", details, `${detailField}.completed_at`);
  validateOptionalStringOrNull(value, "issue_url", details, `${detailField}.issue_url`);
}

function validatePendingEscalations(
  value: Record<string, unknown>,
  field: string,
  details: Record<string, unknown>,
  thrower: NonSymphonyThrower = throwNonSymphonyState,
): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!Array.isArray(fieldValue)) {
    thrower(details, "state response field had an invalid shape", { field, expected: "array" });
  }
  fieldValue.forEach((entry, index) => validatePendingEscalation(entry, details, `${field}.${index}`, thrower));
}

function validatePendingEscalation(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) {
    thrower(details, "state response field had an invalid shape", { field: detailField, expected: "object" });
  }
  validateRequiredString(value, "request_id", details, `${detailField}.request_id`, thrower);
  validateRequiredString(value, "issue_id", details, `${detailField}.issue_id`, thrower);
  validateRequiredString(value, "issue_identifier", details, `${detailField}.issue_identifier`, thrower);
  validateRequiredString(value, "method", details, `${detailField}.method`, thrower);
  validateRequiredString(value, "preview", details, `${detailField}.preview`, thrower);
  validateRequiredString(value, "created_at", details, `${detailField}.created_at`, thrower);
  validateRequiredNumber(value, "timeout_ms", details, `${detailField}.timeout_ms`, thrower);
}

function validateOptionalArray(value: Record<string, unknown>, field: string, details: Record<string, unknown>, detailField = field): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!Array.isArray(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "array" });
  }
}

function validateOptionalTriageSessions(value: Record<string, unknown>, details: Record<string, unknown>): void {
  const fieldValue = value.triage_sessions;
  if (fieldValue === undefined) return;
  if (!Array.isArray(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", {
      field: "triage_sessions",
      expected: "array",
    });
  }
  fieldValue.forEach((entry, index) => {
    if (!isRecord(entry)) {
      throwNonSymphonyState(details, "state response field had an invalid shape", {
        field: `triage_sessions.${index}`,
        expected: "object",
      });
    }
    validateRequiredString(entry, "issue_identifier", details, `triage_sessions.${index}.issue_identifier`);
    validateRequiredString(entry, "run_id", details, `triage_sessions.${index}.run_id`);
    validateRequiredString(entry, "stage_run_id", details, `triage_sessions.${index}.stage_run_id`);
    validateRequiredNumber(entry, "attempt", details, `triage_sessions.${index}.attempt`);
    validateRequiredString(entry, "harness", details, `triage_sessions.${index}.harness`);
    validateRequiredString(entry, "started_at", details, `triage_sessions.${index}.started_at`);
  });
}

function validateOptionalRecord(value: Record<string, unknown>, field: string, details: Record<string, unknown>, detailField = field): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!isRecord(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "object" });
  }
}

function validateOptionalStringOrNull(
  value: Record<string, unknown>,
  field: string,
  details: Record<string, unknown>,
  detailField = field,
  thrower: NonSymphonyThrower = throwNonSymphonyState,
): void {
  const fieldValue = value[field];
  if (fieldValue === undefined || fieldValue === null) return;
  if (typeof fieldValue !== "string") {
    thrower(details, "state response field had an invalid shape", { field: detailField, expected: "string or null" });
  }
}

function validateRequiredStringOrNull(
  value: Record<string, unknown>,
  field: string,
  details: Record<string, unknown>,
  detailField = field,
  thrower: NonSymphonyThrower = throwNonSymphonyState,
): void {
  const fieldValue = value[field];
  if (fieldValue === null || typeof fieldValue === "string") return;
  thrower(details, "state response field had an invalid shape", { field: detailField, expected: "string or null" });
}

function validateOptionalNumber(value: Record<string, unknown>, field: string, details: Record<string, unknown>, detailField = field): void {
  const fieldValue = value[field];
  if (fieldValue === undefined) return;
  if (!isFiniteNumber(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "number" });
  }
}

function validateOptionalNumberOrNull(value: Record<string, unknown>, field: string, details: Record<string, unknown>, detailField = field): void {
  const fieldValue = value[field];
  if (fieldValue === undefined || fieldValue === null) return;
  if (!isFiniteNumber(fieldValue)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "number or null" });
  }
}

function validateRequiredString(
  value: Record<string, unknown>,
  field: string,
  details: Record<string, unknown>,
  detailField = field,
  thrower: NonSymphonyThrower = throwNonSymphonyState,
): void {
  if (typeof value[field] !== "string") {
    thrower(details, "state response field had an invalid shape", { field: detailField, expected: "string" });
  }
}

function validateRequiredStringArray(value: Record<string, unknown>, field: string, details: Record<string, unknown>, detailField = field): void {
  const fieldValue = value[field];
  if (!Array.isArray(fieldValue) || fieldValue.some((entry) => typeof entry !== "string")) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "string[]" });
  }
}

function validateRequiredBoolean(value: Record<string, unknown>, field: string, details: Record<string, unknown>, detailField = field): void {
  if (typeof value[field] !== "boolean") {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: detailField, expected: "boolean" });
  }
}

function validateRequiredNumber(
  value: Record<string, unknown>,
  field: string,
  details: Record<string, unknown>,
  detailField = field,
  thrower: NonSymphonyThrower = throwNonSymphonyState,
): void {
  if (!isFiniteNumber(value[field])) {
    thrower(details, "state response field had an invalid shape", { field: detailField, expected: "number" });
  }
}

function throwNonSymphonyState(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony state", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isAbortError(error: unknown, signal?: AbortSignal | null): boolean {
  return Boolean(signal?.aborted) || (error instanceof Error && error.name === "AbortError");
}
