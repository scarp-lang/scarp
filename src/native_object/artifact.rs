//! Atomic, no-overwrite output for already compiled ELF64 objects.

use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

use crate::output_stage::OutputStage;

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

fn publish_object(
    destination: &Path,
    write_object: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let stage = OutputStage::create(destination, "object")?;
    {
        let mut file = stage.open_output()?;
        write_object(&mut file)?;
        file.sync_all()?;
    }
    // hard_link has an atomic create-if-absent contract. A rename fallback
    // would overwrite existing output on some hosts.
    stage.publish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

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
