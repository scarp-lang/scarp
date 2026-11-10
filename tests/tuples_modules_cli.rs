use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits},
    ExecutionOptions,
};
use std::{
    fs, io,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "scarp-tuples-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli(args: &[&str], path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}
fn compile(files: &[(&str, &str)]) -> Result<modules::ModuleProgram, modules::ModuleDiagnostic> {
    let mut loader = MemorySourceLoader::new();
    for (name, source) in files {
        loader.insert(name, *source).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())
}

#[test]
fn imported_tuple_functions_defaults_contracts_and_private_helpers_work() {
    let files=[("main.srp","import \"lib.srp\" as lib; print(lib.get(),lib.get((7,\"new\"))[1]);"),
        ("lib.srp","pub pure funcra get(value:tuple<integer,string>=(4,\"kept\"))->tuple<integer,string> ensures result==value; {return helper(value);} pure funcra helper(value:tuple<integer,string>)->tuple<integer,string>{return value;}")];
    let program = compile(&files).unwrap_or_else(|e| panic!("{}", e.render()));
    program.check(Default::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, Default::default())
        .unwrap();
    assert_eq!(out, b"(4, \"kept\") new\n");
    assert!(err.is_empty());
    let denied = compile(&[
        (
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.helper((1,)));",
        ),
        (
            "lib.srp",
            "pure funcra helper(value:tuple<integer>)->tuple<integer>{return value;}",
        ),
    ])
    .unwrap_err();
    assert_eq!(denied.diagnostic.kind, Kind::ModuleVisibility);
}

#[test]
fn module_schema_is_versioned_only_when_structural_types_are_exposed() {
    for (source, expected) in [
        ("pub pure funcra f(x:integer)->integer{return x;}", 1),
        (
            "pub pure funcra f(x:tuple<integer>)->tuple<integer>{return x;}",
            2,
        ),
        ("bind local=(1,); print(local);", 1),
    ] {
        let program = compile(&[("main.srp", source)]).unwrap();
        assert_eq!(program.inspection_schema_version(), expected);
        let mut json = Vec::new();
        program.write_inspection_json(&mut json).unwrap();
        let json = String::from_utf8(json).unwrap();
        assert!(
            json.starts_with(&format!("{{\"schema_version\":{expected},")),
            "{json}"
        );
        if expected == 2 {
            assert!(json.contains("tuple<integer>"), "{json}");
        }
    }
}

#[test]
fn module_constant_tuples_are_explicitly_rejected_before_runtime() {
    for files in [
        vec![("main.srp", "pub bind value=(1,);")],
        vec![
            ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
            ("lib.srp", "pub bind value=(1,);"),
        ],
    ] {
        let error = compile(&files).unwrap_err();
        assert_eq!(error.diagnostic.kind, Kind::ModuleSyntax);
        assert!(error.render().contains("scalar"));
    }
}

#[test]
fn imported_runtime_errors_retain_original_unicode_sources_and_calls() {
    let program=compile(&[("main.srp","import \"lib.srp\" as lib; print(\"before\"); print(lib.fail((0,)));") ,("lib.srp","// 🌍\npub pure funcra fail(value:tuple<integer>)->tuple<integer>{return (1/value[0],);}")]).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let error = program
        .execute(&mut out, &mut err, Default::default())
        .unwrap_err();
    let modules::ModuleRunError::Diagnostic(error) = error else {
        panic!()
    };
    assert_eq!(error.diagnostic.kind, Kind::DivisionByZero);
    let text = error.render();
    assert!(
        text.contains("lib.srp:2:") && text.contains("main.srp:1:"),
        "{text}"
    );
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn static_verify_and_navigation_do_not_evaluate_tuple_payloads() {
    let temp = Temp::new();
    let path = temp.file(
        "static.srp",
        "pure funcra fail()->tuple<integer>{return (1/0,);} print(fail());",
    );
    for args in [
        vec!["verify"],
        vec!["inspect", "modules", "--json"],
        vec!["inspect", "symbols", "--json"],
        vec!["inspect", "references", "--json"],
        vec!["inspect", "ast"],
        vec!["inspect", "tokens"],
    ] {
        let result = cli(&args, &path);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert!(result.stderr.is_empty());
        if args[0] == "verify" {
            assert!(result.stdout.is_empty());
        }
    }
    for verb in ["exec", "check"] {
        let result = cli(&[verb, "--diagnostic-format=json"], &path);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8(result.stderr).unwrap().contains("E0008"));
    }
    let quota = temp.file("quota.srp", "print((1,));");
    let result = cli(
        &["exec", "--max-tuple-bytes", "0", "--diagnostic-format=json"],
        &quota,
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8(result.stderr).unwrap().contains("E0061"));
    let result = cli(&["verify", "--max-tuple-bytes", "0"], &quota);
    assert_eq!(result.status.code(), Some(2));
}

