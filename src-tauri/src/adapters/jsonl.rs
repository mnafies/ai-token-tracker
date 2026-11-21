use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

/// Reads complete (newline-terminated) lines starting at `offset`.
/// Returns the lines (without the trailing newline) and the new offset, which only moves
/// past complete lines; a trailing line without a newline is left for the next read.
pub fn read_complete_lines(path: &Path, offset: u64) -> std::io::Result<(Vec<String>, u64)> {
    let mut r = BufReader::new(File::open(path)?);
    r.seek(SeekFrom::Start(offset))?;
    let mut lines = Vec::new();
    let mut end = offset;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = r.read_until(b'\n', &mut buf)?;
        if buf.pop() != Some(b'\n') {
            break; // EOF, possibly after a partial line
        }
        end += n as u64;
        lines.push(
            String::from_utf8_lossy(&buf)
                .trim_end_matches('\r')
                .to_string(),
        );
    }
    Ok((lines, end))
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

    #[test]
    fn read_complete_lines_streams_a_large_file_from_an_offset() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.jsonl");
        let line = format!("{{\"pad\":\"{}\"}}", "x".repeat(200));
        let mut body = String::new();
        for i in 0..20_000 {
            body.push_str(&format!("{line}{i}\r\n"));
        }
        body.push_str("partial-without-newline");
        std::fs::write(&p, &body).unwrap();
        let full = (body.len() - "partial-without-newline".len()) as u64;

        let (lines, off) = read_complete_lines(&p, 0).unwrap();
        assert_eq!(lines.len(), 20_000);
        assert_eq!(lines[19_999], format!("{line}19999"));
        assert_eq!(off, full);

        let first = (line.len() + 3) as u64; // "<line>0\r\n"
        let (lines, off) = read_complete_lines(&p, first).unwrap();
        assert_eq!(lines.len(), 19_999);
        assert_eq!(lines[0], format!("{line}1"));
        assert_eq!(off, full);

        let (lines, off) = read_complete_lines(&p, full).unwrap();
        assert!(lines.is_empty());
        assert_eq!(off, full);
    }
}
