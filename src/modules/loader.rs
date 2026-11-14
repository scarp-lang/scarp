//! Source identity and bounded reads; no lexer/parser filesystem access.
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLoadKind {
    Io,
    InvalidUtf8,
    InvalidPath,
    Limit,
}

#[derive(Debug, Clone)]
pub struct SourceLoadError {
    pub kind: SourceLoadKind,
    pub message: String,
}
impl SourceLoadError {
    pub fn new(kind: SourceLoadKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for SourceLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SourceLoadError {}

/// Source text and the optional editor revision of those bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSource {
    pub text: String,
    pub revision: Option<u64>,
}

/// Implementations provide stable identities and honor the requested byte cap.
/// The compiler also checks returned source lengths. No source is executed here.
pub trait SourceLoader {
    fn resolve(&self, importer: Option<&str>, specifier: &str) -> Result<String, SourceLoadError>;
    fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError>;

    /// Read a source and its revision together. Editor hosts should keep the
    /// workspace view stable for the duration of graph discovery.
    fn load_with_revision(
        &self,
        identity: &str,
        max_bytes: usize,
    ) -> Result<LoadedSource, SourceLoadError> {
        self.load(identity, max_bytes).map(|text| LoadedSource {
            text,
            revision: None,
        })
    }

    fn display_name(&self, identity: &str) -> String {
        identity.to_string()
    }
    /// Optional qualified-name prefix for diagnostics. Canonical identities
    /// still resolve imports and set a lower bound on expansion charges.
    /// Distinct modules must use distinct qualified-name prefixes.
    fn diagnostic_qualifier(&self, _identity: &str) -> Option<String> {
        None
    }
}

fn path_error(message: impl Into<String>) -> SourceLoadError {
    SourceLoadError::new(SourceLoadKind::InvalidPath, message)
}

pub(super) fn validate_path(path: &str, import: bool) -> Result<(), SourceLoadError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.split('/').any(str::is_empty)
        || (import && !path.ends_with(".srp"))
    {
        return Err(path_error(
            "use a relative .srp path with forward slashes and no drive, URI or control characters",
        ));
    }
    Ok(())
}

pub(super) fn relative_path(
    base: &str,
    path: &str,
    import: bool,
) -> Result<String, SourceLoadError> {
    validate_path(path, import)?;
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(path_error("module path escapes the trusted source root"));
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Err(path_error("module path must identify a source file"));
    }
    Ok(parts.join("/"))
}

/// Portable virtual paths, relative to an in-memory trusted root.
#[derive(Debug, Clone, Default)]
pub struct MemorySourceLoader {
    files: BTreeMap<String, LoadedSource>,
}
impl MemorySourceLoader {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(
        &mut self,
        name: impl AsRef<str>,
        source: impl Into<String>,
    ) -> Result<(), SourceLoadError> {
        let name = relative_path("", name.as_ref(), false)?;
        if self.files.contains_key(&name) {
            return Err(path_error(format!("source {name:?} is already registered")));
        }
        self.files.insert(
            name,
            LoadedSource {
                text: source.into(),
                revision: None,
            },
        );
        Ok(())
    }

    /// Insert or replace an in-memory document. Use a fresh editor revision
    /// when changing the document, including after deleting and reopening it.
    pub fn set_document(
        &mut self,
        name: impl AsRef<str>,
        source: impl Into<String>,
        revision: u64,
    ) -> Result<(), SourceLoadError> {
        let name = relative_path("", name.as_ref(), false)?;
        self.files.insert(
            name,
            LoadedSource {
                text: source.into(),
                revision: Some(revision),
            },
        );
        Ok(())
    }

    /// Remove an in-memory document by its normalized path.
    pub fn remove_document(&mut self, name: impl AsRef<str>) -> Result<bool, SourceLoadError> {
        let name = relative_path("", name.as_ref(), false)?;
        Ok(self.files.remove(&name).is_some())
    }

    fn document(&self, identity: &str, max_bytes: usize) -> Result<&LoadedSource, SourceLoadError> {
        let document = self.files.get(identity).ok_or_else(|| {
            SourceLoadError::new(SourceLoadKind::Io, format!("could not read {identity:?}"))
        })?;
        if document.text.len() > max_bytes {
            return Err(SourceLoadError::new(
                SourceLoadKind::Limit,
                "module source byte limit exceeded",
            ));
        }
        Ok(document)
    }
}
impl SourceLoader for MemorySourceLoader {
    fn resolve(&self, importer: Option<&str>, specifier: &str) -> Result<String, SourceLoadError> {
        let base = importer
            .and_then(|name| name.rsplit_once('/').map(|p| p.0))
            .unwrap_or("");
        let name = relative_path(base, specifier, importer.is_some())?;
        if !self.files.contains_key(&name) {
            return Err(SourceLoadError::new(
                SourceLoadKind::Io,
                format!("could not read {name:?}: source is not registered"),
            ));
        }
        Ok(name)
    }
    fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
        Ok(self.document(identity, max_bytes)?.text.clone())
    }

    fn load_with_revision(
        &self,
        identity: &str,
        max_bytes: usize,
    ) -> Result<LoadedSource, SourceLoadError> {
        self.document(identity, max_bytes).cloned()
    }
}

