//! OS process identity for triage attempts.
//!
//! A restart has to decide whether the process group recorded against an
//! abandoned attempt is still *that* process, or whether the PID has since been
//! reused by something unrelated. Comparing the PID alone is not enough, so an
//! attempt also records its process group and the OS-provided process start
//! token. The executable is retained as diagnostic context, but is not stable
//! identity: a launcher can legitimately replace itself with a worker through
//! `exec` while keeping the same PID, process group, and start token.

use std::path::{Path, PathBuf};

/// Identity of a spawned triage child, captured immediately after spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: i64,
    pub process_group_id: i64,
    /// OS-provided start marker. `None` when this platform exposes none, which
    /// forces recovery to skip signalling rather than risk a reused PID.
    pub start_token: Option<String>,
    /// Executable observed when the process was captured. This is diagnostic
    /// context only because a legitimate child may replace it through `exec`.
    pub executable: Option<String>,
}

/// Diagnostic comparison of the executable recorded at spawn with the live
/// executable observed during recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableComparison {
    Unchanged,
    Changed {
        recorded: Option<String>,
        live: Option<String>,
    },
}

/// Why recovery refused to signal a recorded process group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalBlockReason {
    InvalidPidOrGroup,
    OwnProcessGroup,
    MissingStartToken,
    ProcessNotFound,
    LiveIdentityUnavailable,
    ProcessGroupMismatch { recorded: i64, live: i64 },
    StartTokenMismatch { recorded: String, live: String },
}

impl std::fmt::Display for SignalBlockReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPidOrGroup => formatter.write_str("invalid pid or process group"),
            Self::OwnProcessGroup => formatter.write_str("recorded group is Symphony's group"),
            Self::MissingStartToken => formatter.write_str("recorded start token is missing"),
            Self::ProcessNotFound => formatter.write_str("recorded process is no longer present"),
            Self::LiveIdentityUnavailable => {
                formatter.write_str("live process start token is unavailable")
            }
            Self::ProcessGroupMismatch { recorded, live } => {
                write!(
                    formatter,
                    "process group mismatch (recorded {recorded}, live {live})"
                )
            }
            Self::StartTokenMismatch { .. } => formatter.write_str("process start token mismatch"),
        }
    }
}

/// Recovery's authorization decision for a recorded child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signalability {
    Authorized { executable: ExecutableComparison },
    Denied(SignalBlockReason),
}

/// Verified result of bounded process-group termination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationOutcome {
    Terminated,
    NoLongerSignalable(SignalBlockReason),
    StillRunning,
}

/// Capture identity for a live `pid`, reading its current process group.
pub fn capture(pid: u32) -> ProcessIdentity {
    let pid = i64::from(pid);
    ProcessIdentity {
        pid,
        process_group_id: process_group_of(pid).unwrap_or(pid),
        start_token: start_token(pid),
        executable: executable(pid),
    }
}

/// Capture identity for a child we just spawned.
///
/// A child placed in its own group calls `setpgid` after `fork`, so reading the
/// group here would race it and could return *Symphony's* group. When the
/// caller asked for a new group the id is known to equal the child pid, so it
/// is recorded directly rather than read back.
pub fn capture_child(pid: u32, group_leader: bool) -> ProcessIdentity {
    let mut identity = capture(pid);
    if group_leader {
        identity.process_group_id = identity.pid;
    }
    identity
}

/// Whether the live process still matches the recorded stable identity.
///
/// Executable equality is intentionally excluded because `exec` changes the
/// executable without changing the process identity that authorizes recovery.
pub fn matches(recorded: &ProcessIdentity) -> bool {
    let Some(recorded_token) = recorded.start_token.as_ref() else {
        return false;
    };
    if recorded.pid <= 0 || recorded.process_group_id <= 0 {
        return false;
    }
    process_group_of(recorded.pid) == Some(recorded.process_group_id)
        && start_token(recorded.pid).as_ref() == Some(recorded_token)
}

