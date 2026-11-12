//! Declarative, offline v2 project manifest parsing.
//!
//! This is deliberately not the Scarp source parser: manifests cannot run code,
//! interpolate values, open files, or access a package registry.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub entry: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Word(String),
    Text(String),
    Open,
    Close,
    Colon,
    End,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    offset: usize,
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn error(&self, offset: usize, message: &str) -> String {
        let prefix = &self.src[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("scarp.project:{line}:{column}: {message}")
    }

    fn next(&mut self) -> Result<Token, String> {
        let bytes = self.src.as_bytes();
        loop {
            if self.pos >= bytes.len() {
                return Ok(Token {
                    kind: Kind::End,
                    offset: self.pos,
                });
            }
            if bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if bytes[self.pos] == b'/' && bytes.get(self.pos + 1) == Some(&b'/') {
                while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
        let offset = self.pos;
        let kind = match bytes[self.pos] {
            b'{' => {
                self.pos += 1;
                Kind::Open
            }
            b'}' => {
                self.pos += 1;
                Kind::Close
            }
            b':' => {
                self.pos += 1;
                Kind::Colon
            }
            b'"' => {
                self.pos += 1;
                let mut value = String::new();
                loop {
                    let Some(ch) = self.src[self.pos..].chars().next() else {
                        return Err(self.error(offset, "unterminated quoted string"));
                    };
                    self.pos += ch.len_utf8();
                    match ch {
                        '"' => break,
                        '\\' => {
                            let Some(escaped) = self.src[self.pos..].chars().next() else {
                                return Err(self.error(offset, "unterminated string escape"));
                            };
                            self.pos += escaped.len_utf8();
                            match escaped {
                                '"' | '\\' => value.push(escaped),
                                _ => {
                                    return Err(self.error(
                                        self.pos - escaped.len_utf8(),
                                        "unsupported string escape",
                                    ))
                                }
                            }
                        }
                        '\n' | '\r' => {
                            return Err(self.error(offset, "newline inside a quoted string"))
                        }
                        _ => value.push(ch),
                    }
                }
                Kind::Text(value)
            }
            byte if byte.is_ascii_alphabetic() => {
                self.pos += 1;
                while self.pos < bytes.len()
                    && (bytes[self.pos].is_ascii_alphanumeric()
                        || matches!(bytes[self.pos], b'_' | b'-'))
                {
                    self.pos += 1;
                }
                Kind::Word(self.src[offset..self.pos].to_owned())
            }
            _ => return Err(self.error(offset, "unexpected character in manifest")),
        };
        Ok(Token { kind, offset })
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    token: Token,
}

impl Parser<'_> {
    fn bump(&mut self) -> Result<(), String> {
        self.token = self.lexer.next()?;
        Ok(())
    }

    fn expect(&mut self, kind: Kind) -> Result<(), String> {
        if self.token.kind != kind {
            return Err(self
                .lexer
                .error(self.token.offset, &format!("expected {kind:?}")));
        }
        self.bump()
    }

    fn quoted(&mut self) -> Result<String, String> {
        match &self.token.kind {
            Kind::Text(value) => {
                let value = value.clone();
                self.bump()?;
                Ok(value)
            }
            _ => Err(self
                .lexer
                .error(self.token.offset, "expected a quoted string")),
        }
    }
}

pub fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.trim() == value
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, ':' | '/' | '\\'))
}

fn valid_version(value: &str) -> bool {
    let (without_build, build) = match value.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (value, None),
    };
    let (core, pre) = match without_build.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (without_build, None),
    };
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return false;
    }
    fn identifiers(value: &str, pre: bool) -> bool {
        value.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && (!pre
                    || !part.bytes().all(|b| b.is_ascii_digit())
                    || part.len() == 1
                    || !part.starts_with('0'))
        })
    }
    pre.map_or(true, |v| identifiers(v, true)) && build.map_or(true, |v| identifiers(v, false))
}

pub fn valid_entry(value: &str) -> bool {
    value.len() <= 512
        && value.ends_with(".srp")
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | ':'))
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && !std::path::Path::new(value).is_absolute()
}

