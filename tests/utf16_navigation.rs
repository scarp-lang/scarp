use scarp::modules::{
    self,
    symbols::{
        editor::{byte_to_utf16, utf16_to_byte, Utf16Position as P, Utf16Range},
        IndexLimits,
    },
    MemorySourceLoader, ModuleLimits,
};

#[test]
fn mixed_line_endings_bom_and_supplementary_scalars_round_trip_strictly() {
    let source = "\u{feff}a😀b\r\nλ\rfin\n";
    let crlf = source.find("\r\n").unwrap();
    let lone_cr = source.find("\rfin").unwrap();
    let emoji = source.find('😀').unwrap();

    assert_eq!(byte_to_utf16(source, 0), Some(P::new(0, 0)));
    assert_eq!(byte_to_utf16(source, emoji), Some(P::new(0, 2)));
    assert_eq!(byte_to_utf16(source, source.find('b').unwrap()), Some(P::new(0, 4)));
    assert_eq!(byte_to_utf16(source, crlf), Some(P::new(0, 5)));
    assert_eq!(byte_to_utf16(source, crlf + 1), None);
    assert_eq!(byte_to_utf16(source, crlf + 2), Some(P::new(1, 0)));
    assert_eq!(byte_to_utf16(source, lone_cr), Some(P::new(1, 1)));
    assert_eq!(byte_to_utf16(source, lone_cr + 1), Some(P::new(2, 0)));
    assert_eq!(byte_to_utf16(source, source.len()), Some(P::new(3, 0)));

    assert_eq!(utf16_to_byte(source, P::new(0, 2)), Some(emoji));
    assert_eq!(utf16_to_byte(source, P::new(0, 3)), None); // split surrogate pair
    assert_eq!(utf16_to_byte(source, P::new(0, 5)), Some(crlf));
    assert_eq!(utf16_to_byte(source, P::new(0, 6)), None); // beyond line end
    assert_eq!(utf16_to_byte(source, P::new(1, 1)), Some(lone_cr));
    assert_eq!(utf16_to_byte(source, P::new(3, 0)), Some(source.len()));
    assert_eq!(utf16_to_byte(source, P::new(3, 1)), None);
    assert_eq!(utf16_to_byte(source, P::new(4, 0)), None);
    assert_eq!(utf16_to_byte(source, P::new(u32::MAX, 0)), None);

    for byte in 0..=source.len() {
        if let Some(position) = byte_to_utf16(source, byte) {
            assert_eq!(utf16_to_byte(source, position), Some(byte), "at {byte}");
        }
    }
}

#[test]
fn invalid_offsets_and_empty_lines_do_not_get_clamped() {
    assert_eq!(byte_to_utf16("é", 1), None);
    assert_eq!(byte_to_utf16("a", 2), None);
    assert_eq!(utf16_to_byte("a", P::new(0, 2)), None);
    assert_eq!(utf16_to_byte("a", P::new(1, 0)), None);
    assert_eq!(utf16_to_byte("", P::new(0, 0)), Some(0));
    assert_eq!(byte_to_utf16("", 0), Some(P::new(0, 0)));
    assert_eq!(utf16_to_byte("\r\n", P::new(1, 0)), Some(2));
    assert_eq!(utf16_to_byte("\r\n", P::new(2, 0)), None);
    assert_eq!(byte_to_utf16("\r\n", 1), None);
    assert_eq!(byte_to_utf16("\r\n", 2), Some(P::new(1, 0)));
    assert_eq!(utf16_to_byte("α💡", P::new(0, 1)), Some("α".len()));
    assert_eq!(utf16_to_byte("α💡", P::new(0, 2)), None);
    assert_eq!(utf16_to_byte("α💡", P::new(0, 3)), Some("α💡".len()));
}

#[test]
fn indexed_navigation_uses_utf16_positions_without_changing_source_spans() {
    let source = "bind val = 1;\r\nprint(\"😀\", val);\n";
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", source).unwrap();
    let compiled = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap();
    let index = compiled.index();
    let reference_byte = source.rfind("val").unwrap();
    assert_eq!(index.utf16_position(0, reference_byte), Some(P::new(1, 12)));

    let definition = index.goto_definition_utf16(0, P::new(1, 12)).unwrap();
    assert_eq!(definition.name.as_ref(), "val");
    let definition_location = definition.location.unwrap();
    assert_eq!(definition_location.span.start, source.find("val").unwrap());
    assert_eq!(
        index.utf16_range(definition_location),
        Some(Utf16Range {
            start: P::new(0, 5),
            end: P::new(0, 8),
        })
    );
    let references: Vec<_> = index.find_references(definition.id).collect();
    assert_eq!(references.len(), 1);
    assert_eq!(
        index.utf16_range(references[0].location),
        Some(Utf16Range {
            start: P::new(1, 12),
            end: P::new(1, 15),
        })
    );
    assert!(index.goto_definition_utf16(0, P::new(1, 8)).is_none());
    assert!(index.goto_definition_utf16(0, P::new(1, 11)).is_none());
    assert!(index.goto_definition_utf16(1, P::new(0, 5)).is_none());
    assert!(index.utf16_position(1, 0).is_none());
    let mut invalid_location = definition_location;
    invalid_location.span.end = source.len() + 1;
    assert!(index.utf16_range(invalid_location).is_none());
}
