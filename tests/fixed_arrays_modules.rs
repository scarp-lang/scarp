use scarp::{
    diagnostics::{DiagnosticKind as Kind, Span},
    modules::{self, cache::SyntaxCache, MemorySourceLoader, ModuleProgram, ModuleRunError},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn compile(files: &[(&str, &str)]) -> Result<ModuleProgram, modules::ModuleDiagnostic> {
    let mut loader = MemorySourceLoader::new();
    for &(path, source) in files {
        loader.insert(path, source).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, Default::default())
}

#[test]
fn modules_link_fixed_array_signatures_defaults_contracts_and_private_helpers() {
    let library = r#"
        pub pure funcra labels(value:array<string, 2>=[array:"नमस्ते", "Scarp"])
          ->array<string, 2> ensures result==value; {return helper(value);}
        pure funcra helper(value:array<string, 2>)->array<string, 2>{return value;}
    "#;
    let entry = r#"import "lib.srp" as lib; print(lib.labels()[0], lib.labels(value:[array:"new","name"]));"#;
    let program = compile(&[("main.srp", entry), ("lib.srp", library)]).unwrap();
    program.check(Default::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, Default::default())
        .unwrap();
    assert_eq!(out, "नमस्ते [array: \"new\", \"name\"]\n".as_bytes());
    assert!(err.is_empty());
    assert_eq!(
        compile(&[
            (
                "main.srp",
                r#"import "lib.srp" as lib; print(lib.helper([array:"x","y"]));"#,
            ),
            ("lib.srp", library)
        ])
        .unwrap_err()
        .diagnostic
        .kind,
        Kind::ModuleVisibility
    );
}

#[test]
fn inventory_schema_selects_serialized_types_and_preserves_exact_json() {
    for (kind, version, canonical) in [
        ("integer", 1, "integer"),
        ("tuple<integer>", 2, "tuple<integer>"),
        ("tuple<array<string,0>>", 3, "tuple<array<string, 0>>"),
    ] {
        let source = format!("pure funcra f(x:{kind})->{kind}{{return x;}}");
        let program = compile(&[("main.srp", &source)]).unwrap();
        assert_eq!(program.inspection_schema_version(), version);
        let expected = format!(
            concat!(
                r#"{{"schema_version":{},"entry":0,"modules":[{{"id":0,"path":"main.srp","#,
                r#""imports":[],"dependencies":[],"dependents":[],"symbols":[{{"name":"f","public":false,"span":[12,13],"kind":"function","pure":true,"result":"{}","#,
                r#""parameters":[{{"name":"x","type":"{}","has_default":false,"span":[14,15]}}]}}]}}],"constant_dependencies":[]}}"#,
                "\n"
            ),
            version, canonical, canonical
        );
        let mut bytes = Vec::new();
        program.write_inspection_json(&mut bytes).unwrap();
        assert_eq!(bytes, expected.as_bytes());
    }
    for (source, schema) in [
        ("pure funcra f()->integer{return [array:1][0];}", 1),
        (
            "pure funcra f()->tuple<integer>{return ([array:1][0],);}",
            2,
        ),
        ("pure funcra f(a:array<integer,0>)->integer{return 1;}", 3),
    ] {
        assert_eq!(
            compile(&[("main.srp", source)])
                .unwrap()
                .inspection_schema_version(),
            schema
        );
    }
}

#[test]
fn composite_module_constants_remain_scalar_only() {
    for expression in ["[array: 7]", "[array: <integer>]", "[array: 7][0]"] {
        let source = format!("pub bind a={expression};");
        let error = compile(&[("main.srp", &source)]).unwrap_err();
        assert_eq!(error.diagnostic.kind, Kind::ModuleSyntax);
        assert_eq!(error.diagnostic.kind.code(), "E0037");
    }
}

#[test]
fn cached_asts_keep_original_array_error_locations_in_utf8_crlf_modules() {
    let lib = "// नमस्ते 🌍\r\npub pure funcra get(a:array<integer,1>,i:integer)->integer{return a[i+1];}\r\n";
    let entry =
        r#"import "lib.srp" as lib; print("out"); eprint("err"); print(lib.get([array: 7], i:0));"#;
    let mut cache = SyntaxCache::new(Default::default());
    for prefix in ["", "// longer entry 🌍\r\n"] {
        let mut loader = MemorySourceLoader::new();
        loader.insert("lib.srp", lib).unwrap();
        let source = format!("{prefix}{entry}");
        loader.insert("main.srp", &source).unwrap();
        let graph =
            modules::load_with_syntax_cache("main.srp", &loader, Default::default(), &mut cache)
                .unwrap();
        let program = modules::resolve(graph).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ModuleRunError::Diagnostic(error) = program
            .execute(&mut out, &mut err, Default::default())
            .unwrap_err()
        else {
            panic!("expected array bounds diagnostic");
        };
        assert_eq!(error.diagnostic.kind, Kind::ArrayIndex);
        let site = error.sources.locate_span(error.diagnostic.span).unwrap();
        let start = lib.find("i+1").unwrap();
        assert_eq!(site.source, 1);
        assert_eq!(site.span, Span::new(start, start + 3));
        assert_eq!(site.start.line, 2);
        assert!(error.render().contains("main.srp"));
        assert!(out.is_empty() && err.is_empty());
    }
    assert!(cache.stats().hits > 0);
}

#[test]
fn both_native_backends_reject_arrays_before_emission() {
    for source in [
        "bind values=[array:1];",
        "bind result=[array:1][0];",
        "bind result=([array:1],)[0][0];",
        "pure funcra unused()->array<integer,0>{return [array: <integer>];} bind result=1;",
    ] {
        let c = scarp::c_backend::emit_c(source).unwrap_err();
        let elf = scarp::native_object::emit_elf64_integer_kernel(source).unwrap_err();
        assert_eq!(c.kind, Kind::NativeUnsupported);
        assert_eq!(elf.kind, Kind::NativeUnsupported);
        assert_eq!(c.kind.code(), "E0049");
    }
    assert!(scarp::c_backend::emit_c("print(1);").is_ok());
    assert!(scarp::native_object::emit_elf64_integer_kernel("bind result=1;").is_ok());
}

fn cli(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn cli_runs_the_example_checks_array_quota_and_preserves_navigation_schema() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/fixed_arrays.srp");
    let result = cli(&["exec"], &source);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        result.stdout,
        include_bytes!("../examples/fixed_arrays.stdout")
    );
    assert!(result.stderr.is_empty());
    for verb in ["exec", "check"] {
        let result = cli(
            &[verb, "--max-array-bytes", "0", "--diagnostic-format=json"],
            &source,
        );
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8(result.stderr)
            .unwrap()
            .contains("\"code\":\"E0064\""));
    }
    let result = cli(&["verify", "--max-array-bytes", "0"], &source);
    assert_eq!(result.status.code(), Some(2));
    for (kind, expected) in [("modules", 3), ("symbols", 1), ("references", 1)] {
        let result = cli(&["inspect", kind, "--json"], &source);
        assert!(result.status.success(), "{result:?}");
        assert!(result
            .stdout
            .starts_with(format!("{{\"schema_version\":{expected},").as_bytes()));
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn cli_verify_is_static_for_literal_array_indices_and_json_reports_their_byte_spans() {
    let dir = std::env::temp_dir().join(format!("scarp-fixed-array-bounds-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bounds.srp");
    let source =
        "// 🌍\r\nprint(\"queued\"); eprint(\"queued\"); print([array: <integer>][-1]);\r\n";
    fs::write(&path, source).unwrap();
    let verified = cli(&["verify"], &path);
    let executed = cli(&["exec", "--diagnostic-format=json"], &path);
    fs::remove_dir_all(&dir).unwrap();
    assert!(verified.status.success() && verified.stdout.is_empty() && verified.stderr.is_empty());
    assert_eq!(executed.status.code(), Some(1));
    assert!(executed.stdout.is_empty());
    let json = String::from_utf8(executed.stderr).unwrap();
    assert!(json.starts_with("{\"schema_version\":1,"));
    assert!(json.contains("\"code\":\"E0063\""));
    assert!(!json.contains("queued"));
    let start = source.find("-1").unwrap();
    assert!(
        json.contains(&format!("\"span\":[{start},{}]", start + 2)),
        "{json}"
    );
}
