use crate::error::{Result, RnuError};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Where the isolated repository for a project lives, and whether it already
/// held a previous resolution.
pub struct RepoCache {
    pub dir: PathBuf,
    pub reused: bool,
    /// Kept alive for the duration of the run when the cache directory could
    /// not be determined and a temporary directory had to be used instead.
    _temp: Option<tempfile::TempDir>,
}

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
///
/// Maven's output is consumed line by line and surfaced through a spinner:
/// `-q` used to hide everything, which made a slow first resolution look like a
/// hang. Returns how long the resolution took.
pub fn resolve_dependencies(
    project: &Path,
    repo: &Path,
    with_project: bool,
    mvn_bin: Option<&Path>,
    verbose: bool,
) -> Result<Duration> {
    let exe = mvn_executable(mvn_bin);
    let (work_dir, pom) = split_project(project)?;

    let mut cmd = Command::new(&exe);
    cmd.current_dir(&work_dir);
    // stdin must not be inherited: a repository that asks for credentials would
    // otherwise block forever waiting for input nobody can see.
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // No `-q`: the download lines are the only progress signal we get.
    cmd.arg("-B");
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
    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| {
        RnuError::ConfigError(format!(
            "failed to launch `{}`: {e}\n\
             Install Maven, put it on PATH, or pass --mvn <path> / set RNU_MVN.",
            exe.display()
        ))
    })?;

    let (tx, rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_reader(stdout, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_reader(stderr, tx.clone()));
    }
    drop(tx);

    let spinner = if verbose {
        None
    } else {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_message("resolving dependencies with Maven…");
        Some(pb)
    };

    // Enough history to explain a failure, bounded so a noisy build cannot grow
    // the process without limit.
    let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL_LINES);
    loop {
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(line) => {
                if verbose {
                    eprintln!("{line}");
                }
                if let Some(pb) = &spinner {
                    pb.set_message(summarise(&line));
                }
                tail.push_back(line);
                while tail.len() > TAIL_LINES {
                    tail.pop_front();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(pb) = &spinner {
                    // Keep the elapsed time moving even when Maven is quiet
                    // (e.g. between two large downloads).
                    pb.tick();
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait()?;
    for reader in readers {
        let _ = reader.join();
    }
    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    if status.success() {
        debug!("Maven resolution finished successfully");
        return Ok(started.elapsed());
    }

    let detail: Vec<&str> = tail.iter().map(String::as_str).collect();
    let start = detail.len().saturating_sub(25);
    let tail_text = detail[start..].join("\n");

    Err(RnuError::ConfigError(format!(
        "`{}` failed ({}){}",
        exe.display(),
        status
            .code()
            .map(|c| format!("exit {c}"))
            .unwrap_or_else(|| "terminated by signal".to_string()),
        if tail_text.trim().is_empty() {
            String::new()
        } else {
            format!(" — last lines:\n{tail_text}")
        }
    )))
}

/// Lines kept for the failure report.
const TAIL_LINES: usize = 400;

fn spawn_reader<R: Read + Send + 'static>(stream: R, tx: Sender<String>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}

/// Strip Maven's level prefix and cut the line down to something that fits on
/// one terminal row.
fn summarise(line: &str) -> String {
    let text = line
        .trim_start_matches("[INFO]")
        .trim_start_matches("[WARNING]")
        .trim_start_matches("[DEBUG]")
        .trim();
    if text.is_empty() {
        return "resolving dependencies with Maven…".to_string();
    }
    let mut out: String = text.chars().take(88).collect();
    if text.chars().count() > 88 {
        out.push('…');
    }
    out
}

/// Directory, relative to the working directory, that holds the repository
/// cache. It sits next to `config.toml` so one `rm -rf` cleans up both, and so
/// the artifacts a run may upload never accumulate in the user's home.
pub const CACHE_DIR_NAME: &str = ".rnu";

/// Per-project cache directory, so a second run does not download everything
/// again. One directory per project keeps the "only this project's artifacts"
/// promise intact.
///
/// The cache lives under the **current directory** rather than the platform
/// cache dir: a run is usually tied to one workspace, and keeping the closure
/// next to the project makes it obvious what is on disk and how to reclaim it.
pub fn prepare_repo(project: &Path, fresh: bool) -> Result<RepoCache> {
    let cwd = std::env::current_dir().map_err(|e| {
        RnuError::ConfigError(format!("Could not determine the current directory: {e}"))
    })?;

    match prepare_repo_in(&cache_root(&cwd), project, fresh) {
        Ok(cache) => Ok(cache),
        Err(err) => {
            // A read-only working directory should not fail the upload; the
            // cache is an optimisation, so fall back to a throwaway repo.
            warn!("{err} — using a temporary repository instead (nothing will be cached)");
            let temp = tempfile::Builder::new().prefix("rnu-m2-").tempdir()?;
            Ok(RepoCache {
                dir: temp.path().to_path_buf(),
                reused: false,
                _temp: Some(temp),
            })
        }
    }
}

fn cache_root(cwd: &Path) -> PathBuf {
    cwd.join(CACHE_DIR_NAME).join("m2")
}

fn prepare_repo_in(root: &Path, project: &Path, fresh: bool) -> Result<RepoCache> {
    let key = project_key(project);
    let dir = root.join(&key);
    let stamp = root.join(format!("{key}.pom"));

    // A changed pom.xml means the old closure may no longer be what this
    // project needs (bumped versions, dropped dependencies), and anything left
    // behind would be uploaded as if it were still in use.
    let digest = pom_digest(project)?;
    let matches_stamp = fs_read(&stamp).as_deref() == Some(digest.as_str());

    if !fresh && dir.is_dir() && matches_stamp {
        return Ok(RepoCache {
            dir,
            reused: true,
            _temp: None,
        });
    }

    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| {
            RnuError::ConfigError(format!(
                "Could not clear the cached repository at {}: {e}. \
                 Delete it manually or pass --keep-repo <dir>.",
                dir.display()
            ))
        })?;
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(root)?;
    std::fs::write(&stamp, &digest)?;

    Ok(RepoCache {
        dir,
        reused: false,
        _temp: None,
    })
}

fn fs_read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Stable per-project directory name. Deterministic across runs (unlike
/// `RandomState`), so the cache actually gets hit.
fn project_key(project: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let canonical = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Fingerprint of the project's pom.xml, used to invalidate the cache.
fn pom_digest(project: &Path) -> Result<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let (work_dir, pom) = split_project(project)?;
    let pom = pom.unwrap_or_else(|| work_dir.join("pom.xml"));
    let bytes = std::fs::read(&pom)
        .map_err(|e| RnuError::ConfigError(format!("Could not read {}: {e}", pom.display())))?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
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

    #[test]
    fn cache_is_reused_until_the_pom_changes() {
        let project = tempdir().unwrap();
        let pom = project.path().join("pom.xml");
        fs::write(&pom, "<project><artifactId>a</artifactId></project>").unwrap();
        let root = tempdir().unwrap();

        let first = prepare_repo_in(root.path(), project.path(), false).unwrap();
        assert!(!first.reused);

        // A leftover artifact proves the second call kept the directory.
        let leftover = first.dir.join("com").join("acme").join("demo").join("1.0");
        fs::create_dir_all(&leftover).unwrap();
        fs::write(leftover.join("demo-1.0.pom"), "pom").unwrap();

        let second = prepare_repo_in(root.path(), project.path(), false).unwrap();
        assert!(second.reused, "second run should reuse the cache");
        assert_eq!(second.dir, first.dir);
        assert!(leftover.join("demo-1.0.pom").exists());

        // Editing the pom must throw the old closure away.
        fs::write(&pom, "<project><artifactId>b</artifactId></project>").unwrap();
        let third = prepare_repo_in(root.path(), project.path(), false).unwrap();
        assert!(!third.reused, "changed pom should invalidate the cache");
        assert!(!leftover.join("demo-1.0.pom").exists());
    }

    #[test]
    fn fresh_flag_ignores_a_valid_cache() {
        let project = tempdir().unwrap();
        fs::write(project.path().join("pom.xml"), "<project/>").unwrap();
        let root = tempdir().unwrap();

        prepare_repo_in(root.path(), project.path(), false).unwrap();
        let forced = prepare_repo_in(root.path(), project.path(), true).unwrap();
        assert!(!forced.reused);
    }

    #[test]
    fn same_project_always_maps_to_the_same_key() {
        let project = tempdir().unwrap();
        assert_eq!(project_key(project.path()), project_key(project.path()));
    }

    #[test]
    fn cache_root_lives_under_the_working_directory() {
        let cwd = tempdir().unwrap();
        assert_eq!(cache_root(cwd.path()), cwd.path().join(".rnu").join("m2"));
    }

    #[test]
    fn spinner_text_drops_the_maven_prefix() {
        assert_eq!(
            summarise("[INFO] Downloading from central: x"),
            "Downloading from central: x"
        );
        assert_eq!(
            summarise("[WARNING]  "),
            "resolving dependencies with Maven…"
        );
    }
}
