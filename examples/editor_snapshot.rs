//! Opt-in host integration for version-guarded Scarp symbol navigation.
//! Run with: cargo run --locked --offline --example editor_snapshot

use scarp::modules::{
    self,
    symbols::{
        editor::{EditorLimits, EditorQueryError, EditorSnapshot, Utf16Position},
        IndexLimits,
    },
    MemorySourceLoader, ModuleLimits,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "import \"lib.srp\" as m;\nprint(m.twice(n: 5));\n";
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", source)?;
    loader.insert(
        "lib.srp",
        "pub pure funcra twice(n: integer)->integer{return n*2;}",
    )?;

    let compiled = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        IndexLimits::default(),
    )?;
    // Revisions are supplied by the editor in the compiler's SourceId order.
    let versions = [3, 8];
    let editor = EditorSnapshot::new(compiled.index(), &versions, EditorLimits::default())?;
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

    // A changed import rejects even a lookup in the unchanged entry file.
    assert!(matches!(
        editor.goto_definition(&[3, 9], 0, position),
        Err(EditorQueryError::StaleVersion { source: 1, .. })
    ));
    println!("versioned editor navigation passed");
    Ok(())
}
