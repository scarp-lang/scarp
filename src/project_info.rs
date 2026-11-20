//! Read-only output of validated local project metadata.
//!
//! Keep this schema independent of debug output and avoid adding third-party
//! serializers to Scarp's zero-dependency compiler.

use std::io::{self, Write};

use super::project::ProjectInfo;

pub(crate) fn quote_json(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            ch if ch.is_control() => {
                quoted.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

/// Stable schema 1 for tooling, or a readable summary for interactive use.
/// Output includes only validated manifest fields and contained entry labels,
/// never source code, absolute host paths or an inferred dependency graph.
pub fn write(info: &ProjectInfo, stdout: &mut impl Write, json: bool) -> io::Result<()> {
    if json {
        let version = info
            .version
            .as_deref()
            .map(quote_json)
            .unwrap_or_else(|| "null".to_owned());
        writeln!(
            stdout,
            "{{\"schema_version\":1,\"name\":{},\"version\":{version},\"type\":{},\"entry\":{},\"manifest\":{},\"format\":{}}}",
            quote_json(&info.name),
            quote_json(&info.kind),
            quote_json(&info.entry),
            quote_json(info.manifest_filename),
            quote_json(info.format),
        )
    } else {
        writeln!(stdout, "Name: {}", info.name)?;
        writeln!(
            stdout,
            "Version: {}",
            info.version.as_deref().unwrap_or("unspecified")
        )?;
        writeln!(stdout, "Type: {}", info.kind)?;
        writeln!(
            stdout,
            "Manifest: {} ({})",
            info.manifest_filename, info.format
        )?;
        writeln!(stdout, "Entry: {}", info.entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example() -> ProjectInfo {
        ProjectInfo {
            root: PathBuf::from("ignored"),
            name: "unicode हिंदी \"quoted\"".into(),
            version: Some("1.2.3-beta.1".into()),
            kind: "application".into(),
            entry: "src/main.srp".into(),
            manifest_filename: "scarp.project",
            format: "v2",
            entry_path: PathBuf::from("ignored"),
        }
    }

    #[test]
    fn preserves_unicode_and_escapes_json_special_characters() {
        let info = example();
        let mut output = Vec::new();
        write(&info, &mut output, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("\"name\":\"unicode हिंदी \\\"quoted\\\"\""));
        assert!(text.starts_with("{\"schema_version\":1,"));
        assert!(text.ends_with("}\n"));
        assert_eq!(quote_json("a\nb\t\r\u{0001}"), "\"a\\nb\\t\\r\\u0001\"");
    }

    #[test]
    fn legacy_versions_are_null_in_json_and_unspecified_in_text() {
        let mut info = example();
        info.version = None;
        info.format = "legacy-v1";
        info.manifest_filename = "Scarp.project";
        let mut json = Vec::new();
        write(&info, &mut json, true).unwrap();
        assert!(String::from_utf8(json)
            .unwrap()
            .contains("\"version\":null"));
        let mut text = Vec::new();
        write(&info, &mut text, false).unwrap();
        assert!(String::from_utf8(text)
            .unwrap()
            .contains("Version: unspecified\n"));
    }
}
