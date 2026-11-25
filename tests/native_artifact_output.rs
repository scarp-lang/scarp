use scarp::native_object::{emit_elf64_integer_kernel, write_elf64_object_new};
use std::{
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "scarp object output {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn emitted_bytes_are_preserved_in_unicode_paths_and_existing_outputs_survive() {
    let sandbox = Sandbox::new();
    let object = emit_elf64_integer_kernel("bind value = 42;").unwrap();
    let output = sandbox.0.join("परिणाम file.o");
    write_elf64_object_new(&output, &object).unwrap();
    assert_eq!(fs::read(&output).unwrap(), object);

    let second = emit_elf64_integer_kernel("bind value = 13;").unwrap();
    assert!(write_elf64_object_new(&output, &second).is_err());
    assert_eq!(fs::read(&output).unwrap(), object);

    let directory = sandbox.0.join("existing");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("keep"), b"untouched").unwrap();
    assert!(write_elf64_object_new(&directory, &second).is_err());
    assert_eq!(fs::read(directory.join("keep")).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 2);
}

#[test]
fn invalid_target_envelopes_and_missing_parents_do_not_leave_files() {
    let sandbox = Sandbox::new();
    let output = sandbox.0.join("artifact.o");
    assert_eq!(
        write_elf64_object_new(&output, b"partial")
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    let object = emit_elf64_integer_kernel("bind value = 42;").unwrap();
    let mut wrong_machine = object.clone();
    wrong_machine[18] = 0;
    assert_eq!(
        write_elf64_object_new(&output, &wrong_machine)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        write_elf64_object_new(sandbox.0.join("missing/artifact.o"), &object)
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn existing_symlinks_are_preserved_even_when_the_target_is_missing() {
    use std::os::unix::fs::symlink;

    let sandbox = Sandbox::new();
    let object = emit_elf64_integer_kernel("bind value = 42;").unwrap();
    let existing = sandbox.0.join("existing.o");
    let missing = sandbox.0.join("missing.o");
    fs::write(&existing, b"keep").unwrap();
    for (link, target) in [
        (sandbox.0.join("existing-link.o"), &existing),
        (sandbox.0.join("dangling-link.o"), &missing),
    ] {
        symlink(target, &link).unwrap();
        assert!(write_elf64_object_new(&link, &object).is_err());
        assert_eq!(&fs::read_link(link).unwrap(), target);
    }
    assert_eq!(fs::read(existing).unwrap(), b"keep");
    assert!(!missing.exists());
    assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 3);
}

#[test]
fn concurrent_writers_publish_exactly_one_complete_artifact() {
    let sandbox = Sandbox::new();
    let output = sandbox.0.join("raced.o");
    let barrier = Arc::new(Barrier::new(8));
    let writers = (1..=8)
        .map(|value| {
            let output = output.clone();
            let barrier = barrier.clone();
            let artifact = emit_elf64_integer_kernel(&format!("bind value = {value};")).unwrap();
            std::thread::spawn(move || {
                barrier.wait();
                let published = write_elf64_object_new(&output, &artifact).is_ok();
                (published, artifact)
            })
        })
        .collect::<Vec<_>>();
    let winners = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .filter_map(|(published, artifact)| published.then_some(artifact))
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    assert_eq!(fs::read(output).unwrap(), winners[0]);
    assert_eq!(fs::read_dir(&sandbox.0).unwrap().count(), 1);
}
