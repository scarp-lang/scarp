//! Atomic, no-overwrite output for already compiled native images and objects.

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

/// Publish a Linux executable image at a new path. On Unix the staged file
/// receives execute permission before it becomes visible at the destination.
pub fn write_elf64_linux_executable_new(
    destination: impl AsRef<Path>,
    image: &[u8],
) -> io::Result<()> {
    if image.len() < 64
        || !image.starts_with(b"\x7fELF\x02\x01\x01")
        || image[16..20] != [2, 0, 62, 0]
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a Linux x86-64 ELF64 executable",
        ));
    }
    publish_object(destination.as_ref(), |file| {
        file.write_all(image)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    })
}

/// Publish an emitted x86-64 PE32+ UEFI application at a new path.
/// The header check recognizes the envelope. Structural validation is the
/// responsibility of the compiler emitter.
pub fn write_pe32plus_uefi_application_new(
    destination: impl AsRef<Path>,
    image: &[u8],
) -> io::Result<()> {
    let valid = image.starts_with(b"MZ")
        && image
            .get(0x3c..0x40)
            .and_then(|field| u32::from_le_bytes(field.try_into().ok()?).try_into().ok())
            .and_then(|offset: usize| image.get(offset..))
            .is_some_and(|pe| {
                pe.starts_with(b"PE\0\0\x64\x86")
                    && pe.get(24..26) == Some(&[0x0b, 0x02])
                    && pe.get(92..94) == Some(&[10, 0])
            });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected an x86-64 PE32+ UEFI application",
        ));
    }
    publish_object(destination.as_ref(), |file| file.write_all(image))
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
