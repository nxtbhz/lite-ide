use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptAction {
    OpenFile,
    SaveFileAs,
    SetLanguage,
    CreateFolder { parent_dir: PathBuf },
    ConfirmDeleteFolder { path: PathBuf },
}

#[derive(Clone, Debug)]
pub struct InputPrompt {
    pub action: PromptAction,
    pub title: Cow<'static, str>,
    pub value: String,
    pub cursor_pos: usize,
    pub suggestions: Vec<String>,
    pub selected_suggestion: usize,
}

impl InputPrompt {
    pub fn open_file() -> Self {
        let mut prompt = Self {
            action: PromptAction::OpenFile,
            title: Cow::Borrowed(" Open File (Enter: open, Tab: autocomplete, Esc: cancel) "),
            value: String::new(),
            cursor_pos: 0,
            suggestions: Vec::new(),
            selected_suggestion: 0,
        };
        prompt.update_suggestions();
        prompt
    }

    pub fn save_as(default_name: Option<&str>) -> Self {
        let initial = default_name.unwrap_or("").to_string();
        let len = initial.len();
        let mut prompt = Self {
            action: PromptAction::SaveFileAs,
            title: Cow::Borrowed(" Save File As (Enter: save, Esc: cancel) "),
            value: initial,
            cursor_pos: len,
            suggestions: Vec::new(),
            selected_suggestion: 0,
        };
        prompt.update_suggestions();
        prompt
    }

