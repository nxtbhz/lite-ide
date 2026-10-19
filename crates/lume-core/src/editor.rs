use crate::autosave::AutoSaveController;
use crate::buffer::{BufferError, RopeBuffer, TextBuffer};
use crate::cursor::{Cursor, CursorSet};
use crate::history::{History, HistoryManager};
use crate::journal::Journal;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Tab {
    pub id: usize,
    pub buffer: RopeBuffer,
    pub cursors: CursorSet,
    pub history: History,
    pub file_path: Option<PathBuf>,
    pub is_dirty: bool,
    pub recovered_from_journal: bool,
}

impl Tab {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            buffer: RopeBuffer::new(),
            cursors: CursorSet::new(),
            history: History::new(),
            file_path: None,
            is_dirty: false,
            recovered_from_journal: false,
        }
    }

    pub fn from_str(id: usize, text: &str) -> Self {
        Self {
            id,
            buffer: RopeBuffer::from_str(text),
            cursors: CursorSet::new(),
            history: History::new(),
            file_path: None,
            is_dirty: false,
            recovered_from_journal: false,
        }
    }

    pub fn name(&self) -> String {
        if let Some(ref p) = self.file_path {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("[No Name]")
                .to_string()
        } else {
            format!("[Tab {}]", self.id)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Editor {
    tabs: Vec<Tab>,
    active_tab: usize,
    next_tab_id: usize,
    autosave: AutoSaveController,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        Self {
            tabs: vec![Tab::new(1)],
            active_tab: 0,
            next_tab_id: 2,
            autosave: AutoSaveController::default(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        Self {
            tabs: vec![Tab::from_str(1, text)],
            active_tab: 0,
            next_tab_id: 2,
            autosave: AutoSaveController::default(),
        }
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let mut ed = Self::new();
        ed.open_in_current_tab(path)?;
        Ok(ed)
    }

    pub fn open_in_current_tab<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        let path = path.as_ref();

        // Check if crash recovery journal exists for this file
        let (buffer, recovered) = if let Ok(Some(recovered_text)) = Journal::recover(path) {
            (RopeBuffer::from_str(&recovered_text), true)
        } else {
            let file = std::fs::File::open(path)?;
            (RopeBuffer::from_reader(file)?, false)
        };

        let tab = &mut self.tabs[self.active_tab];
        tab.buffer = buffer;
        tab.cursors.set_single(0);
        tab.history.clear();
        tab.file_path = Some(path.to_path_buf());
        tab.is_dirty = recovered;
        tab.recovered_from_journal = recovered;

        if recovered {
            self.autosave.notify_change();
        }

        Ok(())
    }

    // Tabs management
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut [Tab] {
        &mut self.tabs
    }

    pub fn active_tab_idx(&self) -> usize {
        self.active_tab
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    pub fn new_tab(&mut self) -> usize {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::new(id));
        self.active_tab = self.tabs.len() - 1;
        self.active_tab
    }

    pub fn open_tab<P: AsRef<Path>>(&mut self, path: P) -> Result<usize, std::io::Error> {
        let path = path.as_ref();
        // If already open, switch to it
        for (i, tab) in self.tabs.iter().enumerate() {
            if let Some(ref p) = tab.file_path {
                if p == path {
                    self.active_tab = i;
                    return Ok(i);
                }
            }
        }

        let idx = self.new_tab();
        self.open_in_current_tab(path)?;
        Ok(idx)
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        if let Some(ref p) = self.tabs[index].file_path {
            let _ = Journal::remove(p);
        }

        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::new(self.next_tab_id));
            self.next_tab_id += 1;
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        true
    }

    pub fn switch_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_tab = index;
            true
        } else {
            false
        }
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
    }

    // Active buffer delegation
    pub fn buffer(&self) -> &RopeBuffer {
        &self.tabs[self.active_tab].buffer
    }

    pub fn buffer_mut(&mut self) -> &mut RopeBuffer {
        &mut self.tabs[self.active_tab].buffer
    }

    pub fn cursors(&self) -> &CursorSet {
        &self.tabs[self.active_tab].cursors
    }

    pub fn cursors_mut(&mut self) -> &mut CursorSet {
        &mut self.tabs[self.active_tab].cursors
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.tabs[self.active_tab].file_path.as_deref()
    }

    pub fn set_file_path(&mut self, path: Option<PathBuf>) {
        self.tabs[self.active_tab].file_path = path;
    }

    pub fn is_dirty(&self) -> bool {
        self.tabs[self.active_tab].is_dirty
    }

    pub fn save(&mut self) -> Result<(), std::io::Error> {
        let path = self.tabs[self.active_tab].file_path.clone();
        if let Some(path) = path {
            self.save_as(&path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No file path specified",
            ))
        }
    }

    pub fn save_as<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        let path = path.as_ref();
        let temp_path = path.with_extension(format!(
            "{}.lume_tmp",
            path.extension().and_then(|s| s.to_str()).unwrap_or("tmp")
        ));

        {
            let file = std::fs::File::create(&temp_path)?;
            let mut writer = std::io::BufWriter::new(file);
            self.tabs[self.active_tab].buffer.write_to(&mut writer)?;
            writer.get_ref().sync_all()?;
        }

        std::fs::rename(&temp_path, path)?;
        self.tabs[self.active_tab].file_path = Some(path.to_path_buf());
        self.tabs[self.active_tab].is_dirty = false;
        self.tabs[self.active_tab].recovered_from_journal = false;

        // Clean up recovery journal on clean save
        let _ = Journal::remove(path);
        self.autosave.mark_saved();

        Ok(())
    }

    // AutoSave check
    pub fn check_autosave(&mut self) -> bool {
        if self.autosave.should_autosave(self.is_dirty()) && self.file_path().is_some() {
            self.save().is_ok()
        } else {
            false
        }
    }

    pub fn autosave_controller(&self) -> &AutoSaveController {
        &self.autosave
    }

    pub fn autosave_controller_mut(&mut self) -> &mut AutoSaveController {
        &mut self.autosave
    }

    fn record_edit_internal(&mut self) {
        self.tabs[self.active_tab].is_dirty = true;
        self.autosave.notify_change();

        // Write to crash recovery journal if file path is known
        if let Some(ref path) = self.tabs[self.active_tab].file_path {
            let text = self.tabs[self.active_tab]
                .buffer
                .slice_to_cow(0, self.tabs[self.active_tab].buffer.len_bytes());
            let _ = Journal::record(path, &text);
        }
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let tab = &self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let line = tab.buffer.byte_to_line(head);
        let line_start_char = tab.buffer.rope().line_to_char(line);
        let cursor_char = tab.buffer.byte_to_char(head);
        let col = cursor_char.saturating_sub(line_start_char);
        (line, col)
    }

    pub fn set_cursor_from_line_col(&mut self, line: usize, col: usize, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let total_lines = tab.buffer.len_lines();
        let target_line = line.min(total_lines.saturating_sub(1));
        let line_start_char = tab.buffer.rope().line_to_char(target_line);
        let line_len_chars = tab.buffer.line_len_chars(target_line);
        let content_len = if line_len_chars > 0
            && tab.buffer.rope().char(line_start_char + line_len_chars - 1) == '\n'
        {
            line_len_chars - 1
        } else {
            line_len_chars
        };

        let target_char = line_start_char + col.min(content_len);
        let target_byte = tab.buffer.char_to_byte(target_char);

        let primary = tab.cursors.primary_mut();
        if select {
            primary.select_to(target_byte);
        } else {
            primary.move_to(target_byte);
        }
    }

    pub fn select_word_at_cursor(&mut self) {
        let tab = &mut self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let len = tab.buffer.len_bytes();
        if len == 0 {
            return;
        }

        let char_idx = tab.buffer.byte_to_char(head);
        let total_chars = tab.buffer.len_chars();

        // Scan backwards
        let mut start_char = char_idx;
        while start_char > 0 {
            let prev_c = tab.buffer.rope().char(start_char - 1);
            if prev_c.is_alphanumeric() || prev_c == '_' {
                start_char -= 1;
            } else {
                break;
            }
        }

        // Scan forwards
        let mut end_char = char_idx;
        while end_char < total_chars {
            let c = tab.buffer.rope().char(end_char);
            if c.is_alphanumeric() || c == '_' {
                end_char += 1;
            } else {
                break;
            }
        }

        let start_byte = tab.buffer.char_to_byte(start_char);
        let end_byte = tab.buffer.char_to_byte(end_char);

        let primary = tab.cursors.primary_mut();
        primary.anchor = start_byte;
        primary.head = end_byte;
    }

    pub fn select_line_at_cursor(&mut self) {
        let tab = &mut self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let line = tab.buffer.byte_to_line(head);
        let start_byte = tab.buffer.line_to_byte(line);
        let next_line = line + 1;
        let end_byte = if next_line < tab.buffer.len_lines() {
            tab.buffer.line_to_byte(next_line)
        } else {
            tab.buffer.len_bytes()
        };

        let primary = tab.cursors.primary_mut();
        primary.anchor = start_byte;
        primary.head = end_byte;
    }

    pub fn insert_char(&mut self, c: char) -> Result<(), BufferError> {
        let mut text = [0u8; 4];
        let s = c.encode_utf8(&mut text);
        self.insert_str(s)
    }

    pub fn insert_str(&mut self, s: &str) -> Result<(), BufferError> {
        let tab = &mut self.tabs[self.active_tab];
        let cursors_before = tab.cursors.cursors().to_vec();
        let primary = *tab.cursors.primary();

        let insert_pos = if primary.is_selection() {
            let start = primary.min();
            let end = primary.max();
            let del_delta = tab.buffer.delete(start, end)?;
            tab.history
                .record_edit(del_delta, cursors_before.clone(), vec![Cursor::new(start)]);
            start
        } else {
            primary.head
        };

        let delta = tab.buffer.insert(insert_pos, s)?;
        let new_pos = insert_pos + s.len();
        tab.cursors.set_single(new_pos);
        let cursors_after = tab.cursors.cursors().to_vec();

        tab.history
            .record_edit(delta, cursors_before, cursors_after);
        self.record_edit_internal();
        Ok(())
    }

    pub fn insert_newline(&mut self) -> Result<(), BufferError> {
        self.insert_str("\n")
    }

    pub fn delete_backwards(&mut self) -> Result<(), BufferError> {
        let tab = &mut self.tabs[self.active_tab];
        let primary = *tab.cursors.primary();
        let cursors_before = tab.cursors.cursors().to_vec();

        if primary.is_selection() {
            let start = primary.min();
            let end = primary.max();
            let delta = tab.buffer.delete(start, end)?;
            tab.cursors.set_single(start);
            let cursors_after = tab.cursors.cursors().to_vec();
            tab.history
                .record_edit(delta, cursors_before, cursors_after);
            self.record_edit_internal();
        } else if primary.head > 0 {
            let char_idx = tab.buffer.byte_to_char(primary.head);
            let prev_byte = tab.buffer.char_to_byte(char_idx - 1);
            let delta = tab.buffer.delete(prev_byte, primary.head)?;
            tab.cursors.set_single(prev_byte);
            let cursors_after = tab.cursors.cursors().to_vec();
            tab.history
                .record_edit(delta, cursors_before, cursors_after);
            self.record_edit_internal();
        }

        Ok(())
    }

    pub fn delete_forwards(&mut self) -> Result<(), BufferError> {
        let tab = &mut self.tabs[self.active_tab];
        let primary = *tab.cursors.primary();
        let cursors_before = tab.cursors.cursors().to_vec();

        if primary.is_selection() {
            let start = primary.min();
            let end = primary.max();
            let delta = tab.buffer.delete(start, end)?;
            tab.cursors.set_single(start);
            let cursors_after = tab.cursors.cursors().to_vec();
            tab.history
                .record_edit(delta, cursors_before, cursors_after);
            self.record_edit_internal();
        } else if primary.head < tab.buffer.len_bytes() {
            let char_idx = tab.buffer.byte_to_char(primary.head);
            let next_byte = tab.buffer.char_to_byte(char_idx + 1);
            let delta = tab.buffer.delete(primary.head, next_byte)?;
            let cursors_after = tab.cursors.cursors().to_vec();
            tab.history
                .record_edit(delta, cursors_before, cursors_after);
            self.record_edit_internal();
        }

        Ok(())
    }

    pub fn move_left(&mut self, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let primary = tab.cursors.primary_mut();
        if !select && primary.is_selection() {
            let target = primary.min();
            primary.move_to(target);
            return;
        }

        if primary.head > 0 {
            let char_idx = tab.buffer.byte_to_char(primary.head);
            let prev_byte = tab.buffer.char_to_byte(char_idx - 1);
            if select {
                primary.select_to(prev_byte);
            } else {
                primary.move_to(prev_byte);
            }
        }
    }

    pub fn move_right(&mut self, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let primary = tab.cursors.primary_mut();
        if !select && primary.is_selection() {
            let target = primary.max();
            primary.move_to(target);
            return;
        }

        if primary.head < tab.buffer.len_bytes() {
            let char_idx = tab.buffer.byte_to_char(primary.head);
            let next_byte = tab.buffer.char_to_byte(char_idx + 1);
            if select {
                primary.select_to(next_byte);
            } else {
                primary.move_to(next_byte);
            }
        }
    }

    pub fn move_up(&mut self, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let line = tab.buffer.byte_to_line(head);
        if line == 0 {
            if !select {
                tab.cursors.primary_mut().move_to(0);
            } else {
                tab.cursors.primary_mut().select_to(0);
            }
            return;
        }

        let line_start_char = tab.buffer.rope().line_to_char(line);
        let cursor_char = tab.buffer.byte_to_char(head);
        let col = cursor_char.saturating_sub(line_start_char);

        let prev_line = line - 1;
        let prev_line_start_char = tab.buffer.rope().line_to_char(prev_line);
        let prev_line_len = tab.buffer.line_len_chars(prev_line);
        let prev_line_content_len = if prev_line_len > 0
            && tab
                .buffer
                .rope()
                .char(prev_line_start_char + prev_line_len - 1)
                == '\n'
        {
            prev_line_len - 1
        } else {
            prev_line_len
        };

        let target_char = prev_line_start_char + col.min(prev_line_content_len);
        let target_byte = tab.buffer.char_to_byte(target_char);

        let primary = tab.cursors.primary_mut();
        if select {
            primary.select_to(target_byte);
        } else {
            primary.move_to(target_byte);
        }
    }

    pub fn move_down(&mut self, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let line = tab.buffer.byte_to_line(head);
        let total_lines = tab.buffer.len_lines();
        if line + 1 >= total_lines {
            let end_byte = tab.buffer.len_bytes();
            if !select {
                tab.cursors.primary_mut().move_to(end_byte);
            } else {
                tab.cursors.primary_mut().select_to(end_byte);
            }
            return;
        }

        let line_start_char = tab.buffer.rope().line_to_char(line);
        let cursor_char = tab.buffer.byte_to_char(head);
        let col = cursor_char.saturating_sub(line_start_char);

        let next_line = line + 1;
        let next_line_start_char = tab.buffer.rope().line_to_char(next_line);
        let next_line_len = tab.buffer.line_len_chars(next_line);
        let next_line_content_len = if next_line_len > 0
            && tab
                .buffer
                .rope()
                .char(next_line_start_char + next_line_len - 1)
                == '\n'
        {
            next_line_len - 1
        } else {
            next_line_len
        };

        let target_char = next_line_start_char + col.min(next_line_content_len);
        let target_byte = tab.buffer.char_to_byte(target_char);

        let primary = tab.cursors.primary_mut();
        if select {
            primary.select_to(target_byte);
        } else {
            primary.move_to(target_byte);
        }
    }

    pub fn move_home(&mut self, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let line = tab.buffer.byte_to_line(head);
        let start_byte = tab.buffer.line_to_byte(line);

        let primary = tab.cursors.primary_mut();
        if select {
            primary.select_to(start_byte);
        } else {
            primary.move_to(start_byte);
        }
    }

    pub fn move_end(&mut self, select: bool) {
        let tab = &mut self.tabs[self.active_tab];
        let head = tab.cursors.primary().head;
        let line = tab.buffer.byte_to_line(head);
        let line_start_char = tab.buffer.rope().line_to_char(line);
        let line_len = tab.buffer.line_len_chars(line);
        let content_len =
            if line_len > 0 && tab.buffer.rope().char(line_start_char + line_len - 1) == '\n' {
                line_len - 1
            } else {
                line_len
            };

        let target_char = line_start_char + content_len;
        let target_byte = tab.buffer.char_to_byte(target_char);

        let primary = tab.cursors.primary_mut();
        if select {
            primary.select_to(target_byte);
        } else {
            primary.move_to(target_byte);
        }
    }

    pub fn undo(&mut self) -> bool {
        let tab = &mut self.tabs[self.active_tab];
        if let Some(cursors) = tab.history.undo(&mut tab.buffer) {
            tab.cursors.set_cursors(cursors);
            self.record_edit_internal();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        let tab = &mut self.tabs[self.active_tab];
        if let Some(cursors) = tab.history.redo(&mut tab.buffer) {
            tab.cursors.set_cursors(cursors);
            self.record_edit_internal();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_typing_and_navigation() {
        let mut ed = Editor::new();
        ed.insert_str("line 1\nline 2").unwrap();

        assert_eq!(ed.cursor_line_col(), (1, 6));

        ed.move_left(false);
        assert_eq!(ed.cursor_line_col(), (1, 5));

        ed.move_up(false);
        assert_eq!(ed.cursor_line_col(), (0, 5));

        ed.move_home(false);
        assert_eq!(ed.cursor_line_col(), (0, 0));

        ed.move_end(false);
        assert_eq!(ed.cursor_line_col(), (0, 6));

        ed.delete_backwards().unwrap();
        assert_eq!(ed.cursor_line_col(), (0, 5));
        assert!(ed
            .buffer()
            .slice_to_cow(0, ed.buffer().len_bytes())
            .starts_with("line \n"));
    }

    #[test]
    fn test_editor_undo_redo() {
        let mut ed = Editor::new();
        ed.insert_str("abc").unwrap();
        assert_eq!(ed.buffer().slice_to_cow(0, ed.buffer().len_bytes()), "abc");

        ed.undo();
        assert_eq!(ed.buffer().slice_to_cow(0, ed.buffer().len_bytes()), "");

        ed.redo();
        assert_eq!(ed.buffer().slice_to_cow(0, ed.buffer().len_bytes()), "abc");
    }

    #[test]
    fn test_editor_atomic_save_and_journal_cleanup() {
        let tmp_dir = std::env::temp_dir();
        let file_path = tmp_dir.join(format!("lume_test_save_{}.txt", std::process::id()));

        let mut ed = Editor::new();
        ed.set_file_path(Some(file_path.clone()));
        ed.insert_str("Atomic save content").unwrap();

        // While dirty, journal exists
        assert!(Journal::has_journal(&file_path));

        ed.save().unwrap();
        assert!(!ed.is_dirty());
        assert!(file_path.exists());
        // Clean save removes journal
        assert!(!Journal::has_journal(&file_path));

        let read_back = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(read_back, "Atomic save content");

        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_editor_crash_recovery() {
        let tmp_dir = std::env::temp_dir();
        let file_path = tmp_dir.join(format!("lume_crash_test_{}.txt", std::process::id()));
        std::fs::write(&file_path, "original disk text").unwrap();

        // 1. User opens file and modifies it
        {
            let mut ed = Editor::open(&file_path).unwrap();
            ed.set_cursor_from_line_col(0, 18, false);
            ed.insert_str(" + unsaved edits").unwrap();
            // Journal records unsaved changes!
            assert!(Journal::has_journal(&file_path));
            // Simulate crash: ed is dropped without calling save()!
        }

        // 2. User re-opens file after crash
        let recovered_ed = Editor::open(&file_path).unwrap();
        assert!(recovered_ed.active_tab().recovered_from_journal);
        assert!(recovered_ed.is_dirty());
        let text = recovered_ed
            .buffer()
            .slice_to_cow(0, recovered_ed.buffer().len_bytes());
        assert_eq!(text, "original disk text + unsaved edits");

        // Clean up
        let _ = Journal::remove(&file_path);
        let _ = std::fs::remove_file(&file_path);
    }

    #[test]
    fn test_editor_multi_tabs() {
        let mut ed = Editor::new();
        ed.insert_str("tab 1 content").unwrap();

        let tab2_idx = ed.new_tab();
        assert_eq!(tab2_idx, 1);
        ed.insert_str("tab 2 content").unwrap();

        assert_eq!(
            ed.buffer().slice_to_cow(0, ed.buffer().len_bytes()),
            "tab 2 content"
        );

        ed.switch_tab(0);
        assert_eq!(
            ed.buffer().slice_to_cow(0, ed.buffer().len_bytes()),
            "tab 1 content"
        );

        ed.close_tab(0);
        assert_eq!(ed.tabs().len(), 1);
        assert_eq!(
            ed.buffer().slice_to_cow(0, ed.buffer().len_bytes()),
            "tab 2 content"
        );
    }

    #[test]
    fn test_editor_mouse_word_and_line_selection() {
        let mut ed = Editor::new();
        ed.insert_str("hello_world and goodbye\nsecond line")
            .unwrap();

        // Click inside "hello_world"
        ed.set_cursor_from_line_col(0, 4, false);
        ed.select_word_at_cursor();
        let primary = ed.cursors().primary();
        assert!(primary.is_selection());
        assert_eq!(primary.min(), 0);
        assert_eq!(primary.max(), 11);

        // Triple click on second line
        ed.set_cursor_from_line_col(1, 3, false);
        ed.select_line_at_cursor();
        let primary = ed.cursors().primary();
        assert!(primary.is_selection());
        assert_eq!(primary.min(), 24);
        assert_eq!(primary.max(), 35);
    }
}
