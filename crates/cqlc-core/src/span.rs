//! Source files and byte-offset spans.

/// Identifies a file in a [`SourceMap`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

/// A half-open byte range inside one source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    /// The file the span is in.
    pub file: FileId,
    /// Byte offset of the first byte.
    pub start: u32,
    /// Byte offset after the last byte.
    pub end: u32,
}

impl Span {
    /// Creates a span from byte offsets.
    pub fn new(file: FileId, start: usize, end: usize) -> Self {
        Span {
            file,
            start: start as u32,
            end: end as u32,
        }
    }

    /// Smallest span covering both `self` and `other` (same file assumed).
    pub fn to(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// The span as a byte range.
    pub fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// One source file with a line index.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// Name shown in diagnostics.
    pub name: String,
    /// File contents.
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// Creates a source file and indexes its lines.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        SourceFile {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// 1-based line and column (in characters) of a byte offset.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line];
        let col = self
            .text
            .get(start..offset)
            .map_or(offset - start, |s| s.chars().count());
        (line + 1, col + 1)
    }

    /// Text of a 1-based line without its line terminator.
    pub fn line_text(&self, line: usize) -> &str {
        let start = self.line_starts[line - 1];
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    /// Byte offset where a 1-based line starts.
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts[line - 1]
    }
}

/// All source files of a compilation.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Adds a file and returns its id.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        self.files.push(SourceFile::new(name, text));
        FileId(self.files.len() as u32 - 1)
    }

    /// Returns the file with the given id.
    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }
}
