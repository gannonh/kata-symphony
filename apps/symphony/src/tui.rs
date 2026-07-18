use std::collections::VecDeque;
use std::io;
use std::io::IsTerminal;
use std::time::Duration;

use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::domain::{EventSeverity, OrchestratorSnapshot, SupervisorStatus, SymphonyEventEnvelope};
use crate::event_stream::EventHub;
use crate::orchestrator::SnapshotHandle;
use crate::session_summary::{
    compact_session_id as compact_session_id_value, truncate_for_display,
};

const REFRESH_INTERVAL_MS: u64 = 500;
const CURRENT_TPS_WINDOW_MS: i64 = 5_000;
const SPARKLINE_WINDOW_MS: i64 = 10 * 60 * 1_000;
const SPARKLINE_BUCKETS: usize = 24;
const SPARKLINE_BUCKET_MS: i64 = SPARKLINE_WINDOW_MS / SPARKLINE_BUCKETS as i64;
const SPARKLINE_BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const STALE_ACTIVITY_THRESHOLD_MS: i64 = 120_000;
const LAST_EVENT_COLUMN_WIDTH: u16 = 24;
const MESSAGE_COLUMN_TRUNCATE_WIDTH: usize = 60;
const ACTIVITY_LOG_CAPACITY: usize = 200;
const ACTIVITY_LOG_MESSAGE_WIDTH: usize = 100;
const PINNED_ERROR_LIMIT: usize = 20;
const RUNNING_SESSIONS_HEIGHT: u16 = 10;

#[derive(Debug, Default)]
struct ThroughputTracker {
    token_samples: Vec<(i64, u64)>,
    event_samples: Vec<(i64, u64)>,
}

impl ThroughputTracker {
    fn record_sample(&mut self, timestamp_ms: i64, total_tokens: u64, total_events: u64) {
        Self::record_counter_sample(
            &mut self.token_samples,
            timestamp_ms,
            total_tokens,
            "tokens",
        );
        Self::record_counter_sample(
            &mut self.event_samples,
            timestamp_ms,
            total_events,
            "events",
        );
    }

    fn record_counter_sample(
        samples: &mut Vec<(i64, u64)>,
        timestamp_ms: i64,
        total_value: u64,
        sample_name: &str,
    ) {
        match samples.last_mut() {
            Some((last_ts, _last_total)) if timestamp_ms < *last_ts => {
                tracing::warn!(
                    sample = sample_name,
                    last_ts = *last_ts,
                    received_ts = timestamp_ms,
                    "throughput tracker received out-of-order timestamp; skipping sample"
                );
                return;
            }
            Some((last_ts, last_total)) if timestamp_ms == *last_ts => {
                *last_total = (*last_total).max(total_value);
                return;
            }
            Some((_, last_total)) if total_value < *last_total => {
                samples.clear();
            }
            _ => {}
        }

        samples.push((timestamp_ms, total_value));
        Self::trim_samples(samples, timestamp_ms);
    }

    fn throughput_line(&self, now_ms: i64) -> String {
        let current_tps = self.current_tps(now_ms);
        if current_tps > f64::EPSILON {
            let sparkline = self.sparkline(now_ms);
            return format!("Throughput: {current_tps:.1} tps {sparkline}");
        }

        let event_rate = self.event_rate(now_ms);
        if event_rate > f64::EPSILON {
            let sparkline = self.sparkline_for_samples(now_ms, &self.event_samples);
            return format!("Throughput: {event_rate:.1} eps {sparkline}");
        }

        let sparkline = self.sparkline(now_ms);
        format!("Throughput: 0.0 tps {sparkline}")
    }

    fn current_tps(&self, now_ms: i64) -> f64 {
        self.rate_for_samples(now_ms, &self.token_samples)
    }

    fn event_rate(&self, now_ms: i64) -> f64 {
        self.rate_for_samples(now_ms, &self.event_samples)
    }

    fn rate_for_samples(&self, now_ms: i64, samples: &[(i64, u64)]) -> f64 {
        if samples.len() < 2 {
            return 0.0;
        }
        let window_start = now_ms.saturating_sub(CURRENT_TPS_WINDOW_MS);
        let delta = self.window_delta(samples, window_start, now_ms);
        // Intentionally divide by the fixed 5s window so the dashboard readout
        // stays stable; this smooths noise but under-reports during warm-up.
        let window_seconds = (CURRENT_TPS_WINDOW_MS as f64) / 1_000.0;
        if window_seconds > 0.0 {
            delta / window_seconds
        } else {
            0.0
        }
    }

    fn sparkline(&self, now_ms: i64) -> String {
        self.sparkline_for_samples(now_ms, &self.token_samples)
    }

    fn sparkline_for_samples(&self, now_ms: i64, samples: &[(i64, u64)]) -> String {
        let bucket_rates = self.bucket_rates(now_ms, samples);
        let max_rate = bucket_rates
            .iter()
            .copied()
            .fold(0.0_f64, |acc, value| acc.max(value));

        if max_rate <= f64::EPSILON {
            return std::iter::repeat_n(SPARKLINE_BLOCKS[0], SPARKLINE_BUCKETS).collect();
        }

        bucket_rates
            .into_iter()
            .map(|value| {
                let ratio = (value / max_rate).clamp(0.0, 1.0);
                let idx = (ratio * (SPARKLINE_BLOCKS.len() as f64 - 1.0)).round() as usize;
                SPARKLINE_BLOCKS[idx]
            })
            .collect()
    }

    fn bucket_rates(&self, now_ms: i64, samples: &[(i64, u64)]) -> Vec<f64> {
        let window_start = now_ms.saturating_sub(SPARKLINE_WINDOW_MS);
        let mut bucket_totals = vec![0.0; SPARKLINE_BUCKETS];

        for sample_pair in samples.windows(2) {
            let (start_ms, start_total) = sample_pair[0];
            let (end_ms, end_total) = sample_pair[1];

            if end_ms <= start_ms {
                continue;
            }
            if end_ms <= window_start || start_ms >= now_ms {
                continue;
            }

            let interval_total = end_total.saturating_sub(start_total) as f64;
            if interval_total <= 0.0 {
                continue;
            }

            let interval_start = start_ms.max(window_start);
            let interval_end = end_ms.min(now_ms);
            if interval_end <= interval_start {
                continue;
            }

            let total_per_ms = interval_total / (end_ms - start_ms) as f64;
            let mut cursor = interval_start;
            while cursor < interval_end {
                let bucket_index = (((cursor - window_start) / SPARKLINE_BUCKET_MS) as usize)
                    .min(SPARKLINE_BUCKETS.saturating_sub(1));
                let bucket_end =
                    (window_start + ((bucket_index + 1) as i64 * SPARKLINE_BUCKET_MS)).min(now_ms);
                let segment_end = bucket_end.min(interval_end);
                if segment_end <= cursor {
                    break;
                }
                let overlap_ms = (segment_end - cursor) as f64;
                bucket_totals[bucket_index] += total_per_ms * overlap_ms;
                cursor = segment_end;
            }
        }

        let bucket_seconds = (SPARKLINE_BUCKET_MS as f64) / 1_000.0;
        if bucket_seconds <= 0.0 {
            return vec![0.0; SPARKLINE_BUCKETS];
        }

        bucket_totals
            .into_iter()
            .map(|bucket_total| bucket_total / bucket_seconds)
            .collect()
    }

