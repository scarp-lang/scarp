use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    modules::{
        self,
        cache::{SyntaxCache, SyntaxCacheLimits},
        MemorySourceLoader, ModuleLimits,
    },
    parser::{MAX_BLOCK_DEPTH, MAX_EXPRESSION_DEPTH},
    ExecutionOptions, InputLimits, RunError,
};

fn run(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn selected<'a>(source: &'a str, diagnostic: &Diagnostic) -> &'a str {
    &source[diagnostic.span.start..diagnostic.span.end]
}

fn runtime_error(source: &str, options: ExecutionOptions, kind: Kind) -> Diagnostic {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let RunError::Diagnostic(diagnostic) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected a runtime diagnostic")
    };
    assert_eq!(diagnostic.kind, kind, "{diagnostic:?}");
    assert!(out.is_empty() && err.is_empty());
    diagnostic
}

#[test]
fn values_work_in_bindings_updates_operators_and_tuple_projections() {
    let source = r#"
        bind rating = 76;
        bind label = if rating >= 80 { "excellent" } elif rating >= 50 { "passed" } else { "failed" };
        var total = 10 + if true { 1 + 2 } else { 0 } * 3;
        total += if false { 100 } else { 1 };
        bind pair = if true { ("नमस्ते", (1, true)) } else { ("bye", (2, false)) };
        print(label, total, pair[0], pair[1][0]);
        print(if false { (1, "one") } else { (2, "two") }[1]);
        if if false { true } else { false } { print("wrong"); }
        elif if true { true } else { false } { print("statement"); }
        if true { print(-if true { 2 } else { 1 }); }
    "#;
    assert_eq!(run(source), "passed 20 नमस्ते 1\ntwo\nstatement\n-2\n");
}

#[test]
fn calls_defaults_contracts_and_return_values_use_normal_expression_rules() {
    let source = r#"
        pure funcra pick(enabled: boolean, n: integer = if enabled { 10 } else { 20 }) -> integer
            needs if enabled { n > 0 } else { true };
            ensures if true { result > 0 } else { false };
        {
            return if n < 12 { n + 1 } elif n > 15 { n - 1 } else { 12 };
        }
        print(pick(if false { false } else { if true { true } else { false } }), pick(false));
        print(if false { 0 } else { pick(enabled: true, n: 12) });
    "#;
    assert_eq!(run(source), "11 19\n12\n");
}

