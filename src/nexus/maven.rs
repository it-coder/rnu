use crate::error::{Result, RnuError};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

// ── types ──────────────────────────────────────────────────────────

/// A single file belonging to a Maven artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenFile {
    pub path: PathBuf,
    /// `jar` / `war` / `aar` / `zip` / `pom`
    pub extension: String,
    /// `sources` / `javadoc` / `jdk8` … ; `None` for the main artifact.
    pub classifier: Option<String>,
}

/// A Maven artifact ready for upload.
#[derive(Debug, Clone)]
pub struct MavenArtifact {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    /// Files belonging to this artifact, `pom` first.
    pub files: Vec<MavenFile>,
}

impl MavenArtifact {
    pub fn coordinate(&self) -> String {
        format!("{}:{}:{}", self.group_id, self.artifact_id, self.version)
    }

    fn push_file(&mut self, file: MavenFile) {
        if self.files.iter().any(|f| f.path == file.path) {
            return;
        }
        self.files.push(file);
    }
}

/// Extensions worth uploading. Everything else in a repository directory
/// (`_remote.repositories`, `*.sha1`, `*.lastUpdated`, `maven-metadata-*.xml`,
/// …) is bookkeeping noise and gets skipped.
const ARTIFACT_EXTENSIONS: [&str; 5] = ["jar", "war", "aar", "zip", "pom"];

// ── layout detection ───────────────────────────────────────────────

/// How a `maven <PATH>` argument should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MavenLayout {
    /// A project directory (has `pom.xml`): resolve its dependencies with mvn.
    Project,
    /// A Maven repository layout: `<group>/<artifact>/<version>/<file>`.
    Repo,
    /// A directory of loose `.pom` / `.jar` files.
    Loose,
}

impl MavenLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            MavenLayout::Project => "project",
            MavenLayout::Repo => "repository layout",
            MavenLayout::Loose => "loose POM files",
        }
    }

    /// Detect the layout of `path`.
    ///
    /// Detection order matters: a project directory also contains `.pom` files
    /// (its own `pom.xml`), so `pom.xml` wins before the repository heuristic
    /// runs.
    pub fn detect(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(RnuError::PackageNotFound(format!(
                "Path not found: {}",
                path.display()
            )));
        }

        if path.is_file() {
            return if path.file_name().and_then(|n| n.to_str()) == Some("pom.xml") {
                Ok(MavenLayout::Project)
            } else {
                Err(RnuError::InvalidPackage(format!(
                    "Expected a project directory, a pom.xml, or a Maven repository \
                     directory, but got the file {}",
                    path.display()
                )))
            };
        }

        if path.join("pom.xml").is_file() {
            return Ok(MavenLayout::Project);
        }

        let mut stats = LayoutStats::default();
        scan_layout(path, path, &mut stats)?;

        // One stray hit in an otherwise flat directory is not a repository.
        if stats.repo_hits > 0 && stats.repo_hits * 2 >= stats.candidates {
            Ok(MavenLayout::Repo)
        } else {
            Ok(MavenLayout::Loose)
        }
    }

    /// Directory that actually holds the artifacts: the parent for a `pom.xml`
    /// path, the path itself otherwise.
    pub fn base_dir(path: &Path) -> PathBuf {
        if path.is_file() {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        } else {
            path.to_path_buf()
        }
    }
}

#[derive(Default)]
struct LayoutStats {
    candidates: usize,
    repo_hits: usize,
}

fn scan_layout(root: &Path, dir: &Path, stats: &mut LayoutStats) -> Result<()> {
    // Enough evidence to stop walking; keeps detection fast on large repos.
    if stats.repo_hits >= 32 && stats.repo_hits * 2 >= stats.candidates {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_layout(root, &path, stats)?;
        } else if is_artifact_file(&path) {
            stats.candidates += 1;
            if classify_in_repo(root, &path).is_some() {
                stats.repo_hits += 1;
            }
        }
    }
    Ok(())
}

fn is_artifact_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ARTIFACT_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

// ── filename parsing ───────────────────────────────────────────────

/// Split `{artifact}-{version}[-{classifier}].{ext}` into extension and
/// classifier.
///
/// The version boundary is enforced: for `foo` `1.0`, `foo-1.0.1.jar` is
/// rejected instead of being mistaken for a file of `foo:1.0`.
fn split_artifact_filename(
    filename: &str,
    artifact_id: &str,
    version: &str,
) -> Option<(String, Option<String>)> {
    let prefix = format!("{}-{}", artifact_id, version);
    let rest = filename.strip_prefix(&prefix)?;

    let (classifier, ext) = if let Some(after_dot) = rest.strip_prefix('.') {
        (None, after_dot)
    } else if let Some(after_dash) = rest.strip_prefix('-') {
        let (classifier, ext) = after_dash.rsplit_once('.')?;
        if classifier.is_empty() {
            return None;
        }
        (Some(classifier.to_string()), ext)
    } else {
        // e.g. `foo-1.0.1.jar` against version `1.0`
        return None;
    };

    if !ARTIFACT_EXTENSIONS.contains(&ext) {
        return None;
    }
    Some((ext.to_string(), classifier))
}