/// Canonical local source files restricted to an explicitly supplied directory.
#[derive(Debug, Clone)]
pub struct FileSourceLoader {
    root: PathBuf,
    native_paths: RefCell<BTreeMap<String, PathBuf>>,
    relative_diagnostics: bool,
}
impl FileSourceLoader {
    pub fn new(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source root is not a directory",
            ));
        }
        Ok(Self {
            root,
            native_paths: RefCell::new(BTreeMap::new()),
            relative_diagnostics: false,
        })
    }
    /// Use root-relative names for sources and qualified functions. Canonical
    /// resolution and containment checks still use native paths.
    pub fn with_relative_diagnostics(mut self) -> Self {
        self.relative_diagnostics = true;
        self
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub(super) fn identify(&self, path: &Path) -> Result<String, SourceLoadError> {
        let path = self.checked(path)?;
        if let Some(name) = path.to_str() {
            return Ok(name.to_string());
        }
        // Preserve native non-Unicode entry paths without lossy identity keys.
        // Debug formatting escapes their native bytes/code units; actual paths
        // remain owned by this loader instance, never a mutable global cache.
        let identity = format!("@native:{path:?}");
        self.native_paths
            .borrow_mut()
            .insert(identity.clone(), path);
        Ok(identity)
    }
    fn path(&self, identity: &str) -> PathBuf {
        self.native_paths
            .borrow()
            .get(identity)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(identity))
    }
    fn diagnostic_path<'a>(&self, path: &'a Path) -> Cow<'a, str> {
        if !self.relative_diagnostics {
            return path.to_string_lossy();
        }
        path.strip_prefix(&self.root)
            .ok()
            .and_then(portable_name)
            .unwrap_or_default()
    }

    fn read_error(
        &self,
        path: &Path,
        kind: SourceLoadKind,
        error: impl std::fmt::Display,
    ) -> SourceLoadError {
        let name = self.diagnostic_path(path);
        let message = if name.is_empty() {
            format!("could not read source file: {error}")
        } else {
            format!("could not read '{name}': {error}")
        };
        SourceLoadError::new(kind, message)
    }

    fn checked(&self, path: &Path) -> Result<PathBuf, SourceLoadError> {
        let path = fs::canonicalize(path)
            .map_err(|error| self.read_error(path, SourceLoadKind::Io, error))?;
        if !path.starts_with(&self.root) {
            return Err(path_error(
                "canonical module path escapes the trusted source root",
            ));
        }
        if !path.is_file() {
            return Err(self.read_error(&path, SourceLoadKind::Io, "not a regular file"));
        }
        Ok(path)
    }
}
impl SourceLoader for FileSourceLoader {
    fn resolve(&self, importer: Option<&str>, specifier: &str) -> Result<String, SourceLoadError> {
        let base = if let Some(importer) = importer {
            let importing_path = self.path(importer);
            let parent = importing_path
                .parent()
                .ok_or_else(|| path_error("importing source has no parent"))?;
            parent
                .strip_prefix(&self.root)
                .map_err(|_| path_error("importer is outside the trusted source root"))?
                .to_str()
                .ok_or_else(|| path_error("source paths must be valid Unicode"))?
                .replace('\\', "/")
        } else {
            String::new()
        };
        let relative = relative_path(&base, specifier, importer.is_some())?;
        self.identify(&self.root.join(relative))
    }
    fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
        // Recheck canonical containment immediately before opening. This is a
        // trusted-tree loader, not a sandbox against concurrent hostile changes.
        let path = self.checked(&self.path(identity))?;
        let file = fs::File::open(&path)
            .map_err(|error| self.read_error(&path, SourceLoadKind::Io, error))?;
        let amount = file
            .metadata()
            .map_err(|error| SourceLoadError::new(SourceLoadKind::Io, error.to_string()))?
            .len();
        if amount > max_bytes as u64 {
            return Err(SourceLoadError::new(
                SourceLoadKind::Limit,
                "module source byte limit exceeded",
            ));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(amount as usize).map_err(|_| {
            SourceLoadError::new(SourceLoadKind::Limit, "cannot reserve source buffer")
        })?;
        file.take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| self.read_error(&path, SourceLoadKind::Io, error))?;
        if bytes.len() > max_bytes {
            return Err(SourceLoadError::new(
                SourceLoadKind::Limit,
                "module source grew beyond its byte limit",
            ));
        }
        String::from_utf8(bytes).map_err(|_| {
            self.read_error(
                &path,
                SourceLoadKind::InvalidUtf8,
                "source is not valid UTF-8",
            )
        })
    }
    fn display_name(&self, identity: &str) -> String {
        self.diagnostic_path(&self.path(identity)).into_owned()
    }

    fn diagnostic_qualifier(&self, identity: &str) -> Option<String> {
        if !self.relative_diagnostics {
            return None;
        }
        let path = self.path(identity);
        let relative = path.strip_prefix(&self.root).ok()?;
        Some(portable_name(relative).map_or_else(
            // The NUL distinguishes escaped native names from ordinary paths.
            || format!("\0native:{relative:?}"),
            Cow::into_owned,
        ))
    }
}

fn portable_name(path: &Path) -> Option<Cow<'_, str>> {
    let name = path.to_str()?;
    Some(if cfg!(windows) {
        Cow::Owned(name.replace('\\', "/"))
    } else {
        Cow::Borrowed(name)
    })
}
