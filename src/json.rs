use std::io;

pub(crate) fn string(out: &mut impl io::Write, value: &str) -> io::Result<()> {
    out.write_all(b"\"")?;
    let mut start = 0;
    for (index, ch) in value.char_indices() {
        if !matches!(ch, '"' | '\\') && !ch.is_control() {
            continue;
        }
        out.write_all(&value.as_bytes()[start..index])?;
        match ch {
            '"' => out.write_all(b"\\\"")?,
            '\\' => out.write_all(b"\\\\")?,
            '\n' => out.write_all(b"\\n")?,
            '\r' => out.write_all(b"\\r")?,
            '\t' => out.write_all(b"\\t")?,
            _ => write!(out, "\\u{:04x}", u32::from(ch))?,
        }
        start = index + ch.len_utf8();
    }
    out.write_all(&value.as_bytes()[start..])?;
    out.write_all(b"\"")
}
