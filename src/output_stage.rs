//! Same-filesystem staging for atomic create-if-absent output.

use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);
const STAGE_ATTEMPTS: usize = 64;
const OUTPUT: &str = "result";
const SCRATCH: &str = "scratch";

pub(crate) struct OutputStage {
    directory: PathBuf,
    destination: PathBuf,
}

impl OutputStage {
    pub(crate) fn create(destination: &Path, label: &str) -> io::Result<Self> {
        let destination_name = destination.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "expected an output filename")
        })?;
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        for _ in 0..STAGE_ATTEMPTS {
            let nonce = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let name = format!(".scarp-{label}-{}-{nonce}.tmp", std::process::id());
            // Account for ordinary case-insensitive and trailing-dot aliases.
            if destination_name
                .to_string_lossy()
                .trim_end_matches(['.', ' '])
                .eq_ignore_ascii_case(&name)
            {
                continue;
            }
            let directory = parent.join(name);
            match builder.create(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        directory,
                        destination: destination.to_path_buf(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "unable to reserve an output staging directory",
        ))
    }

    pub(crate) fn open_output(&self) -> io::Result<File> {
        self.open(OUTPUT, false)
    }

    pub(crate) fn open_scratch(&self) -> io::Result<File> {
        self.open(SCRATCH, true)
    }

    fn open(&self, name: &str, read: bool) -> io::Result<File> {
        OpenOptions::new()
            .read(read)
            .write(true)
            .create_new(true)
            .open(self.directory.join(name))
    }

    /// The caller flushes, syncs and closes the staged file before publishing.
    pub(crate) fn publish(&self) -> io::Result<()> {
        fs::hard_link(self.directory.join(OUTPUT), &self.destination)
    }
}

impl Drop for OutputStage {
    fn drop(&mut self) {
        // Clean up only this call's staging paths. The destination is a distinct link.
        let _ = fs::remove_file(self.directory.join(OUTPUT));
        let _ = fs::remove_file(self.directory.join(SCRATCH));
        let _ = fs::remove_dir(&self.directory);
    }
}
