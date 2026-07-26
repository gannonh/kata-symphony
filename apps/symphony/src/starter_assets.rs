use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct StarterAsset {
    pub path: &'static str,
    pub contents: &'static str,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InitSummary {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

pub const STARTER_ASSETS: &[StarterAsset] = &[
    StarterAsset {
        path: ".symphony/WORKFLOW.md",
        contents: include_str!("../WORKFLOW.md"),
    },
    StarterAsset {
        path: ".symphony/.env.example",
        contents: include_str!("../.env.example"),
    },
    StarterAsset {
        path: ".symphony/prompts/system.md",
        contents: include_str!("../prompts/system.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/supervisor.md",
        contents: include_str!("../prompts/supervisor.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/repo.md",
        contents: include_str!("../prompts/repo.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/in-progress.md",
        contents: include_str!("../prompts/in-progress.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/agent-review.md",
        contents: include_str!("../prompts/agent-review.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/merging.md",
        contents: include_str!("../prompts/merging.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/rework.md",
        contents: include_str!("../prompts/rework.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/triage.md",
        contents: include_str!("../prompts/triage.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/spec-draft.md",
        contents: include_str!("../prompts/spec-draft.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/spec-review.md",
        contents: include_str!("../prompts/spec-review.md"),
    },
    StarterAsset {
        path: ".symphony/prompts/spec-revise.md",
        contents: include_str!("../prompts/spec-revise.md"),
    },
    StarterAsset {
        path: ".symphony/docs/WORKFLOW-REFERENCE.md",
        contents: include_str!("../docs/WORKFLOW-REFERENCE.md"),
    },
];

/// Initialize the `.symphony/` project home under `root`.
///
/// For each entry in [`STARTER_ASSETS`]:
/// - if the destination exists and `force` is `false`, the path is recorded in
///   [`InitSummary::skipped`];
/// - otherwise the bundled contents are written, replacing any existing file,
///   including user customizations when `force` is `true`.
///
/// No backup files are created, and [`InitSummary::written`] does not
/// distinguish newly-created files from overwritten files. On partial failure,
/// files written before the error remain on disk.
pub fn init_project_home(root: &Path, force: bool) -> std::io::Result<InitSummary> {
    let mut summary = InitSummary::default();

    for asset in STARTER_ASSETS {
        let path = root.join(asset.path);
        if path.exists() && !force {
            summary.skipped.push(path);
            continue;
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, asset.contents)?;
        summary.written.push(path);
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_assets_include_project_home_tree() {
        let paths = STARTER_ASSETS
            .iter()
            .map(|asset| asset.path)
            .collect::<Vec<_>>();

        assert!(paths.contains(&".symphony/WORKFLOW.md"));
        assert!(paths.contains(&".symphony/.env.example"));
        assert!(paths.contains(&".symphony/prompts/system.md"));
        assert!(paths.contains(&".symphony/prompts/repo.md"));
        assert!(paths.contains(&".symphony/prompts/triage.md"));
        assert!(paths.contains(&".symphony/prompts/spec-draft.md"));
        assert!(paths.contains(&".symphony/prompts/spec-review.md"));
        assert!(paths.contains(&".symphony/prompts/spec-revise.md"));
        assert!(paths.contains(&".symphony/docs/WORKFLOW-REFERENCE.md"));
        assert!(!paths.contains(&".symphony/prompts/repo-mono.md"));
        assert!(!paths.contains(&".symphony/prompts/repo-cli.md"));
        assert!(!paths.contains(&".symphony/prompts/repo-desktop.md"));
        assert!(!paths.contains(&".symphony/prompts/repo-sym.md"));
    }

    #[test]
    fn init_project_home_writes_and_skips_without_force() {
        let temp = tempfile::tempdir().expect("temp dir");

        let first = init_project_home(temp.path(), false).expect("init should write");
        assert_eq!(first.written.len(), STARTER_ASSETS.len());
        assert!(first.skipped.is_empty());

        let second = init_project_home(temp.path(), false).expect("init should skip");
        assert!(second.written.is_empty());
        assert_eq!(second.skipped.len(), STARTER_ASSETS.len());

        assert!(temp.path().join(".symphony/WORKFLOW.md").is_file());
        assert!(temp.path().join(".symphony/.env.example").is_file());
        assert!(temp.path().join(".symphony/prompts/system.md").is_file());
    }

    #[test]
    fn init_project_home_overwrites_with_force() {
        let temp = tempfile::tempdir().expect("temp dir");
        let workflow_asset = STARTER_ASSETS
            .iter()
            .find(|asset| asset.path == ".symphony/WORKFLOW.md")
            .expect("workflow asset");
        let workflow_path = temp.path().join(workflow_asset.path);

        std::fs::create_dir_all(workflow_path.parent().expect("parent dir"))
            .expect("create parent dir");
        std::fs::write(&workflow_path, "user customization").expect("write custom workflow");

        let summary = init_project_home(temp.path(), true).expect("force init should write");
        assert_eq!(summary.written.len(), STARTER_ASSETS.len());
        assert!(summary.skipped.is_empty());
        assert!(summary.written.contains(&workflow_path));

        let contents = std::fs::read_to_string(&workflow_path).expect("read workflow");
        assert_eq!(contents, workflow_asset.contents);
    }
}
