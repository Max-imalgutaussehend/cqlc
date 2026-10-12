//! Tiny indentation-aware text writer.

#[derive(Default)]
pub struct Emitter {
    out: String,
    indent: usize,
}

impl Emitter {
    pub fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if !text.is_empty() {
            for _ in 0..self.indent {
                self.out.push_str("    ");
            }
            self.out.push_str(text);
        }
        self.out.push('\n');
    }

    pub fn blank(&mut self) {
        self.out.push('\n');
    }

    /// Writes `header {` and indents until the matching [`Emitter::close`].
    pub fn open(&mut self, header: impl AsRef<str>) {
        self.line(format!("{} {{", header.as_ref()));
        self.indent += 1;
    }

    pub fn close(&mut self) {
        self.indent -= 1;
        self.line("}");
    }

    /// Ends the current block and starts the next one, e.g. `} else {`.
    pub fn reopen(&mut self, trailer: impl AsRef<str>) {
        self.indent -= 1;
        self.line(trailer);
        self.indent += 1;
    }

    pub fn finish(self) -> String {
        self.out
    }
}
