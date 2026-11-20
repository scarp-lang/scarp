use scarp::{
    diagnostics::DiagnosticKind,
    modules::{
        self,
        cache::{CacheStatus, EditorCompilationCache},
        symbols::IndexLimits,
        MemorySourceLoader, ModuleProgram,
    },
};
use std::{path::Path, process::Command, sync::Arc};

const ENTRY: &str = "core_collections.srp";
const LIBRARY: &str = "modules/collection_tools.srp";
const SOURCE: &str = include_str!("../examples/core_collections.srp");
const MODULE: &str = include_str!("../examples/modules/collection_tools.srp");
const EXPECTED: &[u8] = include_bytes!("../examples/core_collections.stdout");

fn assert_output(program: &ModuleProgram) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, Default::default())
        .unwrap();
    assert_eq!(out, EXPECTED);
    assert!(err.is_empty());
}

#[test]
fn cli_and_embedding_agree_on_the_module_example() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(ENTRY);
    let executable = env!("CARGO_BIN_EXE_scarp");
    let verified = Command::new(executable)
        .arg("verify")
        .arg(&example)
        .output()
        .unwrap();
    assert!(verified.status.success(), "{:?}", verified.stderr);
    assert!(verified.stdout.is_empty() && verified.stderr.is_empty());

    let executed = Command::new(executable)
        .arg("exec")
        .arg(example)
        .output()
        .unwrap();
    assert!(executed.status.success(), "{:?}", executed.stderr);
    assert_eq!(executed.stdout, EXPECTED);
    assert!(executed.stderr.is_empty());

    let mut loader = MemorySourceLoader::new();
    loader.insert(ENTRY, SOURCE).unwrap();
    loader.insert(LIBRARY, MODULE).unwrap();
    let program = modules::compile_with_loader(ENTRY, &loader, Default::default()).unwrap();
    assert_eq!(program.inspection_schema_version(), 3);
    assert_output(&program);
}

#[test]
fn warm_editor_array_interfaces_match_fresh_navigation_and_diagnostics() {
    let mut loader = MemorySourceLoader::new();
    loader.set_document(ENTRY, SOURCE, 1).unwrap();
    let library = MODULE.replace('\n', "\r\n");
    loader.set_document(LIBRARY, &library, 2).unwrap();
    let load =
        |loader: &MemorySourceLoader| modules::load(ENTRY, loader, Default::default()).unwrap();
    let mut cache = EditorCompilationCache::new(Default::default());
    let before = cache
        .resolve_with_loaded_revisions(load(&loader), Default::default(), None)
        .unwrap();

    loader.set_document(LIBRARY, &library, 3).unwrap();
    let warm = cache
        .resolve_with_loaded_revisions(load(&loader), Default::default(), None)
        .unwrap();
    assert_eq!(warm.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&warm.program, &before.program));
    assert!(before.snapshot.ensure_current(&[1, 3]).is_err());
    warm.snapshot.ensure_current(&[1, 3]).unwrap();

    let fresh = modules::resolve_indexed(load(&loader), IndexLimits::default()).unwrap();
    assert_eq!(warm.snapshot.symbols().symbols(), fresh.index().symbols());
    assert_eq!(
        warm.snapshot.symbols().references(),
        fresh.index().references()
    );
    assert_eq!(warm.program.inspection_schema_version(), 3);
    assert_output(&warm.program);

    let wrong = library.replace("array<string, 3>", "array<string, 2>");
    loader.set_document(LIBRARY, wrong, 4).unwrap();
    let actual = cache
        .resolve_with_loaded_revisions(load(&loader), Default::default(), None)
        .unwrap_err();
    let expected = modules::resolve_indexed(load(&loader), IndexLimits::default()).unwrap_err();
    let scarp::modules::cache::EditorCompileError::Module(actual) = actual else {
        panic!("expected the edited module's diagnostic");
    };
    assert_eq!(actual.diagnostic.kind, DiagnosticKind::InvalidReturn);
    assert_eq!(actual.diagnostic, expected.diagnostic);
    assert_output(&warm.program);
}
