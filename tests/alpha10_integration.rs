use scarp::{
    diagnostics::DiagnosticKind,
    modules::{
        self,
        cache::{CacheStatus, ConstantCache, EditorCompilationCache},
        MemorySourceLoader, ModuleRunError,
    },
    ExecutionOptions, InputLimits,
};
use std::io::Cursor;

const ENTRY: &str = r#"
import "lib/café.srp" as cafe_lib;
bind label = cafe_lib.read_label();
bind parts: array<bigint, 2> = cafe_lib.parts(bigint("-100000000000000000007"));
print(label, parts, cafe_lib.exact);
"#;

const LIBRARY: &str = r#"
// नमस्ते 🌍
pub bind exact = 9;
pub funcra read_label() -> string {
    return ask_or("Name> ", "Guest");
}
pub pure funcra parts(n: bigint) -> array<bigint, 2> {
    bind result = bigint_divmod(n, bigint_from_integer(exact));
    return [array: result[0], result[1]];
}
"#;

fn execute(program: &modules::ModuleProgram, input: &[u8]) -> Vec<u8> {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    program
        .execute_with_interactive_input(
            &mut Cursor::new(input),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap();
    assert!(stderr.is_empty());
    stdout
}

#[test]
fn imported_arrays_bigint_and_prompts_survive_warm_editor_revisions() {
    let mut loader = MemorySourceLoader::new();
    loader.set_document("main.srp", ENTRY, 1).unwrap();
    let library = LIBRARY.replace('\n', "\r\n");
    loader.set_document("lib/café.srp", &library, 2).unwrap();
    let graph = |loader: &MemorySourceLoader| {
        modules::load("main.srp", loader, Default::default()).unwrap()
    };
    let mut cache = EditorCompilationCache::new(Default::default());
    let mut constants = ConstantCache::new(Default::default());
    let first = cache
        .resolve_with_loaded_revisions(graph(&loader), Default::default(), Some(&mut constants))
        .unwrap();
    assert_eq!(first.status, CacheStatus::Stored);
    assert_eq!(first.program.inspection_schema_version(), 3);

    loader.set_document("lib/café.srp", &library, 3).unwrap();
    let warm = cache
        .resolve_with_loaded_revisions(graph(&loader), Default::default(), Some(&mut constants))
        .unwrap();
    assert_eq!(warm.status, CacheStatus::Hit);
    assert!(first.snapshot.ensure_current(&[1, 3]).is_err());
    warm.snapshot.ensure_current(&[1, 3]).unwrap();

    let fresh = modules::resolve_indexed(graph(&loader), Default::default()).unwrap();
    assert_eq!(warm.snapshot.symbols().symbols(), fresh.index().symbols());
    assert_eq!(
        warm.snapshot.symbols().references(),
        fresh.index().references()
    );
    for (input, expected) in [
        (
            &b""[..],
            "Name> Guest [array: -11111111111111111111, -8] 9\n",
        ),
        (
            "नमस्ते 🌍\r\n".as_bytes(),
            "Name> नमस्ते 🌍 [array: -11111111111111111111, -8] 9\n",
        ),
    ] {
        assert_eq!(execute(&warm.program, input), expected.as_bytes());
        assert_eq!(execute(fresh.program(), input), expected.as_bytes());
    }

    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = warm
        .program
        .execute_with_interactive_input(
            &mut Cursor::new(b"\xff\n"),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap_err();
    let ModuleRunError::Diagnostic(error) = error else {
        panic!("expected input encoding diagnostic");
    };
    assert_eq!(error.diagnostic.kind, DiagnosticKind::InputEncoding);
    let location = error.sources.locate_span(error.diagnostic.span).unwrap();
    assert_eq!(location.source, 1);
    assert_eq!(
        &library[location.span.start..location.span.end],
        r#"ask_or("Name> ", "Guest")"#
    );
    assert_eq!(stdout, b"Name> ");
    assert!(stderr.is_empty());
}