/// Assess whether recovery may signal this attempt's recorded process group.
pub fn assess_signalability(recorded: &ProcessIdentity) -> Signalability {
    if recorded.pid <= 0 || recorded.process_group_id <= 0 {
        return Signalability::Denied(SignalBlockReason::InvalidPidOrGroup);
    }
    if recorded.process_group_id == own_process_group() {
        return Signalability::Denied(SignalBlockReason::OwnProcessGroup);
    }
    let Some(recorded_token) = recorded.start_token.as_ref() else {
        return Signalability::Denied(SignalBlockReason::MissingStartToken);
    };
    let Some(live_group) = process_group_of(recorded.pid) else {
        return Signalability::Denied(SignalBlockReason::ProcessNotFound);
    };
    if live_group != recorded.process_group_id {
        return Signalability::Denied(SignalBlockReason::ProcessGroupMismatch {
            recorded: recorded.process_group_id,
            live: live_group,
        });
    }
    let Some(live_token) = start_token(recorded.pid) else {
        return Signalability::Denied(SignalBlockReason::LiveIdentityUnavailable);
    };
    if &live_token != recorded_token {
        return Signalability::Denied(SignalBlockReason::StartTokenMismatch {
            recorded: recorded_token.clone(),
            live: live_token,
        });
    }

    let live_executable = executable(recorded.pid);
    let executable = if live_executable == recorded.executable {
        ExecutableComparison::Unchanged
    } else {
        ExecutableComparison::Changed {
            recorded: recorded.executable.clone(),
            live: live_executable,
        }
    };
    Signalability::Authorized { executable }
}

/// Boolean compatibility wrapper for callers that do not need diagnostics.
pub fn is_signalable(recorded: &ProcessIdentity) -> bool {
    matches!(
        assess_signalability(recorded),
        Signalability::Authorized { .. }
    )
}

fn own_process_group() -> i64 {
    process_group_of(std::process::id() as i64).unwrap_or(-1)
}

fn process_group_of(pid: i64) -> Option<i64> {
    // SAFETY: getpgid is a plain POSIX query with no memory effects.
    let pgid = unsafe { getpgid(pid as i32) };
    (pgid >= 0).then_some(i64::from(pgid))
}

unsafe extern "C" {
    fn getpgid(pid: i32) -> i32;
}

#[cfg(target_os = "linux")]
fn start_token(pid: i64) -> Option<String> {
    // Field 22 of /proc/<pid>/stat is starttime in clock ticks since boot.
    // Field 2 (comm) may contain spaces inside parentheses, so split after it.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.split_whitespace().nth(19).map(str::to_string)
}

#[cfg(target_os = "linux")]
fn executable(pid: i64) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|path| path.display().to_string())
}

#[cfg(not(target_os = "linux"))]
fn start_token(pid: i64) -> Option<String> {
    ps_field(pid, "lstart=")
}

#[cfg(not(target_os = "linux"))]
fn executable(pid: i64) -> Option<String> {
    ps_field(pid, "comm=")
}

