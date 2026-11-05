use scarp::modules::{self, symbols::IndexLimits, MemorySourceLoader, ModuleLimits};
fn main() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "symbol_navigation.srp",
            include_str!("symbol_navigation.srp"),
        )
        .unwrap();
    loader
        .insert(
            "modules/navigation_values.srp",
            include_str!("modules/navigation_values.srp"),
        )
        .unwrap();
    let compiled = modules::compile_indexed_with_loader(
        "symbol_navigation.srp",
        &loader,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap();
    let index = compiled.index();
    for reference in index.references() {
        assert_eq!(
            index
                .goto_definition(reference.location.source, reference.location.span.start)
                .unwrap()
                .id,
            reference.target
        );
        assert!(index
            .find_references(reference.target)
            .any(|r| r == reference));
    }
    match std::env::args().nth(1).as_deref() {
        Some("--symbols-json") => index
            .write_symbols_json(&mut std::io::stdout().lock())
            .unwrap(),
        Some("--references-json") => index
            .write_references_json(&mut std::io::stdout().lock())
            .unwrap(),
        _ => println!(
            "symbols: {}, references: {}, name bytes: {}",
            index.usage().symbols,
            index.usage().references,
            index.usage().name_bytes
        ),
    }
}
