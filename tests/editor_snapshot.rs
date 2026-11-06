use scarp::modules::{
    self,
    symbols::{
        editor::{
            byte_to_utf16, utf16_to_byte, EditorBuildError, EditorLimits, EditorQueryError,
            EditorSnapshot, Utf16Position as P, Utf16Range,
        },
        IndexLimits, IndexedProgram,
    },
    MemorySourceLoader, ModuleLimits,
};

fn compile(files: &[(&str, &str)]) -> IndexedProgram {
    let mut loader = MemorySourceLoader::new();
    for &(name, source) in files {
        loader.insert(name, source).unwrap();
    }
    modules::compile_indexed_with_loader(
        files[0].0,
        &loader,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap()
}

#[test]
fn every_byte_and_utf16_boundary_matches_the_original_strict_converter() {
    let source = "\u{feff}bind alpha = 1;\r\nprint(\"😀\", alpha);\rprint(\"λ\", alpha);\n";
    let compiled = compile(&[("main.srp", source)]);
    let editor = EditorSnapshot::new(compiled.index(), &[7], EditorLimits::default()).unwrap();
    assert_eq!(editor.line_count(0), Some(4));
    assert_eq!(editor.total_lines(), 4);
    assert_eq!(editor.line_count(1), None);

    for byte in 0..=source.len() + 1 {
        let expected = byte_to_utf16(source, byte);
        let actual = editor.byte_to_utf16(&[7], 0, byte);
        match expected {
            Some(position) => {
                assert_eq!(actual, Ok(position), "byte {byte}");
                assert_eq!(
                    editor.utf16_to_byte(&[7], 0, position),
                    Ok(byte),
                    "round trip at {byte}"
                );
            }
            None => assert_eq!(
                actual,
                Err(EditorQueryError::InvalidCoordinate),
                "invalid byte {byte}"
            ),
        }
    }

    for line in 0..=5 {
        for character in 0..=36 {
            let position = P::new(line, character);
            let expected = utf16_to_byte(source, position);
            let actual = editor.utf16_to_byte(&[7], 0, position);
            match expected {
                Some(byte) => assert_eq!(actual, Ok(byte), "{position:?}"),
                None => assert_eq!(
                    actual,
                    Err(EditorQueryError::InvalidCoordinate),
                    "{position:?}"
                ),
            }
        }
    }

    assert_eq!(
        editor.utf16_to_byte(&[7], 0, P::new(u32::MAX, u32::MAX)),
        Err(EditorQueryError::InvalidCoordinate)
    );
    let crlf_middle = source.find("\r\n").unwrap() + 1;
    assert_eq!(
        editor.byte_to_utf16(&[7], 0, crlf_middle),
        Err(EditorQueryError::InvalidCoordinate)
    );
}

#[test]
fn stale_import_revision_invalidates_entry_source_and_cross_file_ranges() {
    let main = "import \"lib.srp\" as l;\nprint(l.twice(n: 2));\n";
    let library = "pub pure funcra twice(n: integer)->integer{return n*2;}";
    let compiled = compile(&[("main.srp", main), ("lib.srp", library)]);
    let original = [12, 45];
    let editor = EditorSnapshot::new(compiled.index(), &original, EditorLimits::default()).unwrap();
    let reference = compiled
        .index()
        .references()
        .iter()
        .find(|r| {
            r.location.source == 0 && &main[r.location.span.start..r.location.span.end] == "twice"
        })
        .unwrap();
    let position = editor
        .byte_to_utf16(&original, 0, reference.location.span.start)
        .unwrap();
    let definition = editor
        .goto_definition(&original, 0, position)
        .unwrap()
        .unwrap();
    assert_eq!(definition.name.as_ref(), "twice");
    assert_eq!(definition.location.unwrap().source, 1);

    let ranges: Vec<_> = editor
        .find_references(&original, definition.id)
        .unwrap()
        .map(|reference| editor.utf16_range(&original, reference.location).unwrap())
        .collect();
    assert_eq!(ranges.len(), 1);
    assert_eq!(
        ranges[0],
        Utf16Range {
            start: position,
            end: P::new(position.line, position.character + 5),
        }
    );

    let stale = [12, 46];
    let err = EditorQueryError::StaleVersion {
        source: 1,
        indexed: 45,
        current: 46,
    };
    assert_eq!(editor.ensure_current(&stale), Err(err));
    assert_eq!(editor.goto_definition(&stale, 0, position), Err(err));
    assert_eq!(
        editor.utf16_range(&stale, definition.location.unwrap()),
        Err(err)
    );
    assert_eq!(editor.byte_to_utf16(&stale, 0, 0), Err(err));
    assert_eq!(
        editor.find_references(&stale, definition.id).err(),
        Some(err)
    );
    assert_eq!(
        editor.goto_definition(&[12], 0, position),
        Err(EditorQueryError::VersionCount {
            expected: 2,
            actual: 1,
        })
    );

    // A new compilation and a matching revision list is required after edits.
    let changed = compile(&[
        ("main.srp", main),
        (
            "lib.srp",
            "pub pure funcra twice(n: integer)->integer{return n+n;}",
        ),
    ]);
    let rebuilt = EditorSnapshot::new(changed.index(), &stale, EditorLimits::default()).unwrap();
    assert_eq!(
        rebuilt
            .goto_definition(&stale, 0, position)
            .unwrap()
            .unwrap()
            .name
            .as_ref(),
        "twice"
    );
}

#[test]
fn unknown_sources_and_invalid_offsets_do_not_silently_clamp() {
    let source = "bind x = 1;\r\nprint(x);";
    let compiled = compile(&[("main.srp", source)]);
    let version = [0];
    let editor = EditorSnapshot::new(compiled.index(), &version, EditorLimits::default()).unwrap();

    assert_eq!(editor.goto_definition(&version, 0, P::new(0, 0)), Ok(None));
    assert_eq!(
        editor.goto_definition(&version, 50, P::new(0, 0)),
        Err(EditorQueryError::UnknownSource)
    );
    assert_eq!(
        editor.utf16_to_byte(&version, 0, P::new(1, 999)),
        Err(EditorQueryError::InvalidCoordinate)
    );
    assert_eq!(
        editor.byte_to_utf16(&version, 0, source.len() + 1),
        Err(EditorQueryError::InvalidCoordinate)
    );
    assert_eq!(
        editor.byte_to_utf16(&version, 7, 0),
        Err(EditorQueryError::UnknownSource)
    );
    let mut invalid = compiled.index().symbols()[0].location.unwrap();
    invalid.source = 3;
    assert_eq!(
        editor.utf16_range(&version, invalid),
        Err(EditorQueryError::UnknownSource)
    );
    let mut invalid = compiled.index().symbols()[0].location.unwrap();
    invalid.span.start = 100;
    invalid.span.end = 1;
    assert_eq!(
        editor.utf16_range(&version, invalid),
        Err(EditorQueryError::InvalidCoordinate)
    );
}

#[test]
fn total_line_budget_and_revision_count_are_checked_before_retaining_a_snapshot() {
    let compiled = compile(&[
        ("main.srp", "import \"lib.srp\" as l;\nprint(l.f());\n"),
        ("lib.srp", "pub pure funcra f()->integer{return 3;}"),
    ]);
    let versions = [4, 5];
    assert!(matches!(
        EditorSnapshot::new(compiled.index(), &versions[..1], EditorLimits::default()),
        Err(EditorBuildError::VersionCount {
            expected: 2,
            actual: 1,
        })
    ));
    for max_lines in [0, 1, 2, 3] {
        assert!(matches!(
            EditorSnapshot::new(compiled.index(), &versions, EditorLimits { max_lines }),
            Err(EditorBuildError::LineLimit)
        ));
    }
    let editor =
        EditorSnapshot::new(compiled.index(), &versions, EditorLimits { max_lines: 4 }).unwrap();
    assert_eq!(editor.total_lines(), 4);
    assert_eq!(editor.line_count(0), Some(3));
    assert_eq!(editor.line_count(1), Some(1));
}

#[test]
fn clone_is_read_only_and_safe_for_separate_host_threads() {
    let compiled = compile(&[("main.srp", "bind answer = 42;\nprint(answer);\n")]);
    let source = compiled.index().sources().source(0).unwrap();
    let version = [92];
    let editor = EditorSnapshot::new(compiled.index(), &version, EditorLimits::default()).unwrap();
    let position = utf16_to_byte(source, P::new(1, 6)).unwrap();
    assert_eq!(
        editor.byte_to_utf16(&version, 0, position),
        Ok(P::new(1, 6))
    );

    let shared = editor.clone();
    let handle = std::thread::spawn(move || {
        let name = shared
            .goto_definition(&version, 0, P::new(1, 6))
            .unwrap()
            .unwrap()
            .name
            .clone();
        assert_eq!(name.as_ref(), "answer");
        assert_eq!(shared.line_count(0), Some(3));
    });
    handle.join().unwrap();
    assert!(editor.ensure_current(&version).is_ok());
}
