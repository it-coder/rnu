use crate::error::{Result, RnuError};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

// ── types ──────────────────────────────────────────────────────────

/// A Maven artifact ready for upload.
#[derive(Debug, Clone)]
pub struct MavenArtifact {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    /// "jar", "war", "pom"
    #[allow(dead_code)]
    pub packaging: String,
    /// All files belonging to this artifact: (path, extension)
    pub files: Vec<(PathBuf, String)>,
}

// ── scanner ────────────────────────────────────────────────────────

/// Stateless scanner that discovers Maven artifacts in a directory tree.
pub struct MavenScanner;

impl MavenScanner {
    /// Walk `root` recursively, find every `.pom` file, parse its
    /// coordinates, and collect the associated `.jar` / `.war` files.
    pub fn scan(root: &Path) -> Result<Vec<MavenArtifact>> {
        let mut poms: Vec<PathBuf> = Vec::new();
        collect_poms(root, &mut poms)?;

        if poms.is_empty() {
            return Err(RnuError::PackageNotFound(format!(
                "No .pom files found under {}",
                root.display()
            )));
        }

        let mut map: HashMap<String, MavenArtifact> = HashMap::new();

        for pom_path in &poms {
            let pom = match parse_pom(pom_path) {
                Ok(p) => p,
                Err(e) => {
                    debug!("Skipping {}: {}", pom_path.display(), e);
                    continue;
                }
            };

            let key = format!("{}:{}:{}", pom.group_id, pom.artifact_id, pom.version);
            let entry = map.entry(key).or_insert_with(|| MavenArtifact {
                group_id: pom.group_id.clone(),
                artifact_id: pom.artifact_id.clone(),
                version: pom.version.clone(),
                packaging: pom.packaging.clone(),
                files: Vec::new(),
            });

            // Register the POM file itself
            entry.files.push((pom_path.clone(), "pom".to_string()));

            // Look for companion files in the same directory
            let parent = pom_path.parent().unwrap_or(Path::new("."));
            if let Ok(dir_entries) = fs::read_dir(parent) {
                let prefix = format!("{}-{}", pom.artifact_id, pom.version);
                for de in dir_entries.filter_map(|e| e.ok()) {
                    let name = de.file_name();
                    let name_str = name.to_string_lossy();
                    if !name_str.starts_with(&prefix) {
                        continue;
                    }
                    let path = de.path();
                    if !path.is_file() {
                        continue;
                    }
                    if let Some(ext) = classify_extension(&name_str) {
                        if ext != "pom" {
                            entry.files.push((path, ext.to_string()));
                        }
                    }
                }
            }
        }

        let mut artifacts: Vec<MavenArtifact> = map.into_values().collect();
        artifacts.sort_by(|a, b| {
            a.artifact_id
                .cmp(&b.artifact_id)
                .then_with(|| a.version.cmp(&b.version))
        });

        info!(
            "Found {} Maven artifact(s) ({} .pom files scanned)",
            artifacts.len(),
            poms.len()
        );
        Ok(artifacts)
    }
}

// ── helpers ────────────────────────────────────────────────────────

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

struct PomCoords {
    group_id: String,
    artifact_id: String,
    version: String,
    packaging: String,
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
    let mut packaging = String::from("jar"); // Maven default
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
                    "packaging" => packaging = text,
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
        packaging,
    })
}

fn classify_extension(filename: &str) -> Option<&str> {
    if filename.ends_with("-sources.jar") {
        Some("jar") // treat source jars as regular jars
    } else if filename.ends_with("-javadoc.jar") {
        Some("jar")
    } else if filename.ends_with(".jar") {
        Some("jar")
    } else if filename.ends_with(".war") {
        Some("war")
    } else if filename.ends_with(".pom") {
        Some("pom")
    } else if filename.ends_with(".aar") {
        Some("aar")
    } else {
        None
    }
}
