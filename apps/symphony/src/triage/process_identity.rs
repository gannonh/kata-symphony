//! OS process identity for triage attempts.
//!
//! A restart has to decide whether the process group recorded against an
//! abandoned attempt is still *that* process, or whether the PID has since been
//! reused by something unrelated. Comparing the PID alone is not enough, so an
//! attempt also records the OS-provided process start token and the executable
//! the child was running. Signalling only happens when every recorded field
//! still matches the live process.

use std::path::{Path, PathBuf};

/// Identity of a spawned triage child, captured immediately after spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: i64,
    pub process_group_id: i64,
    /// OS-provided start marker. `None` when this platform exposes none, which
    /// forces recovery to skip signalling rather than risk a reused PID.
    pub start_token: Option<String>,
    /// Executable backing the process, used to reject a reused PID that now
    /// runs an unrelated program.
    pub executable: Option<String>,
}

/// Capture identity for `pid`, reading the real process group rather than
/// assuming the child leads its own.
pub fn capture(pid: u32) -> ProcessIdentity {
    let pid = i64::from(pid);
    ProcessIdentity {
        pid,
        process_group_id: process_group_of(pid).unwrap_or(pid),
        start_token: start_token(pid),
        executable: executable(pid),
    }
}

/// Whether the live process still matches every recorded identity field.
///
/// Missing recorded or live values are treated as a mismatch: an attempt whose
/// identity cannot be proven must never be signalled.
pub fn matches(recorded: &ProcessIdentity) -> bool {
    let (Some(recorded_token), Some(recorded_exe)) = (&recorded.start_token, &recorded.executable)
    else {
        return false;
    };
    if recorded.pid <= 0 {
        return false;
    }

    let live = capture(recorded.pid as u32);
    live.process_group_id == recorded.process_group_id
        && live.start_token.as_ref() == Some(recorded_token)
        && live.executable.as_ref() == Some(recorded_exe)
}

/// Whether recovery may signal this attempt's recorded process group.
///
/// Beyond a full identity match the group must be a real, foreign group: a
/// child that never got its own group would share Symphony's, and signalling
/// that would take down the orchestrator itself.
pub fn is_signalable(recorded: &ProcessIdentity) -> bool {
    if recorded.process_group_id <= 0 || recorded.process_group_id == own_process_group() {
        return false;
    }
    matches(recorded)
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

/// Send a bounded termination to `process_group_id`: `SIGTERM`, then `SIGKILL`
/// if the group is still alive after [`FORCE_KILL_WAIT`].
pub async fn terminate_process_group(process_group_id: i64) {
    let group = match i32::try_from(process_group_id) {
        Ok(group) if group > 0 => group,
        _ => return,
    };

    // SAFETY: kill against a negative pid signals the process group; the call
    // has no memory effects and a failure only means the group is already gone.
    unsafe { kill(-group, SIGTERM) };

    let deadline = std::time::Instant::now() + FORCE_KILL_WAIT;
    while std::time::Instant::now() < deadline {
        if unsafe { kill(-group, 0) } != 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    unsafe { kill(-group, SIGKILL) };
}

const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;
const FORCE_KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
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
        let mut child = tokio::process::Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .expect("spawn sleep");
        let pid = child.id().expect("child pid") as i64;
        let identity = capture(pid as u32);
        assert!(
            is_signalable(&identity),
            "a foreign group leader must be signalable"
        );

        terminate_process_group(identity.process_group_id).await;

        let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
            .await
            .expect("child must exit after termination");
        assert!(status.is_ok());
    }

    /// An attempt recorded without a usable identity must never be signalled.
    #[test]
    fn rejects_identity_missing_os_fields() {
        let identity = ProcessIdentity {
            pid: 1,
            process_group_id: 1,
            start_token: None,
            executable: Some("/bin/sleep".to_string()),
        };

        assert!(!matches(&identity));
    }
}
