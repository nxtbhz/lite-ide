use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TextDelta {
    pub start_byte: usize,
    pub old_text: String,
    pub new_text: String,
    pub timestamp: u64,
}

impl TextDelta {
    pub fn new(
        start_byte: usize,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            start_byte,
            old_text: old_text.into(),
            new_text: new_text.into(),
            timestamp,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BufferError {
    #[error("Byte index {0} out of bounds (len: {1})")]
    OutOfBounds(usize, usize),
    #[error("Invalid byte range {0}..{1} (len: {2})")]
    InvalidRange(usize, usize, usize),
    #[error("Index is not a valid UTF-8 char boundary: {0}")]
    NotCharBoundary(usize),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait TextBuffer: Send + Sync {
    fn len_bytes(&self) -> usize;
    fn len_lines(&self) -> usize;
    fn byte_to_line(&self, byte_idx: usize) -> usize;
    fn line_to_byte(&self, line_idx: usize) -> usize;
    fn slice_to_cow(&self, start_byte: usize, end_byte: usize) -> std::borrow::Cow<'_, str>;
    fn insert(&mut self, byte_idx: usize, text: &str) -> Result<TextDelta, BufferError>;
    fn delete(&mut self, start_byte: usize, end_byte: usize) -> Result<TextDelta, BufferError>;
}

#[derive(Clone, Debug)]
pub struct RopeBuffer {
    rope: ropey::Rope,
}

impl Default for RopeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl From<&str> for RopeBuffer {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl std::str::FromStr for RopeBuffer {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str(s))
    }
}

impl RopeBuffer {
    pub fn new() -> Self {
        Self {
            rope: ropey::Rope::new(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        Self {
            rope: ropey::Rope::from_str(text),
        }
    }

    pub fn from_reader<R: std::io::Read>(reader: R) -> Result<Self, std::io::Error> {
        let rope = ropey::Rope::from_reader(reader)?;
        Ok(Self { rope })
    }

    pub fn write_to<W: std::io::Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        self.rope.write_to(&mut writer)
    }

    pub fn rope(&self) -> &ropey::Rope {
        &self.rope
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn char_to_byte(&self, char_idx: usize) -> usize {
        self.rope.char_to_byte(char_idx)
    }

    pub fn byte_to_char(&self, byte_idx: usize) -> usize {
        self.rope.byte_to_char(byte_idx)
    }

    pub fn line(&self, line_idx: usize) -> Option<ropey::RopeSlice<'_>> {
        if line_idx < self.rope.len_lines() {
            Some(self.rope.line(line_idx))
        } else {
            None
        }
    }

    pub fn line_len_bytes(&self, line_idx: usize) -> usize {
        if line_idx < self.rope.len_lines() {
            self.rope.line(line_idx).len_bytes()
        } else {
            0
        }
    }

    pub fn line_len_chars(&self, line_idx: usize) -> usize {
        if line_idx < self.rope.len_lines() {
            self.rope.line(line_idx).len_chars()
        } else {
            0
        }
    }
}

impl TextBuffer for RopeBuffer {
    fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    fn byte_to_line(&self, byte_idx: usize) -> usize {
        let byte_idx = byte_idx.min(self.rope.len_bytes());
        self.rope.byte_to_line(byte_idx)
    }

    fn line_to_byte(&self, line_idx: usize) -> usize {
        let line_idx = line_idx.min(self.rope.len_lines());
        self.rope.line_to_byte(line_idx)
    }

    fn slice_to_cow(&self, start_byte: usize, end_byte: usize) -> Cow<'_, str> {
        let len = self.rope.len_bytes();
        let start = start_byte.min(len);
        let end = end_byte.min(len);
        if start >= end {
            return Cow::Borrowed("");
        }

        let start_char = self.rope.byte_to_char(start);
        let end_char = self.rope.byte_to_char(end);
        let slice = self.rope.slice(start_char..end_char);

        if let Some(s) = slice.as_str() {
            Cow::Borrowed(s)
        } else {
            Cow::Owned(slice.to_string())
        }
    }

    fn insert(&mut self, byte_idx: usize, text: &str) -> Result<TextDelta, BufferError> {
        let len = self.rope.len_bytes();
        if byte_idx > len {
            return Err(BufferError::OutOfBounds(byte_idx, len));
        }

        let char_idx = self.rope.byte_to_char(byte_idx);
        // Verify char boundary
        if self.rope.char_to_byte(char_idx) != byte_idx {
            return Err(BufferError::NotCharBoundary(byte_idx));
        }

        self.rope.insert(char_idx, text);

        Ok(TextDelta::new(byte_idx, "", text))
    }

    fn delete(&mut self, start_byte: usize, end_byte: usize) -> Result<TextDelta, BufferError> {
        let len = self.rope.len_bytes();
        if start_byte > len || end_byte > len || start_byte > end_byte {
            return Err(BufferError::InvalidRange(start_byte, end_byte, len));
        }

        if start_byte == end_byte {
            return Ok(TextDelta::new(start_byte, "", ""));
        }

        let start_char = self.rope.byte_to_char(start_byte);
        let end_char = self.rope.byte_to_char(end_byte);

        if self.rope.char_to_byte(start_char) != start_byte {
            return Err(BufferError::NotCharBoundary(start_byte));
        }
        if self.rope.char_to_byte(end_char) != end_byte {
            return Err(BufferError::NotCharBoundary(end_byte));
        }

        let deleted_text = self.rope.slice(start_char..end_char).to_string();
        self.rope.remove(start_char..end_char);

        Ok(TextDelta::new(start_byte, deleted_text, ""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let buf = RopeBuffer::new();
        assert_eq!(buf.len_bytes(), 0);
        assert_eq!(buf.len_lines(), 1);
        assert_eq!(buf.byte_to_line(0), 0);
        assert_eq!(buf.line_to_byte(0), 0);
    }

    #[test]
    fn test_insert_and_delete() {
        let mut buf = RopeBuffer::from_str("Hello World");
        assert_eq!(buf.len_bytes(), 11);
        assert_eq!(buf.slice_to_cow(0, 5), "Hello");

        let delta = buf.insert(5, ", Beautiful").unwrap();
        assert_eq!(delta.new_text, ", Beautiful");
        assert_eq!(
            buf.slice_to_cow(0, buf.len_bytes()),
            "Hello, Beautiful World"
        );

        let del_delta = buf.delete(5, 16).unwrap();
        assert_eq!(del_delta.old_text, ", Beautiful");
        assert_eq!(buf.slice_to_cow(0, buf.len_bytes()), "Hello World");
    }

    #[test]
    fn test_utf8_multibyte() {
        let mut buf = RopeBuffer::from_str("Olá Mundo! 🚀");
        assert_eq!(buf.len_lines(), 1);

        // Attempting to insert inside multibyte character should fail gracefully
        let res = buf.insert(3, "x"); // byte 3 is inside 'á' (bytes 2..4)
        assert!(res.is_err());

        // Insert at valid boundary
        buf.insert(0, "¡").unwrap();
        assert!(buf.slice_to_cow(0, buf.len_bytes()).starts_with("¡Olá"));
    }

    #[test]
    fn test_line_indexing() {
        let buf = RopeBuffer::from_str("Line 1\nLine 2\nLine 3\n");
        assert_eq!(buf.len_lines(), 4); // last empty line
        assert_eq!(buf.byte_to_line(0), 0);
        assert_eq!(buf.byte_to_line(7), 1);
        assert_eq!(buf.line_to_byte(0), 0);
        assert_eq!(buf.line_to_byte(1), 7);
    }

    #[test]
    fn test_insertion_latency_in_100k_lines() {
        let mut text = String::with_capacity(100_000 * 30);
        for i in 0..100_000 {
            use std::fmt::Write;
            let _ = writeln!(text, "let var_{} = {};", i, i);
        }

        let mut buf = RopeBuffer::from_str(&text);
        assert_eq!(buf.len_lines(), 100_001);

        let mid_byte = buf.line_to_byte(50_000);
        let start = std::time::Instant::now();
        buf.insert(mid_byte, "// comment\n").unwrap();
        let elapsed = start.elapsed();

        // Target: insertion in 100k lines < 5ms
        assert!(
            elapsed < std::time::Duration::from_millis(5),
            "Insertion took {:?}, exceeding 5ms target",
            elapsed
        );
    }
}