    fn trim_samples(samples: &mut Vec<(i64, u64)>, now_ms: i64) {
        if samples.len() < 2 {
            return;
        }

        let cutoff = now_ms.saturating_sub(SPARKLINE_WINDOW_MS);
        let first_in_window = samples
            .iter()
            .position(|(ts, _)| *ts >= cutoff)
            .unwrap_or_else(|| samples.len().saturating_sub(1));
        let keep_from = first_in_window.saturating_sub(1);
        if keep_from > 0 {
            samples.drain(0..keep_from);
        }
    }

    fn window_delta(
        &self,
        samples: &[(i64, u64)],
        window_start_ms: i64,
        window_end_ms: i64,
    ) -> f64 {
        if window_end_ms <= window_start_ms {
            return 0.0;
        }

        let mut total = 0.0;
        for sample_pair in samples.windows(2) {
            let (start_ms, start_total) = sample_pair[0];
            let (end_ms, end_total) = sample_pair[1];

            if end_ms <= start_ms {
                continue;
            }
            if end_ms <= window_start_ms || start_ms >= window_end_ms {
                continue;
            }

            let overlap_start = start_ms.max(window_start_ms);
            let overlap_end = end_ms.min(window_end_ms);
            if overlap_end <= overlap_start {
                continue;
            }

            let interval_total = end_total.saturating_sub(start_total) as f64;
            let overlap_ms = (overlap_end - overlap_start) as f64;
            let interval_ms = (end_ms - start_ms) as f64;
            total += interval_total * (overlap_ms / interval_ms);
        }

        total
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiExitReason {
    ShutdownSignal,
    CtrlC,
    SetupFailed,
    InputError,
    DrawError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivityLogEntry {
    timestamp: DateTime<Utc>,
    severity: EventSeverity,
    issue: Option<String>,
    event: String,
    message: Option<String>,
}

#[derive(Debug, Default)]
struct ActivityLog {
    entries: VecDeque<ActivityLogEntry>,
}

impl ActivityLog {
    fn push(&mut self, entry: ActivityLogEntry) {
        self.entries.push_back(entry);
        while self.entries.len() > ACTIVITY_LOG_CAPACITY {
            let _ = self.entries.pop_front();
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn has_errors(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.severity == EventSeverity::Error)
    }
}

pub fn validate_terminal_for_tui() -> Result<(), String> {
    if !io::stdout().is_terminal() {
        return Err("stdout is not a terminal; --tui requires an interactive terminal".to_string());
    }

    enable_raw_mode().map_err(|err| format!("failed to enable raw mode: {err}"))?;
    let mut stdout = io::stdout();
    if let Err(err) = execute!(stdout, EnterAlternateScreen, LeaveAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(format!(
            "failed to enter/leave alternate screen during TUI preflight: {err}"
        ));
    }
    disable_raw_mode().map_err(|err| format!("failed to disable raw mode: {err}"))?;
    Ok(())
}

pub async fn run_tui(
    snapshot_handle: SnapshotHandle,
    event_hub: EventHub,
    mut shutdown: watch::Receiver<bool>,
) -> TuiExitReason {
    let mut terminal = match setup_terminal() {
        Ok(terminal) => terminal,
        Err(err) => {
            tracing::error!(error = %err, "failed to initialize tui terminal");
            return TuiExitReason::SetupFailed;
        }
    };

    let mut ticker = tokio::time::interval(Duration::from_millis(REFRESH_INTERVAL_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticker.tick().await;
    let mut throughput_tracker = ThroughputTracker::default();
    let mut event_rx = event_hub.subscribe();
    let mut activity_log = ActivityLog::default();
    let exit_reason = loop {
        match ctrl_c_pressed() {
            Ok(true) => break TuiExitReason::CtrlC,
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(error = %err, "failed reading terminal input for ctrl+c");
                break TuiExitReason::InputError;
            }
        }

        drain_activity_events(&mut event_rx, &mut activity_log);
        let snapshot = snapshot_handle.read();
        let now = Utc::now();
        let now_ms = now.timestamp_millis();
        throughput_tracker.record_sample(
            now_ms,
            snapshot.codex_totals.total_tokens,
            snapshot.codex_totals.event_count,
        );
        let throughput_line = throughput_tracker.throughput_line(now_ms);
        if let Err(err) = terminal
            .draw(|frame| draw_dashboard(frame, &snapshot, &activity_log, now, &throughput_line))
        {
            tracing::error!(error = %err, "failed drawing tui frame");
            break TuiExitReason::DrawError;
        }

        tokio::select! {
            _ = ticker.tick() => {}
            changed = shutdown.changed() => {
                match changed {
                    Ok(()) => {
                        if *shutdown.borrow() {
                            break TuiExitReason::ShutdownSignal;
                        }
                    }
                    Err(_) => break TuiExitReason::ShutdownSignal,
                }
            }
        }
    };

    if let Err(err) = restore_terminal(&mut terminal) {
        tracing::warn!(error = %err, "failed restoring terminal after tui shutdown");
    }

    exit_reason
}

fn ctrl_c_pressed() -> io::Result<bool> {
    while event::poll(Duration::from_millis(0))? {
        if let Event::Key(key_event) = event::read()? {
            let ctrl_c = key_event.modifiers.contains(KeyModifiers::CONTROL)
                && key_event.code == KeyCode::Char('c')
                && matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat);
            if ctrl_c {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn drain_activity_events(
    event_rx: &mut tokio::sync::broadcast::Receiver<SymphonyEventEnvelope>,
    activity_log: &mut ActivityLog,
) {
    loop {
        match event_rx.try_recv() {
            Ok(envelope) => activity_log.push(activity_entry_from_envelope(envelope)),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(count)) => {
                activity_log.push(ActivityLogEntry {
                    timestamp: Utc::now(),
                    severity: EventSeverity::Warn,
                    issue: None,
                    event: "event_log_lagged".to_string(),
                    message: Some(format!("missed {count} events")),
                });
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
        }
    }
}

fn activity_entry_from_envelope(envelope: SymphonyEventEnvelope) -> ActivityLogEntry {
    ActivityLogEntry {
        timestamp: envelope.timestamp,
        severity: envelope.severity,
        issue: envelope.issue,
        event: envelope.event,
        message: activity_message_from_payload(&envelope.payload),
    }
}

fn activity_message_from_payload(payload: &serde_json::Value) -> Option<String> {
    for key in [
        "error_preview",
        "output_preview",
        "command_preview",
        "tool_args_preview",
        "summary",
        "error",
        "instruction_preview",
        "issue_identifier",
        "request_id",
        "session_id",
    ] {
        if let Some(value) = payload.get(key).and_then(|value| value.as_str()) {
            let normalized = normalize_whitespace(value);
            if !normalized.is_empty() {
                return Some(truncate_for_display(
                    &normalized,
                    ACTIVITY_LOG_MESSAGE_WIDTH,
                ));
            }
        }
    }

    let normalized = normalize_whitespace(&payload.to_string());
    (!normalized.is_empty() && normalized != "{}")
        .then(|| truncate_for_display(&normalized, ACTIVITY_LOG_MESSAGE_WIDTH))
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if let Err(err) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(err);
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(err) => {
            cleanup_partial_terminal_state();
            return Err(err);
        }
    };
    if let Err(err) = terminal.clear() {
        cleanup_partial_terminal_state();
        return Err(err);
    }
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let raw_result = disable_raw_mode();
    let screen_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let cursor_result = terminal.show_cursor();

    raw_result?;
    screen_result?;
    cursor_result
}

fn cleanup_partial_terminal_state() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen);
}

fn build_summary_lines(snapshot: &OrchestratorSnapshot, throughput_line: &str) -> Vec<String> {
    let mut counts = vec![format!("Running: {}", snapshot.running.len())];
    if !snapshot.pending_escalations.is_empty() {
        counts.push(format!(
            "Escalations: {}",
            snapshot.pending_escalations.len()
        ));
    }
    counts.push(format!("Retry: {}", snapshot.retry_queue.len()));
    counts.push(format!("Completed: {}", snapshot.completed.len()));
    counts.push(format!(
        "Tokens: {}",
        format_tokens(snapshot.codex_totals.total_tokens)
    ));

    if snapshot.shared_context.total_entries > 0 {
        counts.push(format!(
            "Context: {} entries",
            snapshot.shared_context.total_entries
        ));
    }

    let mut lines = vec![counts.join("  |  "), throughput_line.to_string()];
    lines.push(supervisor_summary_line(snapshot));

    if let Some(project_url) = snapshot.tracker_project_url.as_deref() {
        lines.push(format!("Project: {project_url}"));
    }

    lines
}

fn supervisor_summary_line(snapshot: &OrchestratorSnapshot) -> String {
    let label = match snapshot.supervisor.status {
        SupervisorStatus::Disabled => "⚪ disabled",
        SupervisorStatus::Starting => "🟡 starting",
        SupervisorStatus::Active => "🟢 active",
        SupervisorStatus::Stopped => "⚫ stopped",
        SupervisorStatus::Failed => "🔴 failed",
    };

    if snapshot.supervisor.status == SupervisorStatus::Disabled {
        return format!("Supervisor: {label}");
    }

    format!(
        "Supervisor: {label} | {} steers | {} conflicts | {} patterns | {} escalations",
        snapshot.supervisor.steers_issued,
        snapshot.supervisor.conflicts_detected,
        snapshot.supervisor.patterns_detected,
        snapshot.supervisor.escalations_created,
    )
}

fn draw_dashboard(
    frame: &mut Frame,
    snapshot: &OrchestratorSnapshot,
    activity_log: &ActivityLog,
    now: DateTime<Utc>,
    throughput_line: &str,
) {
    let root = Block::default()
        .title("Symphony Dashboard")
        .borders(Borders::ALL);
    frame.render_widget(root, frame.area());
    let inner = Block::default().borders(Borders::ALL).inner(frame.area());

    let summary_lines_data = build_summary_lines(snapshot, throughput_line);
    let summary_height = summary_lines_data.len() as u16;
    let has_blocked = !snapshot.blocked.is_empty();
    let has_pinned_errors = activity_log.has_errors();
    let has_triage = !snapshot.triage_sessions.is_empty();
    let blocked_height = if has_blocked {
        // borders (2) + header (1) + data rows
        (snapshot.blocked.len() as u16 + 3).min(8)
    } else {
        0
    };
    let triage_height = if has_triage {
        (snapshot.triage_sessions.len() as u16 + 4).min(9)
    } else {
        0
    };
    let pinned_error_height = if has_pinned_errors { 5 } else { 0 };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(summary_height),
            Constraint::Length(RUNNING_SESSIONS_HEIGHT),
            Constraint::Length(triage_height),
            Constraint::Length(blocked_height),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(pinned_error_height),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(inner);

    let summary_lines: Vec<Line<'static>> =
        summary_lines_data.into_iter().map(Line::from).collect();
    frame.render_widget(Paragraph::new(summary_lines), sections[0]);

    let running_header = Row::new(vec![
        Cell::from(""),
        Cell::from("ID"),
        Cell::from("Session"),
        Cell::from("State"),
        Cell::from("Turn"),
        Cell::from("Last Event"),
        Cell::from("Message"),
        Cell::from("Last Activity"),
        Cell::from("Tokens"),
        Cell::from("Model"),
        Cell::from("Host"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));
    let running_rows = running_rows(snapshot, now);
    let running_table = Table::new(
        running_rows,
        [
            Constraint::Length(2),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(LAST_EVENT_COLUMN_WIDTH),
            Constraint::Min(16),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Min(20),
            Constraint::Length(10),
        ],
    )
    .header(running_header)
    .block(
        Block::default()
            .title("Running Sessions")
            .borders(Borders::ALL),
    );
    frame.render_widget(running_table, sections[1]);

    // Triage attempts (between Running and Blocked)
    if has_triage {
        let triage_table = Table::new(
            triage_rows(snapshot, now),
            [
                Constraint::Length(2),
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Length(8),
                Constraint::Length(LAST_EVENT_COLUMN_WIDTH),
                Constraint::Min(16),
                Constraint::Length(14),
                Constraint::Length(12),
                Constraint::Min(20),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(vec![
                Cell::from(""),
                Cell::from("ID"),
                Cell::from("Session"),
                Cell::from("State"),
                Cell::from("Turn"),
                Cell::from("Last Event"),
                Cell::from("Message"),
                Cell::from("Last Activity"),
                Cell::from("Tokens"),
                Cell::from("Model"),
                Cell::from("Harness"),
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .title("Triage")
                .title_style(Style::default().fg(Color::Cyan))
                .borders(Borders::ALL),
        );
        frame.render_widget(triage_table, sections[2]);
    }

    // Blocked section (between Running and Retry)
    if has_blocked {
        let blocked_header = Row::new(vec![
            Cell::from("ID"),
            Cell::from("State"),
            Cell::from("Blocked By"),
        ])
        .style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        );
        let blocked_rows: Vec<Row<'static>> = snapshot
            .blocked
            .iter()
            .map(|entry| {
                Row::new(vec![
                    Cell::from(entry.identifier.clone()),
                    Cell::from(entry.state.clone()),
                    Cell::from(entry.blocker_identifiers.join(", ")),
                ])
            })
            .collect();
        let blocked_table = Table::new(
            blocked_rows,
            [
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Min(20),
            ],
        )
        .header(blocked_header)
        .block(
            Block::default()
                .title("Blocked")
                .title_style(Style::default().fg(Color::Yellow))
                .borders(Borders::ALL),
        );
        frame.render_widget(blocked_table, sections[3]);
    }

    let retry_header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("Attempt"),
        Cell::from("Retry In"),
        Cell::from("Host"),
        Cell::from("Error"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));
    let retry_rows = retry_rows(snapshot);
    let retry_table = Table::new(
        retry_rows,
        [
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(16),
        ],
    )
    .header(retry_header)
    .block(Block::default().title("Retry Queue").borders(Borders::ALL));
    frame.render_widget(retry_table, sections[4]);

    let completed_items = completed_items(snapshot);
    let completed_list =
        List::new(completed_items).block(Block::default().title("Completed").borders(Borders::ALL));
    frame.render_widget(completed_list, sections[5]);

    if has_pinned_errors {
        let pinned_error_items = pinned_error_items(activity_log, now, sections[6].height);
        let pinned_error_list = List::new(pinned_error_items).block(
            Block::default()
                .title("Pinned Errors")
                .title_style(Style::default().fg(Color::Red))
                .borders(Borders::ALL),
        );
        frame.render_widget(pinned_error_list, sections[6]);
    }

    let activity_items = activity_items(activity_log, now, sections[7].height);
    let activity_title = format!("Activity Log (last {})", activity_log.len());
    let activity_list = List::new(activity_items)
        .block(Block::default().title(activity_title).borders(Borders::ALL));
    frame.render_widget(activity_list, sections[7]);

    let polling_last = snapshot
        .polling
        .last_poll_at
        .as_deref()
        .and_then(parse_rfc3339)
        .map(|ts| format_age(Some(ts), now))
        .unwrap_or_else(|| "never".to_string());
    let polling_line = format!(
        "Polling: last {}  count: {}  interval: {}",
        polling_last,
        snapshot.polling.poll_count,
        format_duration(snapshot.polling.poll_interval_ms as i64),
    );
    let rate_summary = rate_summary(snapshot, now);
    let footer = Paragraph::new(vec![Line::from(polling_line), Line::from(rate_summary)])
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, sections[8]);
}

fn running_rows(snapshot: &OrchestratorSnapshot, now: DateTime<Utc>) -> Vec<Row<'static>> {
    let mut rows = Vec::new();
    let pending_by_issue: std::collections::HashMap<&str, &crate::domain::PendingEscalation> =
        snapshot
            .pending_escalations
            .iter()
            .map(|pending| (pending.issue_id.as_str(), pending))
            .collect();

    for (issue_id, run) in &snapshot.running {
        let metrics = snapshot.running_sessions.get(issue_id);
        let turn_count = metrics.map(|m| m.turn_count).unwrap_or(0);
        let last_activity = metrics
            .and_then(|m| m.last_activity_at.as_ref().cloned())
            .or(Some(run.started_at));
        let total_tokens = metrics.map(|m| m.total_tokens).unwrap_or(0);
        let last_event = metrics.and_then(|m| m.last_event.as_deref());
        let last_event_message = metrics.and_then(|m| m.last_event_message.as_deref());
        let session_id = metrics.and_then(|m| m.session_id.as_deref());
        let current_tool_name = metrics.and_then(|m| m.current_tool_name.as_deref());
        let current_tool_args = metrics.and_then(|m| m.current_tool_args_preview.as_deref());
        let escalation = pending_by_issue.get(issue_id.as_str()).copied();
        let last_error = metrics.and_then(|m| m.last_error.as_deref());
        let stale = escalation.is_none() && is_stale_session(last_activity, now);
        let state = if escalation.is_some() {
            format!(
                "⚠ {}",
                run.tracker_state.as_deref().unwrap_or(run.status.as_str())
            )
        } else {
            run.tracker_state
                .as_deref()
                .unwrap_or(run.status.as_str())
                .to_string()
        };
        let last_activity_text = format_age(last_activity, now);
        let last_activity_cell = if stale {
            Cell::from(Span::styled(
                last_activity_text,
                Style::default().fg(Color::Red),
            ))
        } else {
            Cell::from(last_activity_text)
        };

        let activity_message = if let Some(error) = last_error {
            format!("🚨 {error}")
        } else if let Some(pending) = escalation {
            let waiting = format_age(Some(pending.created_at), now);
            format!("⚠ escalation: \"{}\" ({waiting})", pending.preview)
        } else {
            format_activity_message(current_tool_name, current_tool_args, last_event_message)
        };

        let activity_text = truncate_for_display(&activity_message, MESSAGE_COLUMN_TRUNCATE_WIDTH);
        let activity_cell = if last_error.is_some() {
            Cell::from(Span::styled(activity_text, Style::default().fg(Color::Red)))
        } else {
            Cell::from(activity_text)
        };

        let status_last_activity = if escalation.is_some() {
            Some(now)
        } else {
            last_activity
        };

        let mut row = Row::new(vec![
            Cell::from(status_dot(
                last_event,
                status_last_activity,
                now,
                last_error.is_some(),
            )),
            Cell::from(run.issue_identifier.clone()),
            Cell::from(compact_session_id(session_id)),
            Cell::from(state),
            Cell::from(turn_count.to_string()),
            Cell::from(truncate_for_display(
                last_event.unwrap_or("-"),
                LAST_EVENT_COLUMN_WIDTH as usize,
            )),
            activity_cell,
            last_activity_cell,
            Cell::from(format_tokens(total_tokens)),
            Cell::from(run.model.clone().unwrap_or_else(|| "-".to_string())),
            Cell::from(
                run.worker_host
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| "local".to_string()),
            ),
        ]);

        if escalation.is_some() && last_error.is_none() {
            row = row.style(Style::default().fg(Color::Yellow));
        }

        rows.push(row);
    }

    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from(""),
            Cell::from("(none)"),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ]));
    }

