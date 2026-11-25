//! Atomic, no-overwrite output for already compiled ELF64 objects.

use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_STAGE: AtomicUsize = AtomicUsize::new(0);
const STAGE_ATTEMPTS: usize = 64;

/// Write an emitted Linux x86-64 ELF64 relocatable object at a new path.
///
/// After the header check, the writer stages the complete object in the
/// output directory and publishes it with an atomic no-overwrite hard link.
/// An existing file, directory or symlink at the output path is preserved.
/// A filesystem that does not support hard links returns an I/O error.
/// The header check recognizes the target envelope. The compiler emitters
/// are responsible for validating structural layout and machine code.
pub fn write_elf64_object_new(destination: impl AsRef<Path>, object: &[u8]) -> io::Result<()> {
    if object.len() < 64
        || !object.starts_with(b"\x7fELF\x02\x01\x01")
        || object[16..20] != [1, 0, 62, 0]
    // ET_REL, EM_X86_64
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a Linux x86-64 ELF64 relocatable object",
        ));
    }
    publish_object(destination.as_ref(), |file| file.write_all(object))
}

struct Stage {
    directory: PathBuf,
    object: PathBuf,
}

impl Stage {
    fn create(parent: &Path, destination_name: &std::ffi::OsStr) -> io::Result<Self> {
        let mut builder = DirBuilder::new();
        builder.recursive(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        for _ in 0..STAGE_ATTEMPTS {
            let nonce = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let name = format!(".scarp-object-{}-{nonce}.tmp", std::process::id());
            // Also avoid the ordinary case-insensitive and trailing-dot
            // aliases of this ASCII name on Windows.
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
                    let object = directory.join("object");
                    return Ok(Self { directory, object });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "unable to reserve a native object staging directory",
        ))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // Clean up only our own staging paths. The successful destination
        // is a distinct link and is never removed by this guard.
        let _ = fs::remove_file(&self.object);
        let _ = fs::remove_dir(&self.directory);
    }
}

fn publish_object(
    destination: &Path,
    write_object: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let name = destination.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "expected an output filename")
    })?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stage = Stage::create(parent, name)?;
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage.object)?;
        write_object(&mut file)?;
        file.sync_all()?;
    }
    // hard_link has an atomic create-if-absent contract. A rename fallback
    // would overwrite existing output on some hosts.
    fs::hard_link(&stage.object, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_write_does_not_expose_partial_bytes_or_leave_staging() {
        let sandbox =
            std::env::temp_dir().join(format!("scarp-object-write-failure-{}", std::process::id()));
        fs::create_dir(&sandbox).unwrap();
        struct Remove(PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Remove(sandbox.clone());
        let output = sandbox.join("destination.o");
        let result = publish_object(&output, |file| {
            file.write_all(b"staged prefix")?;
            assert!(matches!(
                fs::symlink_metadata(&output).unwrap_err().kind(),
                io::ErrorKind::NotFound
            ));
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected write error",
            ))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WriteZero);
        assert_eq!(fs::read_dir(&sandbox).unwrap().count(), 0);
    }
}
