//! Assertions must retain exact source owners and UTF-8 spans when the
//! compiler reuses parsed syntax or lexed tokens from another source graph.
use scarp::{
    modules::{
        self,
        cache::{SyntaxCache, SyntaxCacheLimits, TokenCache, TokenCacheLimits},
        MemorySourceLoader, ModuleLimits, ModuleRunError,
    },
    ExecutionOptions,
};

fn source(prefix: &str) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            format!("{prefix}import \"lib.srp\" as lib;\nprint(lib.guarded(-1));\n"),
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra guarded(value: integer)->integer {\n\
             assert(value > 0, \"positive only\");\n\
             return value;\n\
             }\n",
        )
        .unwrap();
    loader
}

fn runtime_json(program: &modules::ModuleProgram) -> String {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap_err();
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    let diagnostic = match error {
        ModuleRunError::Diagnostic(diagnostic) => diagnostic,
        ModuleRunError::Output(other) => panic!("unexpected output error: {other}"),
    };
    let mut json = Vec::new();
    diagnostic.write_json(&mut json).unwrap();
    let text = String::from_utf8(json).unwrap();
    assert!(text.contains("\"code\":\"E0062\""), "{text}");
    assert!(text.contains("\"path\":\"lib.srp\""), "{text}");
    text
}

#[test]
fn source_syntax_cache_keeps_assertion_failure_spans_after_unicode_prefix_shift() {
    let before = source("");
    let after = source("// नमस्ते 🌍\r\n// more source text\r\n");
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let first = modules::resolve(
        modules::load_with_syntax_cache("main.srp", &before, ModuleLimits::default(), &mut syntax)
            .unwrap(),
    )
    .unwrap();
    let warm = modules::resolve(
        modules::load_with_syntax_cache("main.srp", &after, ModuleLimits::default(), &mut syntax)
            .unwrap(),
    )
    .unwrap();
    let cold = modules::compile_with_loader("main.srp", &after, ModuleLimits::default()).unwrap();

    assert!(syntax.stats().hits > 0, "{:?}", syntax.stats());
    assert_eq!(runtime_json(&warm), runtime_json(&cold));
    assert_ne!(
        first.sources().files()[1].range(),
        warm.sources().files()[1].range()
    );
}

#[test]
fn source_token_cache_keeps_assertion_failure_spans_after_unicode_prefix_shift() {
    let before = source("");
    let after = source("// नमस्ते 🌍\r\n// more source text\r\n");
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let _ = modules::resolve(
        modules::load_with_token_cache("main.srp", &before, ModuleLimits::default(), &mut tokens)
            .unwrap(),
    )
    .unwrap();
    let warm = modules::resolve(
        modules::load_with_token_cache("main.srp", &after, ModuleLimits::default(), &mut tokens)
            .unwrap(),
    )
    .unwrap();
    let cold = modules::compile_with_loader("main.srp", &after, ModuleLimits::default()).unwrap();

    assert!(tokens.stats().hits > 0, "{:?}", tokens.stats());
    assert_eq!(runtime_json(&warm), runtime_json(&cold));
}
