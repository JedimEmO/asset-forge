//! One log file per job: the child's stdout and stderr interleaved as they
//! arrived, appended line by line, read back by byte offset.
//!
//! The cap is 8 MB with the **head kept and the middle elided once**. A tail
//! cap would throw away the line that says which backend refused and why,
//! which is the line every refusal is read for; a hard stop would throw away
//! the end of a run that finally worked. Eliding once, loudly, in the middle
//! keeps both ends and says in the file that it happened.

use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use crate::ServeError;
use crate::job::LogChunk;

/// The cap: 8 MB per job.
pub(crate) const LOG_CAP_BYTES: u64 = 8 * 1024 * 1024;

/// How much of a capped log is kept as the head.
const HEAD_BYTES: u64 = LOG_CAP_BYTES / 2;

/// The line written where the middle was.
const ELISION: &str = "\n… the middle of this log was elided once at the 8 MB cap; the head above and \
     everything after this line are the child's own …\n";

/// An open log, appended to by the executor.
#[derive(Debug)]
pub(crate) struct LogSink {
    path: PathBuf,
    file: std::fs::File,
    written: u64,
    elided: bool,
}

impl LogSink {
    /// Open (and truncate) a job's log.
    pub(crate) fn create(path: &Path) -> Result<Self, ServeError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ServeError::io(parent, &e))?;
        }
        let file = std::fs::File::create(path).map_err(|e| ServeError::io(path, &e))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            written: 0,
            elided: false,
        })
    }

    /// Append one line, with its newline.
    pub(crate) fn line(&mut self, text: &str) {
        if self.written >= LOG_CAP_BYTES {
            if self.elided {
                return;
            }
            self.elide();
        }
        let _ = self.file.write_all(text.as_bytes());
        let _ = self.file.write_all(b"\n");
        let _ = self.file.flush();
        self.written += text.len() as u64 + 1;
    }

    /// Cut the file back to its head and say so, once.
    fn elide(&mut self) {
        self.elided = true;
        let head = std::fs::File::open(&self.path)
            .and_then(|mut file| {
                let mut buffer = vec![0_u8; usize::try_from(HEAD_BYTES).unwrap_or(usize::MAX)];
                let read = file.read(&mut buffer)?;
                buffer.truncate(read);
                Ok(buffer)
            })
            .unwrap_or_default();
        if let Ok(mut file) = std::fs::File::create(&self.path) {
            let _ = file.write_all(&head);
            let _ = file.write_all(ELISION.as_bytes());
            let _ = file.flush();
            self.written = head.len() as u64 + ELISION.len() as u64;
            self.file = file;
        }
    }
}

/// Read a job's log from a byte offset.
///
/// `done` is the caller's to fill in — this module knows about bytes, not
/// about whether a job is over.
pub(crate) fn read_from(path: &Path, from: u64) -> Result<LogChunk, ServeError> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LogChunk {
                text: String::new(),
                next: 0,
                done: false,
            });
        }
        Err(err) => return Err(ServeError::io(path, &err)),
    };
    let size = file.metadata().map_err(|e| ServeError::io(path, &e))?.len();
    // A log that was elided is shorter than the offset a follower holds;
    // starting again from the top is the honest answer to "the file you
    // were reading is not the file that is there".
    let from = if from > size { 0 } else { from };
    file.seek(SeekFrom::Start(from))
        .map_err(|e| ServeError::io(path, &e))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| ServeError::io(path, &e))?;
    let next = from + bytes.len() as u64;
    Ok(LogChunk {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        next,
        done: false,
    })
}

/// The last `count` lines of a log, for a status frame.
pub(crate) fn tail(path: &Path, count: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_picks_up_where_it_left_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("j.log");
        let mut sink = LogSink::create(&path).expect("create");
        sink.line("first");
        let chunk = read_from(&path, 0).expect("read");
        assert_eq!(chunk.text, "first\n");
        assert_eq!(chunk.next, 6);
        sink.line("second");
        let chunk = read_from(&path, chunk.next).expect("read");
        assert_eq!(chunk.text, "second\n");
        assert_eq!(tail(&path, 1), vec![String::from("second")]);
    }

    #[test]
    fn the_cap_keeps_the_head_and_elides_the_middle_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("j.log");
        let mut sink = LogSink::create(&path).expect("create");
        sink.line("the backend refused: no such voice");
        let filler = "x".repeat(4096);
        for _ in 0..2200 {
            sink.line(&filler);
        }
        sink.line("the last thing that happened");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.starts_with("the backend refused: no such voice"),
            "the head is what a refusal is read for"
        );
        assert!(text.contains("elided once"), "the elision says so");
        assert!(text.ends_with("the last thing that happened\n"));
        assert_eq!(
            text.matches("elided once").count(),
            1,
            "the middle goes once, not once per line"
        );
    }
}