// ── scanner: loose POM files ───────────────────────────────────────

/// Scans a directory tree of loose `.pom` files (the original input mode).
pub struct MavenScanner;

impl MavenScanner {
    /// Walk `root` recursively, find every `.pom` file, parse its
    /// coordinates, and collect the companion files sitting next to it.
    pub fn scan(root: &Path) -> Result<Vec<MavenArtifact>> {
        let mut poms: Vec<PathBuf> = Vec::new();
        collect_poms(root, &mut poms)?;

        if poms.is_empty() {
            return Err(RnuError::PackageNotFound(format!(
                "No .pom files found under {}",
                root.display()
            )));
        }

        let mut map: BTreeMap<String, MavenArtifact> = BTreeMap::new();

        for pom_path in &poms {
            let pom = match parse_pom(pom_path) {
                Ok(p) => p,
                Err(e) => {
                    debug!("Skipping {}: {}", pom_path.display(), e);
                    continue;
                }
            };

            let entry = map
                .entry(format!(
                    "{}:{}:{}",
                    pom.group_id, pom.artifact_id, pom.version
                ))
                .or_insert_with(|| MavenArtifact {
                    group_id: pom.group_id.clone(),
                    artifact_id: pom.artifact_id.clone(),
                    version: pom.version.clone(),
                    files: Vec::new(),
                });

            entry.push_file(MavenFile {
                path: pom_path.clone(),
                extension: "pom".to_string(),
                classifier: None,
            });

            let parent = pom_path.parent().unwrap_or(Path::new("."));
            let Ok(dir_entries) = fs::read_dir(parent) else {
                continue;
            };
            for de in dir_entries.filter_map(|e| e.ok()) {
                let path = de.path();
                if !path.is_file() {
                    continue;
                }
                let name = de.file_name();
                let name_str = name.to_string_lossy();
                if let Some((extension, classifier)) =
                    split_artifact_filename(&name_str, &pom.artifact_id, &pom.version)
                {
                    entry.push_file(MavenFile {
                        path,
                        extension,
                        classifier,
                    });
                }
            }
        }

        let artifacts = finish(map);
        info!(
            "Found {} Maven artifact(s) ({} .pom files scanned)",
            artifacts.len(),
            poms.len()
        );
        Ok(artifacts)
    }
}

// ── scanner: Maven repository layout ───────────────────────────────

/// Scans a Maven **repository layout** directory: `~/.m2/repository`, a
/// directory produced by `dependency:copy-dependencies
/// -Dmdep.useRepositoryLayout=true`, or the isolated repo that
/// `mvn -Dmaven.repo.local=…` populated.
///
/// Coordinates come from the path (`group/artifact/version/file`), so there is
/// no XML parsing involved and classifiers / version boundaries are exact.
pub struct MavenRepoScanner;

impl MavenRepoScanner {
    pub fn scan(root: &Path) -> Result<Vec<MavenArtifact>> {
        if !root.is_dir() {
            return Err(RnuError::PackageNotFound(format!(
                "Maven repository directory not found: {}",
                root.display()
            )));
        }

        let mut files: Vec<PathBuf> = Vec::new();
        collect_files(root, &mut files)?;

        let mut map: BTreeMap<String, MavenArtifact> = BTreeMap::new();
        let mut skipped = 0usize;

        for path in files {
            match classify_in_repo(root, &path) {
                Some((group_id, artifact_id, version, file)) => {
                    map.entry(format!("{}:{}:{}", group_id, artifact_id, version))
                        .or_insert_with(|| MavenArtifact {
                            group_id,
                            artifact_id,
                            version,
                            files: Vec::new(),
                        })
                        .push_file(file);
                }
                None => skipped += 1,
            }
        }

        let artifacts = finish(map);
        info!(
            "Scanned repository {}: {} artifact(s), {} non-artifact file(s) ignored",
            root.display(),
            artifacts.len(),
            skipped
        );
        Ok(artifacts)
    }
}