    rows
}

fn triage_rows(snapshot: &OrchestratorSnapshot, now: DateTime<Utc>) -> Vec<Row<'static>> {
    let mut rows = Vec::new();

    for session in &snapshot.triage_sessions {
        let last_activity = session.last_activity_at.or(Some(session.started_at));
        let last_activity_text = format_age(last_activity, now);
        let state = format!("triage#{}", session.attempt);
        let activity_message = format_activity_message(
            session.current_tool_name.as_deref(),
            session.current_tool_args_preview.as_deref(),
            session.last_event_message.as_deref(),
        );
        let activity_text = truncate_for_display(&activity_message, MESSAGE_COLUMN_TRUNCATE_WIDTH);

        rows.push(Row::new(vec![
            Cell::from(status_dot(
                session.last_event.as_deref(),
                last_activity,
                now,
                false,
            )),
            Cell::from(session.issue_identifier.clone()),
            Cell::from(compact_session_id(session.session_id.as_deref())),
            Cell::from(state),
            Cell::from("1"),
            Cell::from(truncate_for_display(
                session.last_event.as_deref().unwrap_or("-"),
                LAST_EVENT_COLUMN_WIDTH as usize,
            )),
            Cell::from(activity_text),
            Cell::from(last_activity_text),
            Cell::from(format_tokens(session.total_tokens)),
            Cell::from(session.model.clone().unwrap_or_else(|| "-".to_string())),
            Cell::from(session.harness.clone()),
        ]));
    }

    if rows.is_empty() {
        rows.push(Row::new(vec![Cell::from(""), Cell::from("(none)")]));
    }

    rows
}