#[test]
fn shipped_example_has_exact_bytes_and_new_negative_fixtures_have_exact_codes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("examples/tuples.srp");
    for verb in ["exec", "run"] {
        let result = cli(&[verb], &path);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, include_bytes!("../examples/tuples.stdout"));
        assert!(result.stderr.is_empty());
    }
    let directory = root.join("tests/fixtures/tuples");
    for entry in fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "srp") {
            let code = fs::read_to_string(path.with_extension("error")).unwrap();
            let result = cli(&["exec", "--diagnostic-format=json"], &path);
            assert_eq!(result.status.code(), Some(1), "{result:?}");
            assert!(result.stdout.is_empty());
            let json = String::from_utf8(result.stderr).unwrap();
            assert!(json.contains(code.trim()), "{json}");
        }
    }
}

#[test]
fn module_navigation_keeps_nested_projection_reads_labels_and_utf8_offsets() {
    let mut loader = MemorySourceLoader::new();
    let source =
        "// 🌍\nimport \"lib.srp\" as lib; bind pair=(7,\"é\"); print(lib.echo(value:pair)[1]);";
    loader.insert("main.srp", source).unwrap();
    loader.insert("lib.srp","pub pure funcra echo(value:tuple<integer,string>)->tuple<integer,string>{return value;}").unwrap();
    let indexed = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let index = indexed.index();
    let declaration = index
        .goto_definition(0, source.rfind("pair").unwrap())
        .unwrap();
    assert_eq!(declaration.name.as_ref(), "pair");
    let mut json = Vec::new();
    index.write_references_json(&mut json).unwrap();
    let json = String::from_utf8(json).unwrap();
    assert!(json.starts_with("{\"schema_version\":1,") && json.contains("argument_label"));
}

#[test]
fn cached_tuple_asts_rebase_spans_and_preserve_compiler_results() {
    use scarp::modules::cache::{SyntaxCache, SyntaxCacheLimits};
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let library =
        "// 🌍\npub pure funcra echo(value:tuple<integer>)->tuple<integer>{return value;}";
    for entry in [
        "import \"lib.srp\" as lib; print(lib.echo((1,))[0]);",
        "// shifted entry 🌍\nimport \"lib.srp\" as lib; print(lib.echo((1,))[0]);",
    ] {
        let mut loader = MemorySourceLoader::new();
        loader.insert("main.srp", entry).unwrap();
        loader.insert("lib.srp", library).unwrap();
        for _ in 0..2 {
            let graph = modules::load_with_syntax_cache(
                "main.srp",
                &loader,
                Default::default(),
                &mut cache,
            )
            .unwrap();
            let indexed = modules::resolve_indexed(graph, Default::default()).unwrap();
            let mut actual = Vec::new();
            indexed.index().write_references_json(&mut actual).unwrap();
            let cold = modules::compile_indexed_with_loader(
                "main.srp",
                &loader,
                Default::default(),
                Default::default(),
            )
            .unwrap();
            let mut expected = Vec::new();
            cold.index().write_references_json(&mut expected).unwrap();
            assert_eq!(actual, expected);
            let program = modules::resolve(
                modules::load_with_syntax_cache(
                    "main.srp",
                    &loader,
                    Default::default(),
                    &mut cache,
                )
                .unwrap(),
            )
            .unwrap();
            let mut out = Vec::new();
            program
                .execute(&mut out, &mut Vec::new(), Default::default())
                .unwrap();
            assert_eq!(out, b"1\n");
        }
    }
}

#[test]
fn explicit_primitive_host_conversions_and_numeric_api_are_preserved() {
    use scarp::{
        numeric::{self, Builtin},
        types::{PrimitiveType as P, SourceType},
        value::{FiniteFloat, Value},
    };
    for primitive in P::ALL {
        let source = SourceType::from(*primitive);
        assert_eq!(source.as_primitive(), Some(*primitive));
        assert_eq!(*primitive as usize, primitive.id().index());
    }
    let value = Value::Integer(7);
    assert_eq!(value.as_integer(), Some(7));
    assert_eq!(value.kind().as_primitive(), Some(P::Integer));
    let float = Value::Float(FiniteFloat::new(2.0).unwrap());
    assert_eq!(float.as_float(), Some(2.0));
    let result = numeric::evaluate(Builtin::ToFloat, &[value], 100).unwrap();
    assert_eq!(result.as_float(), Some(7.0));
}

#[test]
fn broken_pipe_semantics_remain_host_output_errors() {
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let error = scarp::run_with_options(
        "print((1,\"x\"));",
        &mut Broken,
        ExecutionOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(error,scarp::RunError::Output(e) if e.kind()==io::ErrorKind::BrokenPipe));
}

#[test]
fn folding_a_module_constant_does_not_turn_a_named_projection_into_literal_syntax() {
    for files in [
        vec![("main.srp", "pub bind index=0; print((7,)[index]);")],
        vec![
            (
                "main.srp",
                "import \"lib.srp\" as lib; print((7,)[lib.index]);",
            ),
            ("lib.srp", "pub bind index=0;"),
        ],
    ] {
        let error = compile(&files).unwrap_err();
        assert_eq!(error.diagnostic.kind, Kind::TupleIndex);
    }
    let program = compile(&[("main.srp", "pub bind index=0; print([7.0][index]);")]).unwrap();
    let mut out = Vec::new();
    program
        .execute(&mut out, &mut Vec::new(), Default::default())
        .unwrap();
    assert_eq!(out, b"7.0\n");
}