/// Parse a bounded UTF-8 manifest. Dependencies are recognized only as an
/// empty block until a reproducible local dependency specification exists.
pub fn parse(source: &str) -> Result<Manifest, String> {
    let mut parser = Parser {
        lexer: Lexer {
            src: source,
            pos: 0,
        },
        token: Token {
            kind: Kind::End,
            offset: 0,
        },
    };
    parser.bump()?;
    parser.expect(Kind::Word("project".into()))?;
    let name = parser.quoted()?;
    if !valid_name(&name) {
        return Err(parser.lexer.error(
            0,
            "project name must be 1..96 UTF-8 bytes without separators or controls",
        ));
    }
    parser.expect(Kind::Open)?;
    let (mut version, mut entry, mut kind) = (None, None, None);
    let mut dependencies = false;
    while parser.token.kind != Kind::Close {
        if parser.token.kind == Kind::End {
            return Err(parser
                .lexer
                .error(parser.token.offset, "missing closing project brace"));
        }
        let at = parser.token.offset;
        let key = match parser.token.kind.clone() {
            Kind::Word(key) => key,
            _ => return Err(parser.lexer.error(at, "expected a project field")),
        };
        parser.bump()?;
        if key == "dependencies" {
            if dependencies {
                return Err(parser.lexer.error(at, "duplicate dependencies block"));
            }
            dependencies = true;
            parser.expect(Kind::Open)?;
            if parser.token.kind != Kind::Close {
                return Err(parser.lexer.error(
                    parser.token.offset,
                    "dependency declarations are not implemented; only an empty block is supported",
                ));
            }
            parser.bump()?;
            continue;
        }
        parser.expect(Kind::Colon)?;
        let value = parser.quoted()?;
        let field = match key.as_str() {
            "version" => &mut version,
            "entry" => &mut entry,
            "type" => &mut kind,
            _ => {
                return Err(parser
                    .lexer
                    .error(at, &format!("unsupported project field '{key}'")))
            }
        };
        if field.replace(value).is_some() {
            return Err(parser
                .lexer
                .error(at, &format!("duplicate project field '{key}'")));
        }
    }
    parser.bump()?;
    parser.expect(Kind::End)?;
    let version =
        version.ok_or_else(|| parser.lexer.error(0, "missing required 'version' field"))?;
    let entry = entry.ok_or_else(|| parser.lexer.error(0, "missing required 'entry' field"))?;
    let kind = kind.ok_or_else(|| parser.lexer.error(0, "missing required 'type' field"))?;
    if !valid_version(&version) {
        return Err(parser
            .lexer
            .error(0, "version must be a valid semantic version"));
    }
    if !valid_entry(&entry) {
        return Err(parser.lexer.error(
            0,
            "entry must be a safe relative .srp path without traversal",
        ));
    }
    if kind != "application" {
        return Err(parser
            .lexer
            .error(0, "only project type 'application' is currently supported"));
    }
    Ok(Manifest {
        name,
        version,
        entry,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const GOOD: &str = r#"project "unicode नमस्ते" {
        version: "0.1.0-beta.2+build.4"
        entry: "src/main.srp"
        type: "application"
        dependencies {}
    }"#;

    #[test]
    fn parses_a_real_project_without_executing_it() {
        let value = parse(GOOD).unwrap();
        assert_eq!(value.name, "unicode नमस्ते");
        assert_eq!(value.version, "0.1.0-beta.2+build.4");
        assert_eq!(value.entry, "src/main.srp");
        assert_eq!(value.kind, "application");
    }

    #[test]
    fn accepts_comments_and_independent_field_order() {
        assert!(parse("project \"a\" { // header\n type: \"application\"\n entry: \"src/main.srp\"\n version: \"1.0.0\"\n }").is_ok());
    }

    #[test]
    fn rejects_duplicate_or_unknown_fields() {
        for value in [
            GOOD.replace(
                "type: \"application\"",
                "type: \"application\" type: \"application\"",
            ),
            GOOD.replace("dependencies {}", "dependencies {} dependencies {}"),
            GOOD.replace("dependencies {}", "garbage: \"ignored\""),
        ] {
            assert!(parse(&value).is_err(), "{value}");
        }
    }

    #[test]
    fn rejects_wrong_versions_and_missing_fields() {
        for version in [
            "0.01.0",
            "01.0.0",
            "1.0",
            "1.0.0-",
            "1.0.0+",
            "1.0.0-alpha..1",
            "1.0.0-01",
            "1.0.0-α",
            "1.0.0+foo+bar",
        ] {
            let bad = GOOD.replace("0.1.0-beta.2+build.4", version);
            assert!(parse(&bad).is_err(), "{version}");
        }
        assert!(parse(&GOOD.replace("type: \"application\"", "")).is_err());
    }

    #[test]
    fn rejects_unsafe_entries_and_executable_dependency_blocks() {
        for value in [
            "../outside.srp",
            "/root.srp",
            "src/./main.srp",
            "C:/tmp/main.srp",
            "src\\main.srp",
            "src//main.srp",
        ] {
            let bad = GOOD.replace("src/main.srp", value);
            assert!(parse(&bad).is_err(), "{value}");
        }
        assert!(parse(&GOOD.replace("dependencies {}", "dependencies { evil: \"run\" }")).is_err());
    }

    #[test]
    fn rejects_unterminated_input_and_reports_locations() {
        let error = parse("project \"app\" {\n version: \"0.1.0\"\n !").unwrap_err();
        assert!(error.contains("scarp.project:3:"), "{error}");
        assert!(parse("project \"app\" {").is_err());
        assert!(parse("").is_err());
        assert!(parse(&GOOD.replace("project \"unicode नमस्ते\"", "project \"a\\nb\"")).is_err());
    }

    #[test]
    fn accepts_valid_semver_and_rejects_invalid_pre_release_identifiers() {
        for v in [
            "0.0.0",
            "1.2.3",
            "1.2.3-alpha",
            "1.2.3-0",
            "1.2.3+001",
            "12.100.999-rc.1+git.abc",
        ] {
            assert!(valid_version(v), "{v}");
        }
        for v in ["1.2.3-01", "1.2.3-1.02", "1.2.3-", "1.2.3++x"] {
            assert!(!valid_version(v), "{v}");
        }
    }
}