fn compact_session_id(session_id: Option<&str>) -> String {
    session_id
        .map(compact_session_id_value)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

/// Format the activity message for display, preferring current tool info when available.
fn format_activity_message(
    current_tool_name: Option<&str>,
    current_tool_args: Option<&str>,
    fallback_message: Option<&str>,
) -> String {
    if let Some(tool) = current_tool_name {
        match current_tool_args {
            Some(args) if !args.is_empty() => format!("tool: {tool} ({args})"),
            _ => format!("tool: {tool}"),
        }
    } else {
        fallback_message.unwrap_or("-").to_string()
    }
}

fn status_dot(
    last_event: Option<&str>,
    last_activity: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    has_error: bool,
) -> Span<'static> {
    Span::styled(
        "●",
        Style::default().fg(status_color(last_event, last_activity, now, has_error)),
    )
}

fn status_color(
    last_event: Option<&str>,
    last_activity: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    has_error: bool,
) -> Color {
    if has_error {
        return Color::Red;
    }

    if is_stale_session(last_activity, now) {
        return Color::Red;
    }

    let Some(last_event) = last_event else {
        return Color::Red;
    };
    let normalized = last_event.trim().to_ascii_lowercase();

    if is_failure_event(&normalized) {
        Color::Red
    } else if is_turn_completed_event(&normalized) {
        Color::Magenta
    } else if normalized.contains("token_count") {
        Color::Yellow
    } else if normalized.contains("task_started")
        || normalized.contains("tool_call")
        || normalized.contains("item/tool/call")
    {
        Color::Green
    } else {
        Color::Blue
    }
}

