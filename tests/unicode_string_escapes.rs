use scarp::{
    diagnostics::DiagnosticKind as Kind,
    lexer::{self, TokenKind},
    modules::{self, MemorySourceLoader, ModuleLimits},
    ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    out
}

#[test]
fn decodes_ascii_hindi_emoji_and_uncombined_scalars() {
    let source = r#"
        print("A=\u{41}, a=\u{000061}, \u{9}!");
        print("\u{0928}\u{092e}\u{0938}\u{094d}\u{0924}\u{0947} \u{1F44B}");
        bind text = "e\u{301}";
        print(text, text_len(text), text_bytes(text), text == "\u{e9}");
    "#;
    assert_eq!(
        run(source),
        "A=A, a=a, \t!\nनमस्ते 👋\ne\u{301} 2 3 false\n".as_bytes()
    );
}

#[test]
fn accepts_the_first_and_last_scalars_and_both_sides_of_surrogates() {
    let source = r#"write("\u{0}\u{7f}\u{80}\u{D7FF}\u{E000}\u{FFFF}\u{10000}\u{10FFFF}");"#;
    assert_eq!(
        run(source),
        "\0\u{7f}\u{80}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}".as_bytes()
    );
}

#[test]
fn legacy_escapes_raw_unicode_and_escaped_backslashes_are_preserved() {
    let source = r#"write("raw 🌍 \\ \" \n\t\r \\u{1F44B} \u{5C}\u{22}\u{a}");"#;
    assert_eq!(
        run(source),
        "raw 🌍 \\ \" \n\t\r \\u{1F44B} \\\"\n".as_bytes()
    );
}

#[test]
fn tokenizer_preserves_crlf_and_original_utf8_byte_spans() {
    let literal = r#""🪷\u{1f44b}\u{000041}""#;
    let source = format!("// हिन्दी 🌍\r\nprint({literal});\r\n");
    let tokens = lexer::lex(&source).unwrap();
    let token = tokens
        .iter()
        .find(|token| matches!(token.kind, TokenKind::String(_)))
        .unwrap();
    assert_eq!(token.kind, TokenKind::String("🪷👋A".into()));
    assert_eq!(&source[token.span.start..token.span.end], literal);
    assert_eq!(run(&source), "🪷👋A\n".as_bytes());
}

#[test]
fn invalid_escapes_use_e0003_and_never_split_utf8_characters() {
    for (escape, highlighted, message) in [
        (r"\u", r"\u", "expected '{'"),
        (r"\u0041", r"\u", "expected '{'"),
        (r"\u{}", r"\u{}", "at least one"),
        (r"\u{0000041}", r"\u{0000041", "six hex digits"),
        (r"\u{0x41}", r"\u{0x", "ASCII hexadecimal"),
        (r"\u{-1}", r"\u{-", "ASCII hexadecimal"),
        (r"\u{4_1}", r"\u{4_", "ASCII hexadecimal"),
        (r"\u{４1}", r"\u{４", "ASCII hexadecimal"),
        (r"\u{1🌍}", r"\u{1🌍", "ASCII hexadecimal"),
        (r"\u{41 }", r"\u{41 ", "ASCII hexadecimal"),
        (r"\u{41", r"\u{41", "expected '}'"),
        (r"\u{D800}", r"\u{D800}", "scalar value range"),
        (r"\u{DFFF}", r"\u{DFFF}", "scalar value range"),
        (r"\u{110000}", r"\u{110000}", "scalar value range"),
        (r"\u{FFFFFF}", r"\u{FFFFFF}", "scalar value range"),
        (r"\U{41}", r"\U", "unsupported escape"),
        (r"\🌍", r"\🌍", "unsupported escape"),
    ] {
        let source = format!("// हिन्दी 🌍\r\nprint(\"{escape}\");");
        let error = scarp::compile(&source).unwrap_err();
        assert_eq!(error.kind, Kind::InvalidEscape, "{escape}: {error:?}");
        assert_eq!(error.kind.code(), "E0003");
        assert_eq!(&source[error.span.start..error.span.end], highlighted);
        assert!(error.message.contains(message), "{escape}: {error:?}");
    }
}

#[test]
fn unfinished_escape_excludes_physical_line_breaks_and_quote_boundaries() {
    let prefix = r#"print("🌍\u{41"#;
    for suffix in ["", "\"", "\r\n", "\n", "\r"] {
        let source = format!("{prefix}{suffix}");
        let error = scarp::parse(&source).unwrap_err();
        assert_eq!(error.kind, Kind::InvalidEscape);
        assert_eq!(&source[error.span.start..error.span.end], r"\u{41");
        assert_eq!(error.span.end, prefix.len());
    }
    for source in [r#"print("raw"#, "print(\"raw\\\r\n", r#"print("\u{41}"#] {
        assert_eq!(
            scarp::parse(source).unwrap_err().kind,
            Kind::UnterminatedString
        );
    }
}

#[test]
fn string_allocation_budget_counts_decoded_utf8_bytes() {
    let source = r#"bind text = "\u{41}\u{1f44b}"; write(text);"#;
    let mut options = ExecutionOptions {
        max_text_bytes: 5,
        ..Default::default()
    };
    let mut out = Vec::new();
    scarp::run_with_options(source, &mut out, options).unwrap();
    assert_eq!(out, "A👋".as_bytes());

    options.max_text_bytes = 4;
    out.clear();
    let RunError::Diagnostic(error) =
        scarp::run_with_options(source, &mut out, options).unwrap_err()
    else {
        panic!("expected a text allocation diagnostic");
    };
    assert_eq!(error.kind, Kind::TextConstruction);
    assert_eq!(
        &source[error.span.start..error.span.end],
        r#""\u{41}\u{1f44b}""#
    );
    assert!(out.is_empty());
}

#[test]
fn decoded_import_paths_and_module_constants_share_the_string_lexer() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            r#"import "l\u{69}b.srp" as lib; print(lib.greeting);"#,
        )
        .unwrap();
    loader
        .insert("lib.srp", r#"pub bind greeting = "नमस्ते \u{1F44B}";"#)
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, "नमस्ते 👋\n".as_bytes());
    assert!(err.is_empty());
}
