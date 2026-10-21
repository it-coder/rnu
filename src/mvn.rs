use crate::error::{Result, RnuError};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

/// Resolve `project`'s dependency closure into `repo` (a local repository
/// directory), without touching the user's `~/.m2/repository`.
///
/// Both `dependency:go-offline` and `install` honour `-Dmaven.repo.local`, so
/// the isolated directory ends up holding exactly what this project needed —
/// which is the whole point: unrelated projects in the shared local repo never
/// leak into the upload set.
///
/// The user's own `settings.xml` is intentionally left in place: projects may
/// legitimately depend on internal artifacts that only exist in the private
/// Nexus, and cutting it out would break resolution.
pub fn resolve_dependencies(
    project: &Path,
    repo: &Path,
    with_project: bool,
    mvn_bin: Option<&Path>,
) -> Result<()> {
    let exe = mvn_executable(mvn_bin);
    let (work_dir, pom) = split_project(project)?;

    let mut cmd = Command::new(&exe);
    cmd.current_dir(&work_dir);
    cmd.arg("-B").arg("-q");
    cmd.arg(format!("-Dmaven.repo.local={}", repo.display()));

    if with_project {
        // `install` also puts this project's own modules into the repo.
        cmd.arg("-DskipTests").arg("install");
    } else {
        cmd.arg("dependency:go-offline");
    }

    if let Some(pom) = pom {
        cmd.arg("-f").arg(pom);
    }

    info!("Running {}", describe(&cmd));
    let output = cmd.output().map_err(|e| {
        RnuError::ConfigError(format!(
            "failed to launch `{}`: {e}\n\
             Install Maven, put it on PATH, or pass --mvn <path> / set RNU_MVN.",
            exe.display()
        ))
    })?;

    if output.status.success() {
        debug!("Maven resolution finished successfully");
        return Ok(());
    }

    let mut detail = String::new();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for part in [stderr.trim(), stdout.trim()] {
        if !part.is_empty() {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(part);
        }
    }

    let tail: Vec<&str> = detail.lines().rev().take(25).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();

    Err(RnuError::ConfigError(format!(
        "`{}` failed ({}){}\n{}",
        exe.display(),
        output
            .status
            .code()
            .map(|c| format!("exit {c}"))
            .unwrap_or_else(|| "terminated by signal".to_string()),
        if tail.is_empty() {
            String::new()
        } else {
            " — last lines:".to_string()
        },
        tail.join("\n")
    )))
}

fn mvn_executable(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Some(from_env) = std::env::var_os("RNU_MVN") {
        return PathBuf::from(from_env);
    }
    if cfg!(windows) {
        PathBuf::from("mvn.cmd")
    } else {
        PathBuf::from("mvn")
    }
}

/// `project` may be a directory containing `pom.xml` or a POM file itself.
fn split_project(project: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if project.is_file() {
        let dir = project
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        return Ok((dir, Some(project.to_path_buf())));
    }

    if !project.is_dir() {
        return Err(RnuError::ConfigError(format!(
            "Project path does not exist: {}",
            project.display()
        )));
    }

    let pom = project.join("pom.xml");
    if !pom.is_file() {
        return Err(RnuError::ConfigError(format!(
            "No pom.xml in {} — pass the directory of a Maven project",
            project.display()
        )));
    }
    Ok((project.to_path_buf(), None))
}

fn describe(cmd: &Command) -> String {
    let mut parts = vec![cmd.get_program().to_string_lossy().to_string()];
    parts.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn rejects_directory_without_pom() {
        let dir = tempdir().unwrap();
        let err = split_project(dir.path()).unwrap_err();
        assert!(err.to_string().contains("No pom.xml"));
    }

    #[test]
    fn accepts_pom_file_and_uses_its_parent() {
        let dir = tempdir().unwrap();
        let pom = dir.path().join("pom.xml");
        fs::write(&pom, "<project/>").unwrap();

        let (work_dir, explicit) = split_project(&pom).unwrap();
        assert_eq!(work_dir, dir.path());
        assert_eq!(explicit.as_deref(), Some(pom.as_path()));
    }

    #[test]
    fn default_executable_name() {
        let exe = mvn_executable(None);
        let name = exe.to_string_lossy().to_string();
        if cfg!(windows) {
            assert_eq!(name, "mvn.cmd");
        } else {
            assert_eq!(name, "mvn");
        }
    }
}