#[test]
fn only_reached_conditions_and_results_use_input_and_call_frames() {
    let source = r#"
        funcra line() -> string { return read_line(); }
        print(if line() == "ready" { line() } elif line() == "unreached" { line() } else { "wrong" },
              if false { line() } else { read_line() });
    "#;
    scarp::compile(source).unwrap();
    let mut input = std::io::Cursor::new("ready\nपहला\r\nदूसरा\n".as_bytes());
    let mut out = Vec::new();
    let mut err = Vec::new();
    scarp::run_with_input_and_options(
        source,
        &mut input,
        &mut out,
        &mut err,
        ExecutionOptions::default(),
        InputLimits {
            max_reads: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(out, "पहला दूसरा\n".as_bytes());
    assert!(err.is_empty());
    assert_eq!(
        run("print(if true { \"safe\" } else { read_line() });"),
        "safe\n"
    );
}

#[test]
fn short_circuiting_preserves_selected_errors_and_output_atomicity() {
    let source = "print(if false and 1 / 0 == 0 { 1 / 0 } elif true { 7 } elif 1 / 0 == 0 { 1 / 0 } else { 1 / 0 });";
    assert_eq!(run(source), "7\n");
    for conditional in [
        "if true { 1 / 0 } else { 7 }",
        "if false { 7 } elif 1 / 0 == 0 { 8 } else { 9 }",
        "if false { 7 } else { 1 / 0 }",
    ] {
        let source = format!("print(\"पहले\"); eprint(\"before\"); print({conditional});");
        scarp::compile(&source).unwrap();
        let error = runtime_error(&source, ExecutionOptions::default(), Kind::DivisionByZero);
        assert_eq!(selected(&source, &error), "/");
    }
}

#[test]
fn syntax_requires_one_value_per_branch_and_a_final_else() {
    for (source, token, expected) in [
        ("print(if true { 1 });", ")", "'else'"),
        ("print(if false { 1 } elif true { 2 });", ")", "'else'"),
        (
            "print(if true { 1; } else { 2 });",
            ";",
            "one conditional result",
        ),
        (
            "print(if true { 1 2 } else { 3 });",
            "2",
            "one conditional result",
        ),
        ("print(if true {} else { 2 });", "}", "a literal"),
        ("print(if true { 1 } else 2);", "2", "'{'"),
        (
            "print(if true { bind x = 1; } else { 2 });",
            "x",
            "one conditional result",
        ),
    ] {
        let error = scarp::parse(source).unwrap_err();
        assert_eq!(error.kind, Kind::UnexpectedToken, "{source}: {error:?}");
        assert_eq!(selected(source, &error), token);
        assert!(error.message.contains(expected), "{source}: {error:?}");
    }
}

#[test]
fn static_resolution_checks_unreached_conditions_results_and_effects() {
    for (source, kind, token) in [
        ("print(if 1 { 2 } else { 3 });", Kind::TypeMismatch, "1"),
        ("print(if true { 2 } elif \"नहीं\" { 3 } else { 4 });", Kind::TypeMismatch, "\"नहीं\""),
        ("print(if true { 1 } else { missing });", Kind::UnknownName, "missing"),
        ("print(if false { 1 } elif true { 2.0 } else { 3 });", Kind::TypeMismatch, "2.0"),
        ("print(if true { (1, (true,)) } else { (2, (false, 1)) });", Kind::TypeMismatch, "(2, (false, 1))"),
        ("funcra f()->unit {} print(if true { f() } else { f() });", Kind::InvalidReturn, "f()"),
        ("pure funcra f()->string { return if true { \"ok\" } else { read_line() }; }", Kind::ImpureFunction, "read_line"),
        ("funcra g()->integer { return 1; } funcra f(n: integer = if true { 2 } else { g() }) -> unit {}", Kind::ImpureFunction, "g"),
        ("funcra f()->unit needs if true { true } else { read_line() == \"ok\" }; {}", Kind::ImpureFunction, "read_line"),
    ] {
        let error = scarp::compile(source).unwrap_err();
        assert_eq!(error.kind, kind, "{source}: {error:?}");
        assert_eq!(selected(source, &error), token, "{source}: {error:?}");
    }

    let source = "// 🌍\r\nprint(if false { \"हिन्दी\" } else { 1 });";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, Kind::TypeMismatch);
    assert_eq!(selected(source, &error), "1");
    let first = error.related[0].span;
    assert_eq!(&source[first.start..first.end], "\"हिन्दी\"");
}

#[test]
fn unselected_allocations_and_long_elif_chains_do_not_consume_runtime_quotas() {
    let source = format!(
        "print(if true {{ \"\" }} {} else {{ \"unreached\" }});",
        "elif false { \"blocked\" } ".repeat(256)
    );
    let mut options = ExecutionOptions {
        max_text_bytes: 0,
        ..Default::default()
    };
    options.limits.max_steps = 32;
    let mut out = Vec::new();
    scarp::run_with_options(&source, &mut out, options).unwrap();
    assert_eq!(out, b"\n");
    runtime_error(
        &source.replacen("if true", "if false", 1),
        options,
        Kind::ExecutionLimit,
    );
    let error = runtime_error(
        "print(if false { \"\" } else { \"chosen\" });",
        options,
        Kind::TextConstruction,
    );
    assert_eq!(
        selected("print(if false { \"\" } else { \"chosen\" });", &error),
        "\"chosen\""
    );
}

#[test]
fn conditional_nesting_shares_the_expression_limit_on_small_host_stacks() {
    let programs = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            [false, true].map(|mixed| {
                let mut expression = "1".to_string();
                for level in 0..MAX_EXPRESSION_DEPTH {
                    expression = if mixed && level % 2 == 1 {
                        format!("({expression})")
                    } else {
                        format!("if true {{ {expression} }} else {{ 0 }}")
                    };
                }
                let blocks_open = "{".repeat(MAX_BLOCK_DEPTH);
                let blocks_close = "}".repeat(MAX_BLOCK_DEPTH);
                let source = format!("{blocks_open}print({expression});{blocks_close}");
                let program = scarp::parse(&source).unwrap();
                let too_deep = format!(
                    "{blocks_open}print(if true {{ {expression} }} else {{ 0 }});{blocks_close}"
                );
                assert_eq!(
                    scarp::parse(&too_deep).unwrap_err().kind,
                    Kind::ExpressionNesting
                );
                program
            })
        })
        .unwrap()
        .join()
        .unwrap();
    for program in programs {
        let program = scarp::resolver::resolve(program).unwrap();
        let mut out = Vec::new();
        scarp::interpreter::execute(&program, &mut out).unwrap();
        assert_eq!(out, b"1\n");
    }
}

#[test]
fn imported_functions_and_cached_module_diagnostics_keep_source_spans() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.pick(false));",
        )
        .unwrap();
    loader.insert("lib.srp", "pub pure funcra pick(b: boolean)->string { return if b { \"yes\" } else { \"नहीं\" }; }").unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let mut out = Vec::new();
    program
        .execute(&mut out, &mut Vec::new(), ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, "नहीं\n".as_bytes());

    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let library =
        "// café\r\npub pure funcra pick()->integer { return if true { 1 } else { \"त्रुटि\" }; }";
    for prefix in ["", "// more bytes 🌍\r\n"] {
        let mut loader = MemorySourceLoader::new();
        loader
            .insert("main.srp", format!("{prefix}import \"lib.srp\" as lib;"))
            .unwrap();
        loader.insert("lib.srp", library).unwrap();
        let parsed = modules::load_with_syntax_cache(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            &mut cache,
        )
        .unwrap();
        let Err(error) = modules::resolve(parsed) else {
            panic!("expected a type diagnostic");
        };
        assert_eq!(error.diagnostic.kind, Kind::TypeMismatch);
        let location = error.sources.locate_span(error.diagnostic.span).unwrap();
        assert_eq!(&library[location.span.start..location.span.end], "\"त्रुटि\"");
        assert_eq!(location.start.line, 2);
    }
}

#[test]
fn constant_and_native_subsets_reject_conditional_expressions() {
    let expression = "if true { 1 } else { 2 }";
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib;")
        .unwrap();
    let library = format!("pub bind number = {expression};");
    loader.insert("lib.srp", &library).unwrap();
    let Err(error) = modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())
    else {
        panic!("expected module constant rejection");
    };
    assert_eq!(error.diagnostic.kind, Kind::ModuleSyntax);
    let span = error
        .sources
        .locate_span(error.diagnostic.span)
        .unwrap()
        .span;
    assert_eq!(&library[span.start..span.end], expression);

    let source = format!("bind number = {expression}; print(number);");
    for error in [
        scarp::c_backend::emit_c(&source).unwrap_err(),
        scarp::native_object::emit_elf64_integer_kernel(&source).unwrap_err(),
    ] {
        assert_eq!(error.kind, Kind::NativeUnsupported);
        assert_eq!(selected(&source, &error), expression);
    }
}
