//! Opt-in host integration for version-guarded Scarp symbol navigation.
//! Run with: cargo run --locked --offline --example editor_snapshot

use scarp::modules::{
    self,
    cache::{CacheStatus, EditorCacheLimits, EditorCompilationCache, EditorCompileLimits},
    symbols::editor::{EditorQueryError, Utf16Position},
    MemorySourceLoader, ModuleLimits,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "import \"lib.srp\" as m;\nprint(m.twice(n: 5));\n";
    let library = "pub pure funcra twice(n: integer)->integer{return n*2;}";
    let mut loader = MemorySourceLoader::new();
    loader.set_document("main.srp", source, 3)?;
    loader.set_document("lib.srp", library, 8)?;

    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let compile = |cache: &mut EditorCompilationCache, loader: &MemorySourceLoader| {
        let graph = modules::load("main.srp", loader, ModuleLimits::default())?;
        cache.resolve_with_loaded_revisions(graph, EditorCompileLimits::default(), None)
    };
    let compiled = compile(&mut cache, &loader)?;
    let editor = &compiled.snapshot;
    // Queries use the revisions in the returned program's SourceId order.
    let versions = [3, 8];
    let offset = source.find("twice").expect("example has a call");
    let position = editor.byte_to_utf16(&versions, 0, offset)?;
    let definition = editor
        .goto_definition(&versions, 0, position)?
        .expect("the call resolves to its declaration");
    let target = definition
        .location
        .expect("user function has a source span");
    assert_eq!(target.source, 1);
    assert_eq!(
        editor.utf16_range(&versions, target)?.start,
        Utf16Position::new(0, 16)
    );
    assert_eq!(editor.find_references(&versions, definition.id)?.count(), 1);

    // Advancing the import's revision invalidates queries against the old view.
    loader.set_document("lib.srp", library, 9)?;
    let refreshed = compile(&mut cache, &loader)?;
    assert_eq!(refreshed.status, CacheStatus::Hit);
    assert!(refreshed
        .snapshot
        .goto_definition(&[3, 9], 0, position)?
        .is_some());
    assert!(matches!(
        editor.goto_definition(&[3, 9], 0, position),
        Err(EditorQueryError::StaleVersion { source: 1, .. })
    ));
    println!("versioned editor navigation passed");
    Ok(())
}
