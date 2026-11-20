use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Reads complete (newline-terminated) lines starting at `offset`.
/// Returns the lines (without the trailing newline) and the new offset.
pub fn read_complete_lines(path: &Path, offset: u64) -> std::io::Result<(Vec<String>, u64)> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let end = match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => return Ok((Vec::new(), offset)),
    };
    let lines = buf[..end]
        .split(|&b| b == b'\n')
        .map(|l| String::from_utf8_lossy(l).trim_end_matches('\r').to_string())
        .collect::<Vec<_>>();
    // split yields a trailing empty piece after the final '\n'
    let mut lines = lines;
    lines.pop();
    Ok((lines, offset + end as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_complete_lines_stops_before_partial_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.jsonl");
        std::fs::write(&p, "a\nb\npar").unwrap();
        let (lines, off) = read_complete_lines(&p, 0).unwrap();
        assert_eq!(lines, vec!["a", "b"]);
        assert_eq!(off, 4);
    }
}