/// Derive coordinates from the repository path, then parse the filename.
fn classify_in_repo(root: &Path, path: &Path) -> Option<(String, String, String, MavenFile)> {
    let filename = path.file_name()?.to_str()?;
    let version_dir = path.parent()?;
    let artifact_dir = version_dir.parent()?;
    let group_dir = artifact_dir.parent()?;

    let artifact_id = artifact_dir.file_name()?.to_str()?.to_string();
    let version = version_dir.file_name()?.to_str()?.to_string();

    let group_id = group_dir
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?
        .join(".");
    if group_id.is_empty() {
        return None;
    }

    let (extension, classifier) = split_artifact_filename(filename, &artifact_id, &version)?;

    Some((
        group_id,
        artifact_id,
        version,
        MavenFile {
            path: path.to_path_buf(),
            extension,
            classifier,
        },
    ))
}

// ── helpers ────────────────────────────────────────────────────────

/// Sort files (`pom` first) and artifacts by coordinate.
fn finish(mut map: BTreeMap<String, MavenArtifact>) -> Vec<MavenArtifact> {
    for artifact in map.values_mut() {
        artifact.files.sort_by(|a, b| {
            let rank = |f: &MavenFile| match (&f.extension[..], &f.classifier) {
                ("pom", _) => 0,
                (_, None) => 1,
                (_, Some(c)) if c == "sources" => 2,
                (_, Some(c)) if c == "javadoc" => 3,
                _ => 4,
            };
            rank(a).cmp(&rank(b)).then_with(|| a.path.cmp(&b.path))
        });
    }
    map.into_values().collect()
}

