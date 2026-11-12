//! Minimal, offline, explicit Scarp project manifest and scaffolding.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const MANIFEST: &str = "Scarp.project";
pub const MANIFEST_V2: &str = "scarp.project";
const MAX_MANIFEST_BYTES: u64 = 4096;
const STARTER: &[u8] = b"print(\"Hello from Scarp!\");\n";

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.trim() == value
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, ':' | '/' | '\\'))
}

fn valid_entry(value: &str) -> bool {
    value.len() <= 512
        && value.ends_with(".srp")
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | ':'))
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && !Path::new(value).is_absolute()
}

/// Create only a previously nonexistent directory; never adopt/overwrite
/// an existing directory, even an empty one.
pub fn create(directory: &Path) -> Result<(), String> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("project directory must have a UTF-8 name")?;
    if !valid_name(name) {
        return Err(
            "project name must be 1..96 UTF-8 bytes without control characters or path separators"
                .into(),
        );
    }

    fs::create_dir(directory).map_err(|error| {
        format!(
            "could not create project directory '{}': {error}",
            directory.display()
        )
    })?;
    let src = directory.join("src");
    let entry = src.join("main.srp");
    let manifest = directory.join(MANIFEST);
    let mut created_src = false;
    let mut created_entry = false;
    let mut created_manifest = false;

    let result = (|| -> Result<(), String> {
        fs::create_dir(&src)
            .map_err(|error| format!("could not create source directory: {error}"))?;
        created_src = true;

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&entry)
            .map_err(|error| format!("could not create Scarp source: {error}"))?;
        created_entry = true;
        file.write_all(STARTER)
            .map_err(|error| format!("could not write Scarp source: {error}"))?;

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest)
            .map_err(|error| format!("could not create project manifest: {error}"))?;
        created_manifest = true;
        file.write_all(format!("scarp-project: 1\nname: {name}\nentry: src/main.srp\n").as_bytes())
            .map_err(|error| format!("could not write project manifest: {error}"))?;
        Ok(())
    })();

    if result.is_err() {
        // Remove only files we created. rmdir is nonrecursive, so concurrent
        // foreign content is never recursively deleted during cleanup.
        if created_manifest {
            let _ = fs::remove_file(&manifest);
        }
        if created_entry {
            let _ = fs::remove_file(&entry);
        }
        if created_src {
            let _ = fs::remove_dir(&src);
        }
        let _ = fs::remove_dir(directory);
    }
    result
}

/// Initialize a new project using the declarative `scarp.project` format.
///
/// `create` remains the legacy v1 workflow and is intentionally unchanged.
/// Refuse existing directories; clean up only paths created by this call.
pub fn init(directory: &Path) -> Result<(), String> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("project directory must have a UTF-8 name")?;
    if !crate::project_manifest::valid_name(name) {
        return Err("project name must be 1..96 UTF-8 bytes without separators or controls".into());
    }
    fs::create_dir(directory).map_err(|error| {
        format!(
            "could not create project directory '{}': {error}",
            directory.display()
        )
    })?;
    let mut created_files = Vec::new();
    let mut created_directories = Vec::new();
    let result = (|| -> Result<(), String> {
        for sub in ["src", "tests", "examples"] {
            let path = directory.join(sub);
            fs::create_dir(&path)
                .map_err(|error| format!("could not create '{}': {error}", path.display()))?;
            created_directories.push(path);
        }
        let escaped_name = name.replace('"', "\\\"");
        let manifest = format!(
            "project \"{escaped_name}\" {{\n    version: \"0.1.0\"\n    entry: \"src/main.srp\"\n    type: \"application\"\n    dependencies {{}}\n}}\n"
        );
        let readme = format!(
            "# {name}\n\nA Scarp application.\n\nRun from the project directory:\n\n```sh\nscarp verify .\nscarp run .\n```\n\nThe project is defined by `scarp.project`. No dependencies are installed.\n"
        );
        let files: [(&str, &[u8]); 4] = [
            (MANIFEST_V2, manifest.as_bytes()),
            ("src/main.srp", STARTER),
            ("README.md", readme.as_bytes()),
            (".gitignore", b"/target/\n"),
        ];
        for (name, contents) in files {
            let path = directory.join(name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| format!("could not create '{}': {error}", path.display()))?;
            created_files.push(path.clone());
            file.write_all(contents)
                .map_err(|error| format!("could not write '{}': {error}", path.display()))?;
        }
        Ok(())
    })();
    if result.is_err() {
        for file in created_files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        for dir in created_directories.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        // Nonrecursive: never remove another process's foreign content.
        let _ = fs::remove_dir(directory);
    }
    result
}

