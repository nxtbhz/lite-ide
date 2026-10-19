use crate::buffer::{TextBuffer, TextDelta};
use crate::cursor::Cursor;
use std::path::Path;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub delta: TextDelta,
    pub cursors_before: Vec<Cursor>,
    pub cursors_after: Vec<Cursor>,
}

pub trait HistoryManager {
    fn record_edit(
        &mut self,
        delta: TextDelta,
        cursors_before: Vec<Cursor>,
        cursors_after: Vec<Cursor>,
    );
    fn undo(&mut self, current_buffer: &mut dyn TextBuffer) -> Option<Vec<Cursor>>;
    fn redo(&mut self, current_buffer: &mut dyn TextBuffer) -> Option<Vec<Cursor>>;
    fn persist_to_disk(&self, path: &Path) -> std::io::Result<()>;
}

// shortcut: linear undo stack for M0; full branched undo-tree with snapshot journal in M1
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct History {
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

impl HistoryManager for History {
    fn record_edit(
        &mut self,
        delta: TextDelta,
        cursors_before: Vec<Cursor>,
        cursors_after: Vec<Cursor>,
    ) {
        self.redo_stack.clear();
        self.undo_stack.push(HistoryEntry {
            delta,
            cursors_before,
            cursors_after,
        });
    }

    fn undo(&mut self, current_buffer: &mut dyn TextBuffer) -> Option<Vec<Cursor>> {
        let entry = self.undo_stack.pop()?;
        let delta = &entry.delta;

        // Revert edit: remove new_text, re-insert old_text
        if !delta.new_text.is_empty() {
            let _ =
                current_buffer.delete(delta.start_byte, delta.start_byte + delta.new_text.len());
        }
        if !delta.old_text.is_empty() {
            let _ = current_buffer.insert(delta.start_byte, &delta.old_text);
        }

        let cursors = entry.cursors_before.clone();
        self.redo_stack.push(entry);
        Some(cursors)
    }

    fn redo(&mut self, current_buffer: &mut dyn TextBuffer) -> Option<Vec<Cursor>> {
        let entry = self.redo_stack.pop()?;
        let delta = &entry.delta;

        // Re-apply edit: remove old_text, insert new_text
        if !delta.old_text.is_empty() {
            let _ =
                current_buffer.delete(delta.start_byte, delta.start_byte + delta.old_text.len());
        }
        if !delta.new_text.is_empty() {
            let _ = current_buffer.insert(delta.start_byte, &delta.new_text);
        }

        let cursors = entry.cursors_after.clone();
        self.undo_stack.push(entry);
        Some(cursors)
    }

    fn persist_to_disk(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(path)?;
        // serialize history
        bincode_like_json(self, file)
    }
}

fn bincode_like_json<W: std::io::Write>(history: &History, writer: W) -> std::io::Result<()> {
    // shortcut: use JSON for history persistence in M0; binary bincode in M1
    serde_json::to_writer(writer, history).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::RopeBuffer;
    use proptest::prelude::*;

    #[test]
    fn test_undo_redo_symmetry() {
        let mut buf = RopeBuffer::from_str("Hello World");
        let mut history = History::new();

        // 1. Insert ", Brave" at byte 5
        let delta = buf.insert(5, ", Brave").unwrap();
        history.record_edit(delta, vec![Cursor::new(5)], vec![Cursor::new(13)]);

        assert_eq!(buf.slice_to_cow(0, buf.len_bytes()), "Hello, Brave World");
        assert!(history.can_undo());
        assert!(!history.can_redo());

        // 2. Undo
        let cursors = history.undo(&mut buf).unwrap();
        assert_eq!(cursors[0].head, 5);
        assert_eq!(buf.slice_to_cow(0, buf.len_bytes()), "Hello World");
        assert!(history.can_redo());

        // 3. Redo
        let cursors = history.redo(&mut buf).unwrap();
        assert_eq!(cursors[0].head, 13);
        assert_eq!(buf.slice_to_cow(0, buf.len_bytes()), "Hello, Brave World");
    }

    proptest! {
        #[test]
        fn prop_undo_redo_invariance(initial in "\\PC*", edits in prop::collection::vec("[a-zA-Z0-9 ]+", 1..10)) {
            let mut buf = RopeBuffer::from_str(&initial);
            let mut history = History::new();

            for edit in &edits {
                let len = buf.len_bytes();
                let insert_pos = if len == 0 { 0 } else { len / 2 };
                // ensure insert_pos is on valid char boundary
                let char_idx = buf.byte_to_char(insert_pos);
                let byte_pos = buf.char_to_byte(char_idx);

                if let Ok(delta) = buf.insert(byte_pos, edit) {
                    history.record_edit(delta, vec![Cursor::new(byte_pos)], vec![Cursor::new(byte_pos + edit.len())]);
                }
            }

            // Undo all edits
            while history.can_undo() {
                history.undo(&mut buf);
            }

            // Invariant: buffer must match initial string exactly
            prop_assert_eq!(buf.slice_to_cow(0, buf.len_bytes()), initial);
        }
    }
}