fn is_failure_event(normalized_event: &str) -> bool {
    normalized_event.contains("failed")
        || normalized_event.contains("error")
        || normalized_event.contains("cancelled")
        || normalized_event.contains("canceled")
}

fn is_turn_completed_event(normalized_event: &str) -> bool {
    normalized_event.contains("turn_completed")
        || normalized_event.contains("turn/completed")
        || (normalized_event.contains("turn") && normalized_event.contains("completed"))
}

fn is_stale_session(last_activity: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let Some(last_activity) = last_activity else {
        return true;
    };

    (now - last_activity).num_milliseconds() > STALE_ACTIVITY_THRESHOLD_MS
}

fn retry_rows(snapshot: &OrchestratorSnapshot) -> Vec<Row<'static>> {
    let mut rows = Vec::new();
    for retry in &snapshot.retry_queue {
        rows.push(Row::new(vec![
            Cell::from(retry.identifier.clone()),
            Cell::from(retry.attempt.to_string()),
            Cell::from(format_retry_delay(retry.due_in_ms)),
            Cell::from(
                retry
                    .worker_host
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| "local".to_string()),
            ),
            Cell::from(retry.error.clone().unwrap_or_else(|| "-".to_string())),
        ]));
    }

    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from("(empty)"),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ]));
    }

    rows
}

fn completed_items(snapshot: &OrchestratorSnapshot) -> Vec<ListItem<'static>> {
    if snapshot.completed.is_empty() {
        return vec![ListItem::new("(empty)")];
    }

    snapshot
        .completed
        .iter()
        .map(|entry| {
            let timestamp = entry
                .completed_at
                .as_ref()
                .map(|dt| dt.format("%-I:%M %p").to_string())
                .unwrap_or_else(|| "-".to_string());
            ListItem::new(format!(
                "{} - {} ({})",
                entry.identifier, entry.title, timestamp
            ))
        })
        .collect()
}

fn activity_items(
    activity_log: &ActivityLog,
    now: DateTime<Utc>,
    area_height: u16,
) -> Vec<ListItem<'static>> {
    if activity_log.is_empty() {
        return vec![ListItem::new("(no events yet)")];
    }

    let visible_rows = usize::from(area_height.saturating_sub(2)).max(1);
    activity_log
        .entries
        .iter()
        .rev()
        .take(visible_rows)
        .map(|entry| {
            let age = format_age(Some(entry.timestamp), now);
            let issue = entry.issue.as_deref().unwrap_or("-");
            let message = entry.message.as_deref().unwrap_or("-");
            let text = format!(
                "{age:>8}  {:<5}  {:<10}  {:<24}  {}",
                entry.severity.as_str(),
                truncate_for_display(issue, 10),
                truncate_for_display(&entry.event, 24),
                message
            );
            ListItem::new(Line::from(Span::styled(
                text,
                Style::default().fg(activity_severity_color(entry.severity)),
            )))
        })
        .collect()
}

fn pinned_error_items(
    activity_log: &ActivityLog,
    now: DateTime<Utc>,
    area_height: u16,
) -> Vec<ListItem<'static>> {
    let visible_rows = usize::from(area_height.saturating_sub(2)).clamp(1, PINNED_ERROR_LIMIT);
    let items: Vec<ListItem<'static>> = activity_log
        .entries
        .iter()
        .rev()
        .filter(|entry| entry.severity == EventSeverity::Error)
        .take(visible_rows)
        .map(|entry| {
            let age = format_age(Some(entry.timestamp), now);
            let issue = entry.issue.as_deref().unwrap_or("-");
            let message = entry.message.as_deref().unwrap_or("-");
            let text = format!(
                "{age:>8}  {:<10}  {:<24}  {}",
                truncate_for_display(issue, 10),
                truncate_for_display(&entry.event, 24),
                message
            );
            ListItem::new(Line::from(Span::styled(
                text,
                Style::default().fg(Color::Red),
            )))
        })
        .collect();

    if items.is_empty() {
        vec![ListItem::new("(no errors)")]
    } else {
        items
    }
}

fn activity_severity_color(severity: EventSeverity) -> Color {
    match severity {
        EventSeverity::Debug => Color::DarkGray,
        EventSeverity::Info => Color::White,
        EventSeverity::Warn => Color::Yellow,
        EventSeverity::Error => Color::Red,
    }
}

fn rate_summary(snapshot: &OrchestratorSnapshot, now: DateTime<Utc>) -> String {
    let Some(rate_limits) = snapshot.codex_rate_limits.as_ref() else {
        return "Rate: n/a".to_string();
    };

    let parts = summarize_rate_limits(&rate_limits.data, now);
    if parts.is_empty() {
        "Rate: n/a".to_string()
    } else {
        format!("Rate: {}", parts.join("  "))
    }
}

fn summarize_rate_limits(data: &serde_json::Value, now: DateTime<Utc>) -> Vec<String> {
    let mut parts = Vec::new();

    let Some(obj) = data.as_object() else {
        return parts;
    };

    for (name, value) in obj {
        if matches!(name.as_str(), "limit_id" | "limit_name") {
            continue;
        }
        let Some(bucket) = value.as_object() else {
            continue;
        };
        if let Some(text) = bucket_usage_text(name, bucket, now) {
            parts.push(text);
        }
    }

    if parts.is_empty() {
        if let Some(text) = obj
            .get("limit_name")
            .and_then(|v| v.as_str())
            .and_then(|label| bucket_usage_text(label, obj, now))
        {
            parts.push(text);
        } else if let Some(text) = bucket_usage_text("limit", obj, now) {
            parts.push(text);
        }
    }

    parts.sort();
    parts
}

