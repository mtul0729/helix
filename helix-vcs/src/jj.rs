use anyhow::{bail, Context, Result};
use arc_swap::ArcSwap;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use crate::FileChange;

pub fn get_diff_base(file: &Path) -> Result<Vec<u8>> {
    debug_assert!(!file.exists() || file.is_file());
    debug_assert!(file.is_absolute());

    let repo_dir = get_repo_dir(file)?;
    let output = jj(repo_dir)
        .args(["file", "show", "--ignore-working-copy", "-r", "@-", "--"])
        .arg(file)
        .output()
        .context("failed to run `jj file show`")?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        bail!("{}", command_error("jj file show", &output.stderr));
    }
}

pub fn get_current_head_name(file: &Path) -> Result<Arc<ArcSwap<Box<str>>>> {
    let repo_dir = get_repo_dir(file)?;
    let output = jj(repo_dir)
        .args([
            "log",
            "--ignore-working-copy",
            "--no-graph",
            "-r",
            "@-",
            "-T",
            "change_id.short()",
        ])
        .output()
        .context("failed to run `jj log`")?;

    if !output.status.success() {
        bail!("{}", command_error("jj log", &output.stderr));
    }

    let name = String::from_utf8(output.stdout)
        .context("`jj log` output was not UTF-8")?
        .trim()
        .to_owned();

    Ok(Arc::new(ArcSwap::from_pointee(name.into_boxed_str())))
}

pub fn for_each_changed_file(cwd: &Path, f: impl Fn(Result<FileChange>) -> bool) -> Result<()> {
    let root = get_workspace_root(cwd)?;
    let output = jj(cwd)
        .args(["diff", "--summary", "-r", "@"])
        .output()
        .context("failed to run `jj diff --summary`")?;

    if !output.status.success() {
        bail!("{}", command_error("jj diff --summary", &output.stderr));
    }

    let stdout =
        String::from_utf8(output.stdout).context("`jj diff --summary` output was not UTF-8")?;
    for line in stdout.lines() {
        let Some(change) = parse_summary_line(&root, line) else {
            continue;
        };

        if !f(Ok(change)) {
            break;
        }
    }

    Ok(())
}

fn get_repo_dir(file: &Path) -> Result<&Path> {
    file.parent().context("file has no parent directory")
}

fn get_workspace_root(cwd: &Path) -> Result<PathBuf> {
    let output = jj(cwd)
        .args(["root", "--ignore-working-copy"])
        .output()
        .context("failed to run `jj root`")?;

    if !output.status.success() {
        bail!("{}", command_error("jj root", &output.stderr));
    }

    let root = String::from_utf8(output.stdout).context("`jj root` output was not UTF-8")?;
    Ok(PathBuf::from(root.trim()))
}

fn jj(cwd: &Path) -> Command {
    let mut command = Command::new("jj");
    command
        .current_dir(cwd)
        .arg("--no-pager")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    command
}

fn parse_summary_line(root: &Path, line: &str) -> Option<FileChange> {
    let (kind, path) = line.split_once(' ')?;
    let path = root.join(path.trim());

    match kind {
        "A" => Some(FileChange::Untracked { path }),
        "M" => Some(FileChange::Modified { path }),
        "D" => Some(FileChange::Deleted { path }),
        "C" => Some(FileChange::Conflict { path }),
        _ => None,
    }
}

fn command_error(command: &str, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("`{command}` failed")
    } else {
        format!("`{command}` failed: {stderr}")
    }
}
