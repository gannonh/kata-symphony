import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { matchesKey, truncateToWidth } from "@earendil-works/pi-tui";
import { buildEscalationRows, buildIssueRows, buildTriageRows, buildWorkerRows, formatEventRows, type EscalationRow, type IssueRow, type TriageRow, type WorkerRow } from "./console-model.ts";
import { startSymphonyEventStream, type EventStreamHandle } from "./event-stream.ts";
import type { SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";
import type { SymphonyRuntime } from "./runtime.ts";
import type { ExtensionState } from "./state.ts";

type ConsoleThemeColor = "accent" | "border" | "borderAccent" | "success" | "error" | "warning" | "muted" | "dim" | "text";

type ConsoleThemeBg = "selectedBg";

interface ConsoleTheme {
  fg(color: ConsoleThemeColor, text: string): string;
  bg?(color: ConsoleThemeBg, text: string): string;
  bold(text: string): string;
}

export type ConsoleShortcutAction = "selectPrevious" | "selectNext" | "refresh" | "steer" | "respondEscalation" | "toggleDetails" | "close";

let activeConsole: SymphonyConsoleComponent | undefined;

export async function handleActiveConsoleShortcut(action: ConsoleShortcutAction, ctx: Pick<ExtensionContext, "ui">): Promise<void> {
  if (!activeConsole) {
    ctx.ui.notify("No Symphony console is open. Use /symphony:console first.", "warning");
    return;
  }

  switch (action) {
    case "selectPrevious":
      activeConsole.selectPreviousWorker();
      return;
    case "selectNext":
      activeConsole.selectNextWorker();
      return;
    case "refresh":
      await activeConsole.refreshNow();
      return;
    case "steer":
      await activeConsole.steerNow();
      return;
    case "respondEscalation":
      await activeConsole.respondToEscalationNow();
      return;
    case "toggleDetails":
      activeConsole.toggleDetails();
      return;
    case "close":
      activeConsole.closeConsole();
      return;
  }
}

export function closeActiveConsole(ctx: Pick<ExtensionContext, "ui">): void {
  if (activeConsole) {
    activeConsole.closeConsole();
    return;
  }
  ctx.ui.setWidget("symphony-console", undefined);
}

export interface ConsoleOptions {
  state: ExtensionState;
  getState: () => SymphonyStateResponse | undefined;
  getEvents: () => SymphonyEventEnvelope[];
  refresh: () => Promise<void>;
  steer: (issueIdentifier: string, instruction: string) => Promise<void>;
  respondToEscalation: (requestId: string, response: unknown) => Promise<void>;
  prompt: (title: string, label: string) => Promise<string | undefined>;
  close: () => void;
  requestRender: () => void;
  notify: (message: string, level: "info" | "warning" | "error") => void;
  theme?: ConsoleTheme;
}

export class SymphonyConsoleComponent {
  private refreshing = false;
  private selectedIndex = 0;

  constructor(private readonly options: ConsoleOptions) {}

  handleInput(data: string): void {
    if (data === "q" || data === "Q" || matchesKey(data, "escape")) {
      this.closeConsole();
      return;
    }

    if (data === "r" || data === "R") {
      void this.refreshNow();
      return;
    }

    if (data === "d" || data === "D") {
      this.toggleDetails();
      return;
    }

    if (data === "s" || data === "S") {
      void this.steerNow();
      return;
    }

    if (data === "e" || data === "E") {
      void this.respondToEscalationNow();
      return;
    }

    if (data === "\u001b[A" || matchesKey(data, "up")) {
      this.selectPreviousWorker();
      return;
    }

    if (data === "\u001b[B" || matchesKey(data, "down")) {
      this.selectNextWorker();
    }
  }

  render(width: number): string[] {
    const state = this.options.state;
    const health = state.lastKnownState;
    const symphonyState = this.options.getState();
    const workers: WorkerRow[] = buildWorkerRows(symphonyState);
    const issueRows: IssueRow[] = buildIssueRows(symphonyState);
    const escalationRows: EscalationRow[] = buildEscalationRows(symphonyState);
    this.clampSelection(issueRows.length + escalationRows.length);
    const runningRows = issueRows.filter((row) => row.kind === "running");
    const triageRows = issueRows.filter((row) => row.kind === "triage");
    const triageDetailRows: TriageRow[] = buildTriageRows(symphonyState);
    const retryRows = issueRows.filter((row) => row.kind === "retry");
    const blockedRows = issueRows.filter((row) => row.kind === "blocked");
    const completedRows = issueRows.filter((row) => row.kind === "completed");
    const selectedIssue = issueRows[this.selectedIndex];
    const theme = this.options.theme;
    const connection = state.attachedBaseUrl ? color(theme, "success", "attached") : color(theme, "error", "detached");
    const polling = health?.pollingChecking ? color(theme, "warning", "checking") : color(theme, "success", "idle");
    const consoleWidth = Math.max(44, width);
    const triageOffset = runningRows.length;
    const retryOffset = triageOffset + triageRows.length;
    const blockedOffset = retryOffset + retryRows.length;
    const completedOffset = blockedOffset + blockedRows.length;
    const lines = [
      color(theme, "accent", bold(theme, "Symphony Console")),
      "",
      ...boxLines("Status", [
        `connection: ${connection}`,
        `dashboard: ${color(theme, "dim", state.attachedBaseUrl ?? "none")}`,
        `project: ${color(theme, "dim", health?.trackerProjectUrl ?? "none")}`,
        `polling: ${polling} | next poll: ${health?.nextPollInMs ?? 0}ms`,
        `owned process: ${state.ownedProcess ? color(theme, "success", `pid ${state.ownedProcess.pid}`) : color(theme, "dim", "none")}`,
        `updated: ${color(theme, "dim", health?.updatedAt ?? "never")}`,
      ], consoleWidth, theme),
      "",
      ...boxLines("Worker Summary", [
        `workers: ${color(theme, "success", `running: ${health?.runningCount ?? workers.length}`)} | ${color(theme, "accent", `triage: ${triageRows.length}`)} | ${color(theme, "warning", `retry: ${health?.retryCount ?? retryRows.length}`)} | ${color(theme, "error", `blocked: ${health?.blockedCount ?? blockedRows.length}`)} | ${color(theme, "accent", `completed: ${health?.completedCount ?? completedRows.length}`)}`,
      ], consoleWidth, theme),
      "",
      ...boxLines("Running Workers", renderIssueTable(runningRows, this.selectedIndex, 0, theme), consoleWidth, theme),
      ...boxLines("Triage", renderTriageTable(triageDetailRows, this.selectedIndex, triageOffset, theme), consoleWidth, theme),
      ...boxLines("Retry Queue", renderIssueTable(retryRows, this.selectedIndex, retryOffset, theme), consoleWidth, theme),
      ...boxLines("Blocked Issues", renderIssueTable(blockedRows, this.selectedIndex, blockedOffset, theme), consoleWidth, theme),
      ...boxLines("Completed Issues", renderIssueTable(completedRows, this.selectedIndex, completedOffset, theme), consoleWidth, theme),
      ...boxLines("Selected Issue", renderSelectedIssueDetails(selectedIssue, state.console.showDetails, theme), consoleWidth, theme),
      ...boxLines("Pending Escalations", renderEscalationTable(escalationRows, this.selectedIndex - issueRows.length, theme), consoleWidth, theme),
      ...boxLines("Selected Escalation", renderSelectedEscalationDetails(escalationRows[this.selectedIndex - issueRows.length], state.console.showDetails, theme), consoleWidth, theme),
      ...boxLines("Events", renderRecentEvents(formatEventRows(this.options.getEvents()), theme), consoleWidth, theme),
      "",
      ...boxLines("Actions", renderActionLegend(this.refreshing, consoleWidth, theme), consoleWidth, theme),
    ];

    return lines.map((line) => truncateToWidth(line, width));
  }

  invalidate(): void {}

  selectPreviousWorker(): void {
    this.moveSelection(-1);
  }

  selectNextWorker(): void {
    this.moveSelection(1);
  }

  toggleDetails(): void {
    this.options.state.console.showDetails = !this.options.state.console.showDetails;
    this.options.requestRender();
  }

  async refreshNow(): Promise<void> {
    await this.refresh();
  }

  async steerNow(): Promise<void> {
    await this.steerSelectedWorker();
  }

  async respondToEscalationNow(): Promise<void> {
    await this.respondToSelectedEscalation();
  }

  closeConsole(): void {
    this.options.close();
  }

  private moveSelection(delta: number): void {
    const symphonyState = this.options.getState();
    const rowCount = buildIssueRows(symphonyState).length + buildEscalationRows(symphonyState).length;
    if (rowCount === 0) return;
    this.selectedIndex = Math.max(0, Math.min(rowCount - 1, this.selectedIndex + delta));
    this.options.requestRender();
  }

  private clampSelection(rowCount: number): void {
    if (rowCount === 0) {
      this.selectedIndex = 0;
      return;
    }
    this.selectedIndex = Math.max(0, Math.min(rowCount - 1, this.selectedIndex));
  }

  private async steerSelectedWorker(): Promise<void> {
    const symphonyState = this.options.getState();
    const issueRows = buildIssueRows(symphonyState);
    const escalationRows = buildEscalationRows(symphonyState);
    this.clampSelection(issueRows.length + escalationRows.length);
    const issue = issueRows[this.selectedIndex];
    if (!issue || issue.kind !== "running") {
      this.options.notify("Select a running worker before steering", "warning");
      return;
    }

    try {
      const instruction = (await this.options.prompt("Steer Symphony worker", `Instruction for ${issue.issueIdentifier}`))?.trim();
      if (!instruction) return;

      await this.options.steer(issue.issueIdentifier, instruction);
      this.options.notify(`Steer delivered to ${issue.issueIdentifier}`, "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }

  private async respondToSelectedEscalation(): Promise<void> {
    const symphonyState = this.options.getState();
    const issueRows = buildIssueRows(symphonyState);
    const escalationRows = buildEscalationRows(symphonyState);
    this.clampSelection(issueRows.length + escalationRows.length);
    const escalation = escalationRows[this.selectedIndex - issueRows.length];
    if (!escalation) {
      this.options.notify("Select an escalation before responding", "warning");
      return;
    }

    try {
      const value = (await this.options.prompt("Respond to Symphony escalation", `Response for ${escalation.requestId}`))?.trim();
      if (!value) return;

      await this.options.respondToEscalation(escalation.requestId, parseEscalationResponseInput(value));
      this.options.notify(`Escalation response sent for ${escalation.requestId}`, "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }

  private async refresh(): Promise<void> {
    if (this.refreshing) return;

    this.refreshing = true;
    this.options.requestRender();
    try {
      await this.options.refresh();
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.refreshing = false;
      this.options.requestRender();
    }
  }
}

function renderIssueTable(rows: IssueRow[], selectedIndex: number, selectedOffset: number, theme?: ConsoleTheme): string[] {
  const lines = [color(theme, "dim", "sel issue    kind       status              attempt host      detail")];
  if (rows.length === 0) return [...lines, color(theme, "dim", "-   none")];

  for (const [index, row] of rows.entries()) {
    const rowIndex = selectedOffset + index;
    const selected = rowIndex === selectedIndex ? ">" : " ";
    const line = [
      selected,
      pad(row.issueIdentifier, 8),
      pad(row.kind, 10),
      pad(row.status, 18),
      pad(row.attempt, 7),
      pad(row.workerHost, 9),
      issueDetail(row),
    ].join(" ");
    lines.push(rowIndex === selectedIndex ? selectedLine(theme, line) : line);
  }
  return lines;
}

function renderTriageTable(rows: TriageRow[], selectedIndex: number, selectedOffset: number, theme?: ConsoleTheme): string[] {
  const lines = [color(theme, "dim", "sel issue    attempt harness  model                 event            message")];
  if (rows.length === 0) return [...lines, color(theme, "dim", "-   none")];

  for (const [index, row] of rows.entries()) {
    const rowIndex = selectedOffset + index;
    const selected = rowIndex === selectedIndex ? ">" : " ";
    const line = [
      selected,
      pad(row.issueIdentifier, 8),
      pad(row.attempt, 7),
      pad(row.harness, 8),
      pad(row.model, 21),
      pad(row.lastEvent, 16),
      row.message,
    ].join(" ");
    lines.push(rowIndex === selectedIndex ? selectedLine(theme, line) : line);
  }
  return lines;
}

function renderSelectedIssueDetails(issue: IssueRow | undefined, showDetails: boolean, theme?: ConsoleTheme): string[] {
  if (!showDetails) return [];
  if (!issue) return [color(theme, "dim", "none")];

  const lines = [
    `issue: ${color(theme, "accent", issue.issueIdentifier)} ${issue.title}`,
    `kind: ${issue.kind}`,
    `status: ${issue.status}`,
  ];

  if (issue.kind === "running") {
    lines.push(
      `tracker state: ${color(theme, "success", issue.trackerState)}`,
      `attempt: ${issue.attempt}`,
      `turns: ${issue.turnCount} / ${issue.maxTurns}`,
      `last activity: ${color(theme, "dim", issue.lastActivity)}`,
      `worker host: ${issue.workerHost}`,
      `workspace: ${color(theme, "dim", issue.workspacePath)}`,
      `error: ${issue.errorPreview === "-" ? color(theme, "dim", issue.errorPreview) : color(theme, "error", issue.errorPreview)}`,
    );
  } else if (issue.kind === "triage") {
    lines.push(
      `stage: triage`,
      `attempt: ${issue.attempt}`,
      `harness: ${issue.workerHost}`,
      `last activity: ${color(theme, "dim", issue.lastActivity)}`,
      `session: ${color(theme, "dim", issue.workspacePath)}`,
      `detail: ${issue.title}`,
    );
  } else if (issue.kind === "retry") {
    lines.push(
      `attempt: ${issue.attempt}`,
      `worker host: ${issue.workerHost}`,
      `workspace: ${color(theme, "dim", issue.workspacePath)}`,
      `error: ${issue.errorPreview === "-" ? color(theme, "dim", issue.errorPreview) : color(theme, "error", issue.errorPreview)}`,
    );
  } else if (issue.kind === "blocked") {
    lines.push(`tracker state: ${color(theme, "warning", issue.trackerState)}`, `blockers: ${issue.blockers}`);
  } else {
    lines.push(`completed at: ${issue.completedAt}`);
  }

  return lines;
}

function renderEscalationTable(rows: EscalationRow[], selectedIndex: number, theme?: ConsoleTheme): string[] {
  const lines = [color(theme, "dim", "sel request  issue    method     timeout   preview")];
  if (rows.length === 0) return [...lines, color(theme, "dim", "-   no pending escalations")];

  for (const [index, row] of rows.entries()) {
    const selected = index === selectedIndex ? ">" : " ";
    const line = [selected, pad(row.requestId, 8), pad(row.issueIdentifier, 8), pad(row.method, 10), pad(row.timeout, 8), row.preview].join(" ");
    lines.push(index === selectedIndex ? selectedLine(theme, line) : line);
  }
  return lines;
}

function renderSelectedEscalationDetails(escalation: EscalationRow | undefined, showDetails: boolean, theme?: ConsoleTheme): string[] {
  if (!showDetails) return [];
  if (!escalation) return [color(theme, "dim", "none")];

  return [
    `request: ${color(theme, "accent", escalation.requestId)}`,
    `issue: ${escalation.issueIdentifier}`,
    `method: ${escalation.method}`,
    `created: ${color(theme, "dim", escalation.createdAt)}`,
    `timeout: ${escalation.timeout}`,
    `preview: ${escalation.preview}`,
  ];
}

function parseEscalationResponseInput(value: string): unknown {
  const trimmedValue = value.trimStart();
  try {
    return JSON.parse(trimmedValue);
  } catch {
    const firstChar = trimmedValue[0];
    if (firstChar === "{" || firstChar === "[" || firstChar === "\"") {
      throw new Error("Escalation response must be valid JSON or plain text");
    }
    return value;
  }
}

function issueDetail(issue: IssueRow): string {
  if (issue.kind === "blocked") return `blockers: ${issue.blockers}`;
  if (issue.kind === "completed") return issue.completedAt;
  if (issue.kind === "retry") return issue.errorPreview;
  return issue.errorPreview === "-" ? issue.lastActivity : issue.errorPreview;
}

function renderRecentEvents(events: string[], theme?: ConsoleTheme): string[] {
  if (events.length === 0) return [color(theme, "dim", "none")];
  return events.map((event) => colorEventRow(event, theme));
}

function renderActionLegend(refreshing: boolean, width: number, theme?: ConsoleTheme): string[] {
  const keyboard = "Keyboard: ctrl+shift+↑/↓ select  •  ctrl+shift+r refresh  •  ctrl+shift+t steer  •  ctrl+shift+e escalation  •  ctrl+shift+i details  •  ctrl+shift+q close";
  const commands = "Commands: /symphony:refresh | /symphony:status | /symphony:stop";
  if (refreshing) return [color(theme, "warning", "refreshing..."), commands];
  if (visibleLength(keyboard) <= width - 4) return [keyboard, commands];
  return [
    "Keyboard: ctrl+shift+↑/↓ select  •  ctrl+shift+r refresh  •  ctrl+shift+t steer",
    "          ctrl+shift+e escalation  •  ctrl+shift+i details  •  ctrl+shift+q close",
    commands,
  ];
}

function colorEventRow(event: string, theme?: ConsoleTheme): string {
  if (event.includes(" error ")) return color(theme, "error", event);
  if (event.includes(" warn ") || event.includes(" warning ")) return color(theme, "warning", event);
  if (event.includes(" debug ")) return color(theme, "dim", event);
  if (event.includes(" info ")) return color(theme, "success", event);
  return event;
}

function boxLines(title: string, content: string[], width: number, theme?: ConsoleTheme): string[] {
  const innerWidth = Math.max(20, width - 4);
  const titleText = ` ${title} `;
  const border = (value: string) => color(theme, "borderAccent", value);
  const top = `${border("┌")}${border("─".repeat(1))}${bold(theme, titleText)}${border("─".repeat(Math.max(1, innerWidth + 1 - visibleLength(titleText))))}${border("┐")}`;
  const body = content.length === 0 ? [color(theme, "dim", "none")] : content;
  return [
    top,
    ...body.map((line) => `${border("│")} ${padVisible(line, innerWidth)} ${border("│")}`),
    `${border("└")}${border("─".repeat(innerWidth + 2))}${border("┘")}`,
  ];
}

function color(theme: ConsoleTheme | undefined, name: ConsoleThemeColor, value: string): string {
  return isConsoleTheme(theme) ? theme.fg(name, value) : value;
}

function bold(theme: ConsoleTheme | undefined, value: string): string {
  return isConsoleTheme(theme) ? theme.bold(value) : value;
}

function selectedLine(theme: ConsoleTheme | undefined, value: string): string {
  const styled = color(theme, "accent", bold(theme, value));
  return isConsoleTheme(theme) && theme.bg ? theme.bg("selectedBg", styled) : styled;
}

function isConsoleTheme(theme: ConsoleTheme | undefined): theme is ConsoleTheme {
  if (!theme) return false;
  return typeof theme.fg === "function" && typeof theme.bold === "function";
}

function pad(value: string, width: number): string {
  if (value.length >= width) return value.slice(0, width);
  return value.padEnd(width, " ");
}

function padVisible(value: string, width: number): string {
  const visible = visibleLength(value);
  if (visible >= width) return value;
  return `${value}${" ".repeat(width - visible)}`;
}

function visibleLength(value: string): number {
  return stripAnsi(value).length;
}

function stripAnsi(value: string): string {
  return value.replace(/\u001b\[[0-9;]*m/g, "");
}

export async function openConsole(ctx: ExtensionContext, runtime: SymphonyRuntime): Promise<void> {
  if (!runtime.client) {
    ctx.ui.notify("No Symphony server is attached. Use /symphony:start or /symphony:attach first.", "warning");
    return;
  }

  try {
    await runtime.refreshState();
  } catch (error) {
    ctx.ui.notify(runtime.errorText(error), "error");
  }

  ctx.ui.setWidget("symphony-console", (tui, theme) => {
    let eventStream: EventStreamHandle | undefined;
    let lastEventStreamErrorMessage: string | undefined;
    let closed = false;
    let liveRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    let liveRefreshInFlight = false;
    let liveRefreshPending = false;

    const closeWidget = () => {
      closed = true;
      if (liveRefreshTimer) clearTimeout(liveRefreshTimer);
      eventStream?.close();
      if (activeConsole === component) activeConsole = undefined;
    };

    const scheduleLiveRefresh = () => {
      liveRefreshPending = true;
      if (liveRefreshTimer) return;
      liveRefreshTimer = setTimeout(() => {
        liveRefreshTimer = undefined;
        if (!liveRefreshPending) return;
        liveRefreshPending = false;
        void runLiveRefresh();
      }, 100);
    };

    const runLiveRefresh = async () => {
      if (closed) return;
      if (liveRefreshInFlight) {
        liveRefreshPending = true;
        return;
      }
      liveRefreshInFlight = true;
      try {
        await runtime.refreshState();
      } catch (error) {
        ctx.ui.notify(`Symphony state refresh failed: ${runtime.errorText(error)}`, "warning");
      } finally {
        liveRefreshInFlight = false;
        tui.requestRender();
        if (liveRefreshPending && !closed) scheduleLiveRefresh();
      }
    };

    if (runtime.state.attachedBaseUrl) {
      eventStream = startSymphonyEventStream({
        baseUrl: runtime.state.attachedBaseUrl,
        onEvent: (event) => {
          runtime.recordEvent(event);
          tui.requestRender();
          scheduleLiveRefresh();
        },
        onError: (error) => {
          if (lastEventStreamErrorMessage === error.message) return;
          lastEventStreamErrorMessage = error.message;
          ctx.ui.notify(`Symphony event stream unavailable: ${error.message}`, "warning");
        },
      });
    }

    const component = new SymphonyConsoleComponent({
      state: runtime.state,
      getState: () => runtime.lastState,
      getEvents: () => runtime.recentEvents,
      refresh: async () => {
        await runtime.requestRefresh();
      },
      steer: async (issueIdentifier, instruction) => {
        await runtime.steerWorker(issueIdentifier, instruction);
      },
      respondToEscalation: async (requestId, response) => {
        await runtime.respondToEscalation(requestId, response);
      },
      prompt: async (title, label) => ctx.ui.input(title, label),
      close: () => {
        closeWidget();
        ctx.ui.setWidget("symphony-console", undefined);
      },
      requestRender: () => tui.requestRender(),
      notify: (message, level) => ctx.ui.notify(message, level),
      theme,
    });
    activeConsole = component;
    return Object.assign(component, { dispose: closeWidget });
  });
}