/// `ps` is the portable source of process start time and command outside Linux.
#[cfg(not(target_os = "linux"))]
fn ps_field(pid: i64, format: &str) -> Option<String> {
    let output = std::process::Command::new("ps")
        .arg("-o")
        .arg(format)
        .arg("-p")
        .arg(pid.to_string())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Directory name prefix the runner gives every attempt's disposable root.
pub const ATTEMPT_DIR_PREFIX: &str = "triage-";

/// The attempt root holding a recorded workspace, or `None` when the recorded
/// path is not one the triage runner created.
///
/// Cleanup deletes recursively, so it only ever accepts a directory named
/// `triage-<attempt id>`. A stale, hand-edited, or corrupted `workspace_path`
/// therefore cannot make recovery delete a real repository or the workspace
/// root itself.
pub fn attempt_root_for_cleanup(workspace_path: &Path) -> Option<PathBuf> {
    let root = workspace_path.parent()?;
    let name = root.file_name()?.to_str()?;
    name.starts_with(ATTEMPT_DIR_PREFIX)
        .then(|| root.to_path_buf())
}

/// Terminate the recorded process group after re-checking stable identity:
/// `SIGTERM`, then `SIGKILL` if a running member remains after
/// [`FORCE_KILL_WAIT`].
pub async fn terminate_process_group(recorded: &ProcessIdentity) -> TerminationOutcome {
    if let Signalability::Denied(reason) = assess_signalability(recorded) {
        return TerminationOutcome::NoLongerSignalable(reason);
    }

    let group = match i32::try_from(recorded.process_group_id) {
        Ok(group) if group > 0 => group,
        _ => {
            return TerminationOutcome::NoLongerSignalable(SignalBlockReason::InvalidPidOrGroup);
        }
    };

    // SAFETY: kill against a negative pid signals the process group; the call
    // has no memory effects and a failure only means the group is already gone.
    unsafe { kill(-group, SIGTERM) };

    let deadline = std::time::Instant::now() + FORCE_KILL_WAIT;
    while std::time::Instant::now() < deadline {
        if !process_group_has_running_members(group) {
            return TerminationOutcome::Terminated;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    unsafe { kill(-group, SIGKILL) };

    let deadline = std::time::Instant::now() + FORCE_KILL_CONFIRM_WAIT;
    while std::time::Instant::now() < deadline {
        if !process_group_has_running_members(group) {
            return TerminationOutcome::Terminated;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    if process_group_has_running_members(group) {
        TerminationOutcome::StillRunning
    } else {
        TerminationOutcome::Terminated
    }
}

const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;
const FORCE_KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
const FORCE_KILL_CONFIRM_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(target_os = "linux")]
fn process_group_has_running_members(process_group_id: i32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        // Fail closed when the process table cannot be inspected.
        return true;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i64>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some((_, after_comm)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = after_comm.split_whitespace();
        let Some(state) = fields.next() else {
            continue;
        };
        let _parent_pid = fields.next();
        let Some(group) = fields.next().and_then(|value| value.parse::<i32>().ok()) else {
            continue;
        };
        if group == process_group_id && state != "Z" && state != "X" {
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "linux"))]
fn process_group_has_running_members(process_group_id: i32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-g", &process_group_id.to_string()])
        .output();
    let Ok(output) = output else {
        return true;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .any(|state| !state.is_empty() && !state.starts_with('Z') && !state.starts_with('X'))
}

/// Deterministic launcher-to-worker fixture shared by recovery tests.
#[cfg(test)]
pub(crate) struct ExecTransitionChild {
    child: Option<std::process::Child>,
    identity: ProcessIdentity,
    release_path: PathBuf,
    _temp: tempfile::TempDir,
}

#[cfg(test)]
impl ExecTransitionChild {
    pub(crate) fn spawn() -> Self {
        use std::os::unix::process::CommandExt;

        let temp = tempfile::tempdir().expect("create exec-transition fixture");
        let release_path = temp.path().join("release.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&release_path)
            .status()
            .expect("run mkfifo");
        assert!(status.success(), "mkfifo must create the release barrier");

        let mut command = std::process::Command::new("sh");
        command
            .args([
                "-c",
                "read -r _ < \"$1\"; exec sleep 60",
                "symphony-test-launcher",
            ])
            .arg(&release_path)
            .process_group(0);
        let child = command.spawn().expect("spawn launcher");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let identity = loop {
            let candidate = capture_child(child.id(), true);
            let settled = process_group_of(candidate.pid) == Some(candidate.process_group_id)
                && candidate.start_token.is_some()
                && candidate
                    .executable
                    .as_deref()
                    .is_some_and(|value| value.contains("sh") || value.contains("dash"));
            if settled {
                break candidate;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "launcher must settle before identity capture"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };

        Self {
            child: Some(child),
            identity,
            release_path,
            _temp: temp,
        }
    }

    pub(crate) fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }

    pub(crate) fn release_and_wait_for_exec(&self) {
        use std::io::Write;

        let mut release = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.release_path)
            .expect("open release barrier");
        release.write_all(b"go\n").expect("release launcher");
        drop(release);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if matches!(
                assess_signalability(&self.identity),
                Signalability::Authorized {
                    executable: ExecutableComparison::Changed { .. }
                }
            ) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "launcher must replace itself while stable identity remains"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    pub(crate) fn wait_for_exit(&mut self) -> bool {
        let mut child = self.child.take().expect("child has not been reaped");
        child.wait().is_ok()
    }
}

#[cfg(test)]
impl Drop for ExecTransitionChild {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if let Ok(group) = i32::try_from(self.identity.process_group_id) {
            // SAFETY: this fixture owns the foreign process group.
            unsafe { kill(-group, SIGKILL) };
        }
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A live child must be identifiable, otherwise recovery could never
    /// terminate a real orphan.
    #[test]
    fn captures_identity_for_a_live_process() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let identity = capture(child.id());

        assert_eq!(identity.pid, i64::from(child.id()));
        assert!(
            identity.start_token.is_some(),
            "start token must be captured"
        );
        assert!(identity.executable.is_some(), "executable must be captured");
        assert!(
            matches(&identity),
            "live process must match its own identity"
        );

        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A recorded token that no longer matches means the PID was reused, so
    /// signalling it would kill an unrelated process.
    #[test]
    fn rejects_identity_whose_start_token_changed() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let mut identity = capture(child.id());
        identity.start_token = Some("not-the-recorded-token".to_string());

        assert!(!matches(&identity));

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn executable_drift_after_exec_remains_signalable() {
        let fixture = ExecTransitionChild::spawn();
        let recorded = fixture.identity().clone();

        fixture.release_and_wait_for_exec();

        assert_eq!(
            assess_signalability(&recorded),
            Signalability::Authorized {
                executable: ExecutableComparison::Changed {
                    recorded: recorded.executable.clone(),
                    live: executable(recorded.pid),
                },
            }
        );
    }

    #[test]
    fn missing_executable_is_diagnostic_not_authorization() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sleep");
        command.arg("30").process_group(0);
        let mut child = command.spawn().expect("spawn sleep");
        let mut identity = capture_child(child.id(), true);
        identity.executable = None;

        assert!(is_signalable(&identity));
        assert!(matches!(
            assess_signalability(&identity),
            Signalability::Authorized { .. }
        ));

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn rejects_identity_whose_process_group_changed() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sleep");
        command.arg("30").process_group(0);
        let mut child = command.spawn().expect("spawn sleep");
        let mut identity = capture_child(child.id(), true);
        identity.process_group_id += 1;

        assert!(matches!(
            assess_signalability(&identity),
            Signalability::Denied(SignalBlockReason::ProcessGroupMismatch { .. })
        ));

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn rejects_identity_for_dead_process() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sleep");
        command.arg("30").process_group(0);
        let mut child = command.spawn().expect("spawn sleep");
        let identity = capture_child(child.id(), true);
        child.kill().expect("kill child");
        child.wait().expect("reap child");

        assert_eq!(
            assess_signalability(&identity),
            Signalability::Denied(SignalBlockReason::ProcessNotFound)
        );
    }

    /// A child placed in its own group calls `setpgid` after `fork`, so reading
    /// the group back can still see Symphony's group and make recovery refuse
    /// to terminate a real orphan. Recording the known group id avoids the race.
    #[test]
    fn child_group_is_recorded_without_racing_setpgid() {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sleep");
        command.arg("30").process_group(0);
        let mut child = command.spawn().expect("spawn sleep");

        let identity = capture_child(child.id(), true);

        assert_eq!(identity.process_group_id, identity.pid);
        assert_ne!(
            identity.process_group_id,
            own_process_group(),
            "a group-leader child must never be recorded as Symphony's group"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// A child that never got its own process group shares Symphony's, so
    /// signalling that group would kill the orchestrator. Recovery must refuse
    /// even though every identity field matches a live process.
    #[test]
    fn refuses_to_signal_symphonys_own_process_group() {
        let identity = capture(std::process::id());

        assert!(matches(&identity), "self identity matches by construction");
        assert!(
            !is_signalable(&identity),
            "must never signal our own process group"
        );
    }

    /// Cleanup removes directories recursively, so it must only ever accept a
    /// root the triage runner created. Anything else could be a real repository
    /// or the shared workspace root.
    #[test]
    fn cleanup_root_accepts_only_runner_created_attempt_dirs() {
        assert_eq!(
            attempt_root_for_cleanup(Path::new("/srv/workspaces/triage-abc123/workspace")),
            Some(PathBuf::from("/srv/workspaces/triage-abc123"))
        );
        assert_eq!(
            attempt_root_for_cleanup(Path::new("/home/dev/my-project/workspace")),
            None,
            "a path outside a triage attempt dir must never be deleted"
        );
        assert_eq!(
            attempt_root_for_cleanup(Path::new("/srv/workspaces")),
            None,
            "the workspace root itself must never be deleted"
        );
    }

    /// An orphaned triage child must actually die, otherwise a restart leaves
    /// an agent running against a workspace it is about to delete.
    #[tokio::test]
    async fn terminates_a_live_process_group() {
        let mut fixture = ExecTransitionChild::spawn();
        let identity = fixture.identity().clone();
        fixture.release_and_wait_for_exec();

        assert_eq!(
            terminate_process_group(&identity).await,
            TerminationOutcome::Terminated
        );
        assert!(fixture.wait_for_exit(), "child must be reaped");
    }

    /// An attempt recorded without a usable identity must never be signalled.
    #[test]
    fn rejects_identity_missing_start_token() {
        let identity = ProcessIdentity {
            pid: 1,
            process_group_id: 1,
            start_token: None,
            executable: Some("/bin/sleep".to_string()),
        };

        assert!(!matches(&identity));
        assert_eq!(
            assess_signalability(&identity),
            Signalability::Denied(SignalBlockReason::MissingStartToken)
        );
    }
}