fn bucket_usage_text(
    label: &str,
    bucket: &serde_json::Map<String, serde_json::Value>,
    now: DateTime<Utc>,
) -> Option<String> {
    let remaining = bucket.get("remaining").and_then(as_f64)?;
    let limit = bucket.get("limit").and_then(as_f64)?;
    if limit <= 0.0 {
        return None;
    }

    let used_pct = ((1.0 - (remaining / limit)) * 100.0)
        .clamp(0.0, 100.0)
        .round() as i64;
    let window = bucket_window(bucket, now).unwrap_or_else(|| "-".to_string());
    Some(format!("{label}: {used_pct}% used ({window})"))
}

fn bucket_window(
    bucket: &serde_json::Map<String, serde_json::Value>,
    now: DateTime<Utc>,
) -> Option<String> {
    let reset_at = bucket
        .get("resets_at")
        .and_then(|v| v.as_str())
        .and_then(parse_rfc3339)
        .or_else(|| {
            bucket
                .get("reset_at")
                .and_then(|v| v.as_str())
                .and_then(parse_rfc3339)
        });

    if let Some(reset_at) = reset_at {
        return Some(format_duration((reset_at - now).num_milliseconds()));
    }

    if let Some(reset_seconds) = bucket.get("reset_seconds").and_then(as_f64) {
        return Some(format_duration((reset_seconds * 1000.0) as i64));
    }

    None
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn format_age(timestamp: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(timestamp) = timestamp else {
        return "-".to_string();
    };

    let delta_ms = (now - timestamp).num_milliseconds().max(0);
    if delta_ms < 1_000 {
        "just now".to_string()
    } else {
        format!("{} ago", format_duration(delta_ms))
    }
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_retry_delay(due_in_ms: i64) -> String {
    if due_in_ms <= 0 {
        "ready".to_string()
    } else {
        format_duration(due_in_ms)
    }
}

fn format_duration(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 1_000 {
        return format!("{ms}ms");
    }

    let seconds = ms / 1_000;
    if seconds < 60 {
        return format!("{seconds}s");
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }

    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }

    let days = hours / 24;
    format!("{days}d")
}

fn format_tokens(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|v| v as f64))
        .or_else(|| value.as_i64().map(|v| v as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ratatui::backend::TestBackend;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    fn snapshot_fixture(
        total_tokens: u64,
        tracker_project_url: Option<&str>,
    ) -> OrchestratorSnapshot {
        OrchestratorSnapshot {
            poll_interval_ms: REFRESH_INTERVAL_MS,
            max_concurrent_agents: 1,
            tracker_project_url: tracker_project_url.map(ToString::to_string),
            running: BTreeMap::new(),
            running_sessions: BTreeMap::new(),
            running_session_info: BTreeMap::new(),
            claimed: BTreeSet::new(),
            retry_queue: Vec::new(),
            completed: Vec::new(),
            pending_escalations: Vec::new(),
            codex_totals: crate::domain::CodexTotals {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens,
                event_count: 0,
                seconds_running: 0.0,
            },
            blocked: Vec::new(),
            shared_context: crate::domain::SharedContextSummary::default(),
            supervisor: crate::domain::SupervisorSnapshot::default(),
            codex_rate_limits: None,
            polling: crate::domain::PollingSnapshot {
                checking: false,
                next_poll_in_ms: 0,
                poll_interval_ms: REFRESH_INTERVAL_MS,
                last_poll_at: None,
                poll_count: 0,
            },
            triage_sessions: Vec::new(),
        }
    }

    fn render_text(backend: &TestBackend) -> String {
        let buffer = backend.buffer();
        let area = buffer.area();
        let mut lines = Vec::with_capacity(area.height as usize);
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn assert_approx_eq(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected}, got {actual} (tolerance {tolerance})"
        );
    }

    #[test]
    fn format_age_returns_seconds_ago() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 15, 0, 10)
            .single()
            .expect("valid fixture timestamp");
        let then = Utc
            .with_ymd_and_hms(2026, 3, 22, 15, 0, 8)
            .single()
            .expect("valid fixture timestamp");
        assert_eq!(format_age(Some(then), now), "2s ago");
    }

    #[test]
    fn format_retry_delay_handles_ready_and_future() {
        assert_eq!(format_retry_delay(-100), "ready");
        assert_eq!(format_retry_delay(5_000), "5s");
    }

    #[test]
    fn summarize_rate_limits_extracts_usage() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 10, 0, 0)
            .single()
            .expect("valid fixture timestamp");
        let data = json!({
            "limit_id": "req",
            "primary": {
                "remaining": 82,
                "limit": 100,
                "reset_seconds": 18_000
            },
            "secondary": {
                "remaining": 33,
                "limit": 50,
                "reset_seconds": 604_800
            }
        });

        let summary = summarize_rate_limits(&data, now);
        assert!(
            summary
                .iter()
                .any(|line| line.contains("primary: 18% used")),
            "expected primary usage summary, got: {summary:?}"
        );
        assert!(
            summary
                .iter()
                .any(|line| line.contains("secondary: 34% used")),
            "expected secondary usage summary, got: {summary:?}"
        );
    }

    #[test]
    fn compact_session_id_truncates_to_first_eight_chars() {
        assert_eq!(
            compact_session_id(Some("1234567890abcdef")),
            "12345678".to_string()
        );
        assert_eq!(compact_session_id(None), "-".to_string());
    }

    #[test]
    fn draw_dashboard_truncates_last_event_at_column_width() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 15, 0, 0)
            .single()
            .expect("valid fixture timestamp");
        let mut snapshot = snapshot_fixture(1_337, None);
        let issue_id = "issue-1".to_string();
        let long_event =
            "codex/event/this event label is definitely much longer than twenty four chars";

        snapshot.running.insert(
            issue_id.clone(),
            crate::domain::RunAttempt {
                issue_id: issue_id.clone(),
                issue_identifier: "KAT-898".to_string(),
                issue_title: Some("Refactor helper duplication".to_string()),
                attempt: None,
                workspace_path: "/tmp/workspace".to_string(),
                started_at: now,
                status: "running".to_string(),
                error: None,
                worker_host: None,
                model: None,
                tracker_state: Some("Agent Review".to_string()),
                issue_url: None,
            },
        );
        snapshot.running_sessions.insert(
            issue_id,
            crate::domain::RunningSessionSnapshot {
                turn_count: 3,
                last_activity_at: Some(now),
                total_tokens: 4242,
                last_event: Some(long_event.to_string()),
                last_event_message: Some("message".to_string()),
                session_id: Some("1234567890abcdef".to_string()),
                current_tool_name: None,
                current_tool_args_preview: None,
                last_error: None,
            },
        );

        let backend = TestBackend::new(160, 48);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let activity_log = ActivityLog::default();
        terminal
            .draw(|frame| {
                draw_dashboard(
                    frame,
                    &snapshot,
                    &activity_log,
                    now,
                    "Throughput: 42.3 tps ▁▂▃▄▅▆▇█",
                )
            })
            .expect("dashboard draw should succeed");

        let rendered = render_text(terminal.backend());
        let expected = truncate_for_display(long_event, 24);
        assert!(
            rendered.contains(&expected),
            "expected dashboard output to include truncated last event {expected:?}, got:\n{rendered}"
        );
    }

    #[test]
    fn draw_dashboard_shows_running_error_indicator() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 16, 0, 0)
            .single()
            .expect("valid fixture timestamp");
        let mut snapshot = snapshot_fixture(0, None);
        let issue_id = "issue-error".to_string();

        snapshot.running.insert(
            issue_id.clone(),
            crate::domain::RunAttempt {
                issue_id: issue_id.clone(),
                issue_identifier: "KAT-1660".to_string(),
                issue_title: Some("Worker error visibility".to_string()),
                attempt: Some(1),
                workspace_path: "/tmp/workspace".to_string(),
                started_at: now,
                status: "running".to_string(),
                error: None,
                worker_host: None,
                model: Some("anthropic/claude-sonnet-4-6".to_string()),
                tracker_state: Some("In Progress".to_string()),
                issue_url: None,
            },
        );
        snapshot.running_sessions.insert(
            issue_id,
            crate::domain::RunningSessionSnapshot {
                turn_count: 2,
                last_activity_at: Some(now),
                total_tokens: 1024,
                last_event: Some("codex/event/task_started".to_string()),
                last_event_message: Some("working".to_string()),
                session_id: Some("session-error".to_string()),
                current_tool_name: None,
                current_tool_args_preview: None,
                last_error: Some("rate limit: retry in ~80 min".to_string()),
            },
        );

        let backend = TestBackend::new(200, 48);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let activity_log = ActivityLog::default();
        terminal
            .draw(|frame| {
                draw_dashboard(
                    frame,
                    &snapshot,
                    &activity_log,
                    now,
                    "Throughput: 0.0 tps ▁▁▁▁▁▁▁▁",
                )
            })
            .expect("dashboard draw should succeed");

        let rendered = render_text(terminal.backend());
        assert!(
            rendered.contains("🚨") && rendered.contains("rate limit: retry in ~80 min"),
            "expected running table to show red error indicator text, got:\n{rendered}"
        );
    }

    #[test]
    fn test_tui_renders_github_identifier() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 16, 10, 0)
            .single()
            .expect("valid fixture timestamp");
        let mut snapshot = snapshot_fixture(0, None);
        let issue_id = "issue-github".to_string();

        snapshot.running.insert(
            issue_id.clone(),
            crate::domain::RunAttempt {
                issue_id: issue_id.clone(),
                issue_identifier: "#42".to_string(),
                issue_title: Some("GitHub parity issue".to_string()),
                attempt: Some(1),
                workspace_path: "/tmp/workspace-github".to_string(),
                started_at: now,
                status: "running".to_string(),
                error: None,
                worker_host: None,
                model: None,
                tracker_state: Some("In Progress".to_string()),
                issue_url: Some("https://github.com/owner/repo/issues/42".to_string()),
            },
        );

        let backend = TestBackend::new(180, 48);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let activity_log = ActivityLog::default();
        terminal
            .draw(|frame| {
                draw_dashboard(
                    frame,
                    &snapshot,
                    &activity_log,
                    now,
                    "Throughput: 0.0 tps ▁▁▁▁▁▁▁▁",
                )
            })
            .expect("dashboard draw should succeed");

        let rendered = render_text(terminal.backend());
        assert!(
            rendered.contains("#42"),
            "running table should render github issue identifiers verbatim, got:\n{rendered}"
        );
    }

    #[test]
    fn pinned_errors_prefer_error_preview_from_payload() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 16, 20, 0)
            .single()
            .expect("valid fixture timestamp");
        let envelope = SymphonyEventEnvelope {
            version: "1".to_string(),
            sequence: 1,
            timestamp: now,
            kind: crate::domain::EventKind::Tool,
            severity: EventSeverity::Error,
            issue: Some("#456".to_string()),
            event: "tool_error".to_string(),
            payload: serde_json::json!({
                "summary": "bash",
                "error_preview": "bash: cd apps/symphony && pnpm test"
            }),
        };

        let mut activity_log = ActivityLog::default();
        activity_log.push(activity_entry_from_envelope(envelope));

        let items = pinned_error_items(&activity_log, now, 5);
        let rendered = format!("{items:?}");
        assert!(
            rendered.contains("bash: cd apps/symphony && pnpm test"),
            "pinned errors should show useful command preview, got: {rendered}"
        );
    }

    #[test]
    fn status_color_respects_event_mapping_and_staleness() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 15, 0, 0)
            .single()
            .expect("valid fixture timestamp");
        let fresh = Utc
            .with_ymd_and_hms(2026, 3, 22, 14, 59, 30)
            .single()
            .expect("valid fixture timestamp");
        let stale = Utc
            .with_ymd_and_hms(2026, 3, 22, 14, 55, 0)
            .single()
            .expect("valid fixture timestamp");

        assert_eq!(
            status_color(Some("codex/event/task_started"), Some(fresh), now, false),
            Color::Green
        );
        assert_eq!(
            status_color(Some("codex/event/token_count"), Some(fresh), now, false),
            Color::Yellow
        );
        assert_eq!(
            status_color(Some("turn_completed"), Some(fresh), now, false),
            Color::Magenta
        );
        assert_eq!(
            status_color(Some("codex/event/turn/completed"), Some(fresh), now, false),
            Color::Magenta
        );
        assert_eq!(
            status_color(
                Some("codex/event/tool_call_failed"),
                Some(fresh),
                now,
                false
            ),
            Color::Red
        );
        assert_eq!(
            status_color(Some("startup_failed"), Some(fresh), now, false),
            Color::Red
        );
        assert_eq!(
            status_color(Some("notification"), Some(fresh), now, false),
            Color::Blue
        );
        assert_eq!(status_color(None, Some(fresh), now, false), Color::Red);
        assert_eq!(
            status_color(Some("turn_completed"), Some(stale), now, false),
            Color::Red
        );
        assert_eq!(
            status_color(Some("codex/event/task_started"), Some(fresh), now, true),
            Color::Red
        );
    }

    #[test]
    fn build_summary_lines_includes_tracker_project_url_when_available() {
        let snapshot = snapshot_fixture(1_337, Some("https://linear.app/kata-sh/project/symphony"));
        let throughput_line = "Throughput: 42.3 tps ▁▂▃▄▅▆▇█";
        let lines = build_summary_lines(&snapshot, throughput_line);

        assert!(
            lines.iter().any(|line| line == throughput_line),
            "summary lines should include throughput"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "Project: https://linear.app/kata-sh/project/symphony"),
            "summary lines should include the tracker project URL when configured"
        );
    }

    #[test]
    fn build_summary_lines_adds_context_count_when_present() {
        let mut snapshot = snapshot_fixture(42, None);
        snapshot.shared_context.total_entries = 5;

        let lines = build_summary_lines(&snapshot, "Throughput: 0.0 tps ▁▁▁▁▁▁▁▁");
        assert!(
            lines
                .first()
                .map(|line| line.contains("Context: 5 entries"))
                .unwrap_or(false),
            "summary line should include shared context count when entries exist"
        );
    }

    #[test]
    fn build_summary_lines_includes_disabled_supervisor_status() {
        let snapshot = snapshot_fixture(42, None);
        let lines = build_summary_lines(&snapshot, "Throughput: 0.0 tps ▁▁▁▁▁▁▁▁");

        assert!(
            lines.iter().any(|line| line == "Supervisor: ⚪ disabled"),
            "expected supervisor disabled status line in summary"
        );
    }

    #[test]
    fn build_summary_lines_includes_active_supervisor_counters() {
        let mut snapshot = snapshot_fixture(42, None);
        snapshot.supervisor.status = crate::domain::SupervisorStatus::Active;
        snapshot.supervisor.steers_issued = 3;
        snapshot.supervisor.conflicts_detected = 1;
        snapshot.supervisor.patterns_detected = 2;
        snapshot.supervisor.escalations_created = 1;

        let lines = build_summary_lines(&snapshot, "Throughput: 0.0 tps ▁▁▁▁▁▁▁▁");
        let expected =
            "Supervisor: 🟢 active | 3 steers | 1 conflicts | 2 patterns | 1 escalations";

        assert!(
            lines.iter().any(|line| line == expected),
            "expected active supervisor counters in summary, got {lines:?}"
        );
    }

    #[test]
    fn throughput_tracker_reports_current_tps_from_last_five_seconds() {
        let mut tracker = ThroughputTracker::default();
        tracker.record_sample(0, 0, 0);
        tracker.record_sample(2_500, 50, 0);
        tracker.record_sample(5_000, 100, 0);

        assert_approx_eq(tracker.current_tps(5_000), 20.0, 0.001);
        assert_approx_eq(tracker.event_rate(5_000), 0.0, 0.001);
    }

    #[test]
    fn throughput_tracker_renders_flat_sparkline_for_zero_throughput() {
        let mut tracker = ThroughputTracker::default();
        tracker.record_sample(0, 42, 0);
        tracker.record_sample(SPARKLINE_WINDOW_MS, 42, 0);

        let sparkline = tracker.sparkline(SPARKLINE_WINDOW_MS);
        assert_eq!(sparkline.chars().count(), SPARKLINE_BUCKETS);
        assert!(sparkline.chars().all(|ch| ch == SPARKLINE_BLOCKS[0]));
    }

    #[test]
    fn throughput_tracker_renders_recent_spike_in_last_bucket() {
        let mut tracker = ThroughputTracker::default();
        let now_ms = SPARKLINE_WINDOW_MS;
        tracker.record_sample(now_ms - SPARKLINE_BUCKET_MS, 0, 0);
        tracker.record_sample(now_ms, 2_500, 0);

        let sparkline = tracker.sparkline(now_ms);
        let chars: Vec<char> = sparkline.chars().collect();
        assert_eq!(chars.len(), SPARKLINE_BUCKETS);
        assert!(
            chars[..SPARKLINE_BUCKETS - 1]
                .iter()
                .all(|ch| *ch == SPARKLINE_BLOCKS[0]),
            "expected all leading buckets to be flat, got {sparkline}"
        );
        assert_eq!(
            chars[SPARKLINE_BUCKETS - 1],
            SPARKLINE_BLOCKS[SPARKLINE_BLOCKS.len() - 1]
        );
    }

    #[test]
    fn throughput_tracker_event_based_activity_with_zero_tokens() {
        let mut tracker = ThroughputTracker::default();
        tracker.record_sample(0, 42, 0);
        tracker.record_sample(2_500, 42, 50);
        tracker.record_sample(5_000, 42, 100);

        let throughput = tracker.throughput_line(5_000);
        assert!(
            throughput.contains(" eps "),
            "expected eps fallback when token tps is zero, got {throughput}"
        );
        let sparkline = throughput.split_whitespace().last().unwrap_or_default();
        assert!(
            sparkline.chars().any(|ch| ch != SPARKLINE_BLOCKS[0]),
            "expected event-based sparkline activity, got {throughput}"
        );
        assert_approx_eq(tracker.current_tps(5_000), 0.0, 0.001);
        assert_approx_eq(tracker.event_rate(5_000), 20.0, 0.001);
    }

    #[test]
    fn throughput_tracker_prefers_token_tps_when_available() {
        let mut tracker = ThroughputTracker::default();
        tracker.record_sample(0, 0, 0);
        tracker.record_sample(5_000, 100, 200);

        let throughput = tracker.throughput_line(5_000);
        assert!(
            throughput.contains(" tps "),
            "expected token throughput label when token deltas exist, got {throughput}"
        );
        assert!(
            !throughput.contains(" eps "),
            "token throughput should take precedence over event fallback"
        );
        assert_approx_eq(tracker.current_tps(5_000), 20.0, 0.001);
        assert_approx_eq(tracker.event_rate(5_000), 40.0, 0.001);
    }

    #[test]
    fn throughput_tracker_trims_samples_to_window_with_one_preceding_point() {
        let mut tracker = ThroughputTracker::default();
        tracker.record_sample(0, 0, 0);
        tracker.record_sample(1_000, 1, 1);
        tracker.record_sample(SPARKLINE_WINDOW_MS + 100_000, 2, 2);

        assert_eq!(tracker.token_samples.len(), 2);
        assert_eq!(tracker.token_samples[0].0, 1_000);
        assert_eq!(tracker.event_samples.len(), 2);
        assert_eq!(tracker.event_samples[0].0, 1_000);
    }

    #[test]
    fn draw_dashboard_renders_throughput_row() {
        let now = Utc
            .with_ymd_and_hms(2026, 3, 22, 12, 0, 0)
            .single()
            .expect("valid fixture timestamp");
        let snapshot = snapshot_fixture(1_337, None);
        let throughput_line = "Throughput: 42.3 tps ▁▂▃▄▅▆▇█";

        let backend = TestBackend::new(120, 48);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let activity_log = ActivityLog::default();
        terminal
            .draw(|frame| draw_dashboard(frame, &snapshot, &activity_log, now, throughput_line))
            .expect("dashboard draw should succeed");

        let rendered = render_text(terminal.backend());
        assert!(
            rendered.contains(throughput_line),
            "expected rendered dashboard to contain throughput line, got:\n{rendered}"
        );
    }

    #[test]
    fn validate_terminal_requires_tty_stdout() {
        if io::stdout().is_terminal() {
            return;
        }
        assert!(
            validate_terminal_for_tui().is_err(),
            "non-interactive stdout should fail tui preflight"
        );
    }
}