fn collect_poms(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_poms(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("pom") {
            out.push(path);
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

struct PomCoords {
    group_id: String,
    artifact_id: String,
    version: String,
}

fn parse_pom(path: &Path) -> Result<PomCoords> {
    let xml = fs::read_to_string(path)?;

    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut group_id = String::new();
    let mut artifact_id = String::new();
    let mut version = String::new();
    let mut current_tag = String::new();

    // Track parent coordinates for inheritance
    let mut parent_group: Option<String> = None;
    let mut parent_version: Option<String> = None;
    let mut in_parent = false;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = e.name();
                let local_name = name.local_name();
                let local = String::from_utf8_lossy(local_name.as_ref()).to_string();
                current_tag = local.clone();
                if local == "parent" {
                    in_parent = true;
                }
            }
            Event::End(e) => {
                let name = e.name();
                let local_name = name.local_name();
                let local = String::from_utf8_lossy(local_name.as_ref());
                if local == "parent" {
                    in_parent = false;
                }
                current_tag.clear();
            }
            Event::Text(e) => {
                let text = e.unescape()?.into_owned();
                match current_tag.as_str() {
                    "groupId" if in_parent => parent_group = Some(text),
                    "version" if in_parent => parent_version = Some(text),
                    "groupId" => group_id = text,
                    "artifactId" => artifact_id = text,
                    "version" => version = text,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if artifact_id.is_empty() {
        return Err(RnuError::InvalidPackage(format!(
            "Missing <artifactId> in {}",
            path.display()
        )));
    }

    // Inherit from parent if not specified
    if group_id.is_empty() {
        group_id = parent_group.ok_or_else(|| {
            RnuError::InvalidPackage(format!(
                "Missing <groupId> (and no parent) in {}",
                path.display()
            ))
        })?;
    }
    if version.is_empty() {
        version = parent_version.ok_or_else(|| {
            RnuError::InvalidPackage(format!(
                "Missing <version> (and no parent) in {}",
                path.display()
            ))
        })?;
    }

    Ok(PomCoords {
        group_id,
        artifact_id,
        version,
    })
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn filename_classifier_and_boundary() {
        assert_eq!(
            split_artifact_filename("foo-1.0.jar", "foo", "1.0"),
            Some(("jar".into(), None))
        );
        assert_eq!(
            split_artifact_filename("foo-1.0-sources.jar", "foo", "1.0"),
            Some(("jar".into(), Some("sources".into())))
        );
        assert_eq!(
            split_artifact_filename("foo-1.0-jdk8-javadoc.jar", "foo", "1.0"),
            Some(("jar".into(), Some("jdk8-javadoc".into())))
        );
        // version boundary
        assert_eq!(split_artifact_filename("foo-1.0.1.jar", "foo", "1.0"), None);
        // bookkeeping noise
        assert_eq!(
            split_artifact_filename("_remote.repositories", "foo", "1.0"),
            None
        );
        assert_eq!(
            split_artifact_filename("foo-1.0.jar.sha1", "foo", "1.0"),
            None
        );
        assert_eq!(
            split_artifact_filename("foo-1.0.jar.lastUpdated", "foo", "1.0"),
            None
        );
        assert_eq!(
            split_artifact_filename("maven-metadata-local.xml", "foo", "1.0"),
            None
        );
    }

    #[test]
    fn repo_layout_yields_exact_coordinates() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        touch(&root.join("org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.jar"));
        touch(&root.join("org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.pom"));
        touch(&root.join("org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9-sources.jar"));
        // noise
        touch(&root.join("org/slf4j/slf4j-api/2.0.9/_remote.repositories"));
        touch(&root.join("org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.jar.sha1"));
        touch(&root.join("org/slf4j/slf4j-api/maven-metadata-local.xml"));

        let artifacts = MavenRepoScanner::scan(root).unwrap();
        assert_eq!(artifacts.len(), 1);
        let a = &artifacts[0];
        assert_eq!(a.coordinate(), "org.slf4j:slf4j-api:2.0.9");
        assert_eq!(a.files.len(), 3);
        assert_eq!(a.files[0].extension, "pom");
        assert_eq!(a.files[1].classifier, None);
        assert_eq!(a.files[2].classifier.as_deref(), Some("sources"));
    }

    #[test]
    fn repo_layout_keeps_sibling_versions_apart() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("com/example/foo/1.0/foo-1.0.jar"));
        touch(&root.join("com/example/foo/1.0.1/foo-1.0.1.jar"));

        let artifacts = MavenRepoScanner::scan(root).unwrap();
        let versions: Vec<&str> = artifacts.iter().map(|a| a.version.as_str()).collect();
        assert_eq!(versions, vec!["1.0", "1.0.1"]);
        assert_eq!(artifacts[0].files.len(), 1);
    }

    #[test]
    fn repo_layout_rejects_non_artifact_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("com/example/foo/1.0/readme.txt"));
        touch(&root.join("com/example/foo/1.0/foo-1.0.jar"));

        let artifacts = MavenRepoScanner::scan(root).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].files.len(), 1);
    }

    #[test]
    fn loose_pom_scanner_keeps_classifier() {
        let dir = tempdir().unwrap();
        let d = dir.path();
        fs::write(
            d.join("foo-1.2.3.pom"),
            "<project><groupId>com.example</groupId><artifactId>foo</artifactId><version>1.2.3</version></project>",
        )
        .unwrap();
        fs::write(d.join("foo-1.2.3.jar"), b"x").unwrap();
        fs::write(d.join("foo-1.2.3-sources.jar"), b"x").unwrap();
        fs::write(d.join("foo-1.2.3-sources.jar.sha1"), b"x").unwrap();
        fs::write(d.join("foo-1.2.3.1.jar"), b"x").unwrap();

        let artifacts = MavenScanner::scan(d).unwrap();
        assert_eq!(artifacts.len(), 1);
        let classifiers: Vec<_> = artifacts[0]
            .files
            .iter()
            .map(|f| f.classifier.clone())
            .collect();
        assert_eq!(
            classifiers,
            vec![None, None, Some("sources".to_string())],
            "pom + main jar + sources jar，不含 1.2.3.1 与 .sha1"
        );
    }

    #[test]
    fn detect_project_by_pom_xml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("pom.xml"), "<project/>").unwrap();
        // a project also has loose artifacts under target/
        touch(&root.join("target/app-1.0.jar"));

        assert_eq!(MavenLayout::detect(root).unwrap(), MavenLayout::Project);
        assert_eq!(
            MavenLayout::detect(&root.join("pom.xml")).unwrap(),
            MavenLayout::Project,
            "指向 pom.xml 本身也应识别为项目"
        );
    }

    #[test]
    fn detect_repository_layout() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("com/example/foo/1.0/foo-1.0.jar"));
        touch(&root.join("com/example/foo/1.0/foo-1.0.pom"));
        touch(&root.join("org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.jar"));

        assert_eq!(MavenLayout::detect(root).unwrap(), MavenLayout::Repo);
    }

    #[test]
    fn detect_loose_directory() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("app-1.0.pom"));
        touch(&root.join("app-1.0.jar"));

        assert_eq!(MavenLayout::detect(root).unwrap(), MavenLayout::Loose);
    }

    #[test]
    fn detect_rejects_missing_path() {
        let dir = tempdir().unwrap();
        assert!(MavenLayout::detect(&dir.path().join("nope")).is_err());
    }

    #[test]
    fn base_dir_unwraps_pom_xml() {
        let dir = tempdir().unwrap();
        let pom = dir.path().join("pom.xml");
        fs::write(&pom, "<project/>").unwrap();
        assert_eq!(MavenLayout::base_dir(&pom), dir.path());
        assert_eq!(MavenLayout::base_dir(dir.path()), dir.path());
    }
}