    pub fn set_language(all_langs: &[&'static str]) -> Self {
        let suggestions: Vec<String> = all_langs.iter().map(|s| s.to_string()).collect();
        Self {
            action: PromptAction::SetLanguage,
            title: Cow::Borrowed(
                " Select Language Mode (Type to filter, Enter: apply, Esc: cancel) ",
            ),
            value: String::new(),
            cursor_pos: 0,
            suggestions,
            selected_suggestion: 0,
        }
    }

    pub fn create_folder(parent_dir: PathBuf, display_name: &str) -> Self {
        let formatted = format!(
            " Create Folder in '{}' (Enter: create, Esc: cancel) ",
            display_name
        );
        Self {
            action: PromptAction::CreateFolder { parent_dir },
            title: Cow::Owned(formatted),
            value: String::new(),
            cursor_pos: 0,
            suggestions: Vec::new(),
            selected_suggestion: 0,
        }
    }

    pub fn confirm_delete_folder(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder");
        let formatted = format!(
            " Delete folder '{}' and all contents? (y/Enter: confirm, Esc: cancel) ",
            name
        );
        Self {
            action: PromptAction::ConfirmDeleteFolder { path },
            title: Cow::Owned(formatted),
            value: String::new(),
            cursor_pos: 0,
            suggestions: Vec::new(),
            selected_suggestion: 0,
        }
    }

    pub fn update_suggestions(&mut self) {
        match self.action {
            PromptAction::OpenFile | PromptAction::SaveFileAs => {
                let path_str = if self.value.is_empty() {
                    "."
                } else {
                    &self.value
                };
                let path = Path::new(path_str);

                let (dir_to_read, prefix) = if path.is_dir() && self.value.ends_with('/') {
                    (path, "")
                } else if let Some(parent) = path.parent() {
                    let parent_dir = if parent.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        parent
                    };
                    let file_prefix = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    (parent_dir, file_prefix)
                } else {
                    (Path::new("."), path_str)
                };

                let mut matches = Vec::new();
                if let Ok(rd) = fs::read_dir(dir_to_read) {
                    for entry in rd.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with('.') && !prefix.starts_with('.') {
                            continue;
                        }
                        if name == "target" || name == "node_modules" {
                            continue;
                        }
                        if name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                            let formatted = if is_dir { format!("{}/", name) } else { name };
                            matches.push(formatted);
                        }
                    }
                }
                matches.sort();
                self.suggestions = matches;
                if self.selected_suggestion >= self.suggestions.len() {
                    self.selected_suggestion = 0;
                }
            }
            PromptAction::SetLanguage
            | PromptAction::CreateFolder { .. }
            | PromptAction::ConfirmDeleteFolder { .. } => {
                // Handled in handle_key with filter or no suggestions needed
            }
        }
    }

    pub fn handle_char(&mut self, c: char) {
        self.value.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.update_suggestions();
    }

    pub fn handle_backspace(&mut self) {
        if self.cursor_pos > 0 {
            if let Some(prev_char) = self.value[..self.cursor_pos].chars().next_back() {
                self.cursor_pos -= prev_char.len_utf8();
                self.value.remove(self.cursor_pos);
                self.update_suggestions();
            }
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_pos > 0 {
            if let Some(prev_char) = self.value[..self.cursor_pos].chars().next_back() {
                self.cursor_pos -= prev_char.len_utf8();
            }
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_pos < self.value.len() {
            if let Some(next_char) = self.value[self.cursor_pos..].chars().next() {
                self.cursor_pos += next_char.len_utf8();
            }
        }
    }

    pub fn autocomplete(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }

        let suggestion = &self.suggestions[self.selected_suggestion];
        match self.action {
            PromptAction::OpenFile | PromptAction::SaveFileAs => {
                let path = Path::new(&self.value);
                let new_val = if let Some(parent) = path.parent() {
                    if parent.as_os_str().is_empty() {
                        suggestion.clone()
                    } else {
                        format!("{}/{}", parent.display(), suggestion)
                    }
                } else {
                    suggestion.clone()
                };
                self.value = new_val;
                self.cursor_pos = self.value.len();
                self.update_suggestions();
            }
            PromptAction::SetLanguage => {
                self.value = suggestion.clone();
                self.cursor_pos = self.value.len();
            }
            PromptAction::CreateFolder { .. } | PromptAction::ConfirmDeleteFolder { .. } => {}
        }
    }

    pub fn render<W: Write>(
        &self,
        writer: &mut W,
        term_width: u16,
        term_height: u16,
    ) -> std::io::Result<()> {
        let box_w = (term_width as usize).clamp(40, 72);
        let max_sugg = 5;
        let sugg_count = self.suggestions.len().min(max_sugg);
        let box_h = 4 + sugg_count;
        let start_x = (term_width as usize).saturating_sub(box_w) / 2;
        let start_y = (term_height as usize).saturating_sub(box_h) / 3;

        // Header / Box
        for y in 0..box_h {
            queue!(
                writer,
                MoveTo(start_x as u16, (start_y + y) as u16),
                SetBackgroundColor(Color::Rgb {
                    r: 20,
                    g: 25,
                    b: 38
                }),
                SetForegroundColor(Color::White)
            )?;

            if y == 0 {
                let fill = box_w.saturating_sub(self.title.len());
                queue!(
                    writer,
                    SetForegroundColor(Color::Cyan),
                    Print(format!("{}{:fill$}", self.title, "", fill = fill))
                )?;
            } else if y == 1 {
                let prompt_prefix = " > ";
                let input_disp = format!("{}{}", prompt_prefix, self.value);
                let fill = box_w.saturating_sub(input_disp.chars().count() + 1);
                queue!(
                    writer,
                    SetForegroundColor(Color::White),
                    Print(format!("{}{:fill$} ", input_disp, "", fill = fill))
                )?;
            } else if y == 2 {
                queue!(
                    writer,
                    SetForegroundColor(Color::DarkGrey),
                    Print("─".repeat(box_w))
                )?;
            } else {
                let sugg_idx = y - 3;
                if sugg_idx < self.suggestions.len() {
                    let sugg = &self.suggestions[sugg_idx];
                    let is_sel = sugg_idx == self.selected_suggestion;
                    let prefix = if is_sel { "  ● " } else { "    " };

                    if is_sel {
                        queue!(
                            writer,
                            SetBackgroundColor(Color::DarkBlue),
                            SetForegroundColor(Color::Yellow)
                        )?;
                    } else {
                        queue!(
                            writer,
                            SetBackgroundColor(Color::Rgb {
                                r: 20,
                                g: 25,
                                b: 38
                            }),
                            SetForegroundColor(Color::White)
                        )?;
                    }

                    let line_content = format!("{}{}", prefix, sugg);
                    let fill = box_w.saturating_sub(line_content.chars().count());
                    queue!(
                        writer,
                        Print(format!("{}{:fill$}", line_content, "", fill = fill))
                    )?;
                } else {
                    queue!(writer, Print(" ".repeat(box_w)))?;
                }
            }
        }

        // Render input cursor
        let cursor_col = self.value[..self.cursor_pos].chars().count();
        let cursor_x = (start_x + 3 + cursor_col) as u16;
        let cursor_y = (start_y + 1) as u16;
        queue!(
            writer,
            MoveTo(cursor_x, cursor_y),
            crossterm::cursor::Show,
            ResetColor
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_multibyte_utf8_input_and_backspace() {
        let mut prompt = InputPrompt::save_as(None);
        for c in "ação_é_🚀.rs".chars() {
            prompt.handle_char(c);
        }
        assert_eq!(prompt.value, "ação_é_🚀.rs");
        assert_eq!(prompt.cursor_pos, prompt.value.len());

        // Move left across ASCII and multibyte characters, insert, and backspace
        prompt.move_left(); // before 's'
        prompt.move_left(); // before 'r'
        prompt.move_left(); // before '.'
        prompt.move_left(); // before '🚀' (4 bytes)
        prompt.handle_char('✨'); // 3 bytes
        assert_eq!(prompt.value, "ação_é_✨🚀.rs");

        prompt.handle_backspace(); // removes '✨'
        assert_eq!(prompt.value, "ação_é_🚀.rs");

        prompt.move_right(); // after '🚀'
        prompt.handle_backspace(); // removes '🚀'
        assert_eq!(prompt.value, "ação_é_.rs");
    }

    #[test]
    fn test_prompt_create_folder_action() {
        use std::path::PathBuf;
        let parent = PathBuf::from("/workspace/crates");
        let prompt = InputPrompt::create_folder(parent.clone(), "crates");
        assert_eq!(
            prompt.action,
            PromptAction::CreateFolder { parent_dir: parent }
        );
        assert!(prompt.title.contains("crates"));
        assert_eq!(prompt.value, "");
    }

    #[test]
    fn test_prompt_confirm_delete_folder_action() {
        use std::path::PathBuf;
        let target = PathBuf::from("/workspace/crates/old_module");
        let prompt = InputPrompt::confirm_delete_folder(target.clone());
        assert_eq!(
            prompt.action,
            PromptAction::ConfirmDeleteFolder { path: target }
        );
        assert!(prompt.title.contains("old_module"));
    }
}
