use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as K, Span},
    modules::{self, MemorySourceLoader, ModuleDiagnostic, ModuleLimits},
};
fn json(error: &Diagnostic, source: &str) -> String {
    let mut out = Vec::new();
    error.write_json("test.srp", source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}
#[test]
fn schema_one_golden_has_local_bytes_scalar_positions_notes_and_help() {
    let error = Diagnostic::new(K::TypeMismatch, Span::new(4, 8), "bad\0\"type")
        .with_related(Span::new(8, 9), "related\nmessage")
        .with_help("repair\tthis");
    assert_eq!(json(&error,"é\r\n🌍x"),concat!(
      "{\"schema_version\":1,\"severity\":\"error\",\"code\":\"E0015\",\"message\":\"bad\\u0000\\\"type\",",
      "\"span_encoding\":\"utf-8-bytes\",\"column_encoding\":\"unicode-scalar\",\"position_base\":1,",
      "\"primary\":{\"path\":\"test.srp\",\"span\":[4,8],\"start\":{\"line\":2,\"column\":1},\"end\":{\"line\":2,\"column\":2}},",
      "\"related\":[{\"message\":\"related\\nmessage\",\"location\":{\"path\":\"test.srp\",\"span\":[8,9],\"start\":{\"line\":2,\"column\":2},\"end\":{\"line\":2,\"column\":3}}}],\"help\":\"repair\\tthis\"}\n"));
}
#[test]
fn invalid_spans_are_null_instead_of_being_clamped_or_panicking() {
    for span in [
        Span::new(1, 2),
        Span::new(0, 1),
        Span::new(3, 2),
        Span::new(0, 99),
        Span::new(usize::MAX, usize::MAX),
    ] {
        let value = json(
            &Diagnostic::new(K::TypeMismatch, span, "m").with_related(span, "n"),
            "é",
        );
        assert!(value.contains("\"primary\":null"));
        assert!(value.contains("\"location\":null"));
    }
    let value = json(&Diagnostic::new(K::TypeMismatch, Span::new(2, 2), "m"), "é");
    assert!(value.contains("\"span\":[2,2],\"start\":{\"line\":1,\"column\":2}"));
    assert!(
        json(&Diagnostic::new(K::TypeMismatch, Span::default(), "m"), "")
            .contains("\"span\":[0,0]")
    );
}
#[test]
fn json_escapes_paths_and_controls_without_including_source_excerpts() {
    let error = Diagnostic::new(K::TypeMismatch, Span::default(), "message");
    let path = format!(
        "café🌍\"\\{}",
        (0u8..=31).map(char::from).collect::<String>()
    );
    let mut out = Vec::new();
    error.write_json(&path, "source-secret", &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("café🌍\\\"\\\\\\u0000"));
    assert!(text.contains("\\u001b"));
    assert!(!text.contains("source-secret"));
    assert!(text
        .trim_end_matches('\n')
        .chars()
        .all(|ch| !ch.is_control()));
}
#[test]
fn module_primary_and_related_locations_keep_distinct_source_owners() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib;")
        .unwrap();
    loader
        .insert("lib.srp", "// é🌍\r\npub bind answer=42;")
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let file = &program.sources().files()[1];
    let base = file.range().start;
    let error = ModuleDiagnostic {
        diagnostic: Diagnostic::new(K::TypeMismatch, Span::new(base + 11, base + 14), "primary")
            .with_related(Span::new(0, 6), "related"),
        sources: program.sources().clone(),
    };
    let mut out = Vec::new();
    error.write_json(&mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("\"path\":\"lib.srp\",\"span\":[11,14]"),
        "{text}"
    );
    assert!(text.contains("\"path\":\"main.srp\",\"span\":[0,6]"));
    let invalid = ModuleDiagnostic {
        diagnostic: Diagnostic::new(K::TypeMismatch, Span::new(0, base + 1), "bad range"),
        sources: program.sources().clone(),
    };
    let mut out = Vec::new();
    invalid.write_json(&mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("\"primary\":null"));
}
#[test]
fn missing_modules_have_no_invented_source_location() {
    let error = modules::compile_with_loader(
        "missing.srp",
        &MemorySourceLoader::new(),
        ModuleLimits::default(),
    )
    .unwrap_err();
    let mut out = Vec::new();
    error.write_json(&mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("\"primary\":null"));
}
#[test]
fn non_source_errors_use_null_codes_and_the_same_versioned_envelope() {
    let mut out = Vec::new();
    scarp::diagnostics::write_json_message("usage\nerror", Some("help"), &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("{\"schema_version\":1,\"severity\":\"error\",\"code\":null,"));
    assert!(text.ends_with("\"primary\":null,\"related\":[],\"help\":\"help\"}\n"));
}
#[test]
fn writer_failures_preserve_io_error_kind_after_partial_writes() {
    struct Limited(usize);
    impl std::io::Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            let n = self.0.min(bytes.len());
            self.0 -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error = Diagnostic::new(K::TypeMismatch, Span::default(), "message");
    assert_eq!(
        error
            .write_json("file", "", &mut Limited(10))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        scarp::diagnostics::write_json_message("message", None, &mut Limited(0))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
}
#[test]
fn diagnostic_json_is_deterministic_across_concurrent_readers() {
    let error = std::sync::Arc::new(Diagnostic::new(K::TypeMismatch, Span::new(0, 2), "message"));
    let expected = json(&error, "é");
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let error = error.clone();
            std::thread::spawn(move || json(&error, "é"))
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), expected);
    }
}

#[test]
fn intentional_type_error_example_has_a_verified_json_snapshot() {
    let source = include_str!("../examples/diagnostics/type_error.srp");
    let error = scarp::compile(source).unwrap_err();
    let mut out = Vec::new();
    error
        .write_json("type_error.srp", source, &mut out)
        .unwrap();
    assert_eq!(
        out,
        include_bytes!("../examples/diagnostics/type_error.diagnostic.json")
    );
}