/// A validated project snapshot. Relative labels come from a single bounded
/// manifest read; the canonical entrypoint must remain inside the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub name: String,
    pub version: Option<String>,
    pub entry: String,
    pub kind: String,
    pub format: &'static str,
    pub manifest_filename: &'static str,
    pub entry_path: PathBuf,
}

/// Read the same strictly bounded project manifest used by run/verify, with no
/// source execution, imports, network access or dependency installation.
///
/// Returns validated metadata only after checking the canonical entrypoint.
pub fn info(directory: &Path) -> Result<ProjectInfo, String> {
    let root = fs::canonicalize(directory)
        .map_err(|error| format!("could not open project '{}': {error}", directory.display()))?;
    if !root.is_dir() {
        return Err("project path must be a directory".into());
    }
    // Prefer the new filename; on case-insensitive filesystems a legacy
    // Scarp.project may resolve at both spellings, so the format is selected
    // from the file's actual content instead of filename case alone.
    let new_path = root.join(MANIFEST_V2);
    let manifest_path = if fs::symlink_metadata(&new_path).is_ok() {
        new_path
    } else {
        root.join(MANIFEST)
    };
    let link_meta = fs::symlink_metadata(&manifest_path)
        .map_err(|error| format!("could not open {MANIFEST}: {error}"))?;
    if link_meta.file_type().is_symlink() || !link_meta.file_type().is_file() {
        return Err("project manifest must be an ordinary file, not a symlink".into());
    }
    let file = File::open(&manifest_path)
        .map_err(|error| format!("could not read {MANIFEST}: {error}"))?;
    if file
        .metadata()
        .map_err(|error| format!("could not stat {MANIFEST}: {error}"))?
        .len()
        > MAX_MANIFEST_BYTES
    {
        return Err("project manifest exceeds 4096 bytes".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {MANIFEST}: {error}"))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("project manifest exceeds 4096 bytes".into());
    }
    let content = std::str::from_utf8(&bytes).map_err(|_| "project manifest must be UTF-8")?;
    let (name, version, entry, kind, format, manifest_filename) =
        if content.starts_with("scarp-project:") {
            // Legacy v1 remains accepted verbatim. Preserve its strict line
            // order and diagnostics for previously generated projects.
            let mut lines = content.lines();
            if lines.next() != Some("scarp-project: 1") {
                return Err("expected 'scarp-project: 1' as the first manifest line".into());
            }
            let name = lines
                .next()
                .and_then(|line| line.strip_prefix("name: "))
                .ok_or("expected 'name: NAME' as the second manifest line")?;
            if !valid_name(name) {
                return Err("invalid project name in manifest".into());
            }
            let entry = lines
                .next()
                .and_then(|line| line.strip_prefix("entry: "))
                .ok_or("expected 'entry: SOURCE.srp' as the third manifest line")?;
            if lines.next().is_some() {
                return Err("unknown or duplicate project manifest fields".into());
            }
            if !valid_entry(entry) {
                return Err("entry must be a contained, relative .srp path without traversal".into());
            }
            (
                name.to_owned(),
                None,
                entry.to_owned(),
                "application".to_owned(),
                "legacy-v1",
                MANIFEST,
            )
        } else {
            let manifest = crate::project_manifest::parse(content)?;
            (
                manifest.name,
                Some(manifest.version),
                manifest.entry,
                manifest.kind,
                "v2",
                MANIFEST_V2,
            )
        };
    let target = fs::canonicalize(root.join(&entry))
        .map_err(|error| format!("could not open project entry '{entry}': {error}"))?;
    if !target.starts_with(&root) {
        return Err("project entry resolves outside the project directory".into());
    }
    if !target.is_file() {
        return Err("project entry must be an existing .srp file".into());
    }
    Ok(ProjectInfo {
        name,
        version,
        entry,
        kind,
        format,
        manifest_filename,
        entry_path: target,
    })
}

/// Resolve a user-selected project root to one existing, contained .srp file.
/// No ancestor search, symlink escape, source execution, or registry access.
pub fn entry_file(directory: &Path) -> Result<PathBuf, String> {
    info(directory).map(|project| project.entry_path)
}
