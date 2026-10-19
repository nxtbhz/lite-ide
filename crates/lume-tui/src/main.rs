use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{
        disable_raw_mode, enable_raw_mode, size as terminal_size, Clear, ClearType,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use lume_core::{Editor, TextBuffer};
use lume_syntax::{HighlightToken, Language, SyntaxHighlighter, TokenType};
use std::collections::HashMap;
use std::env;
use std::io::{stdout, BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

enum AppEvent {
    Input(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

struct TerminalGuard;

impl TerminalGuard {
    fn init() -> Result<Self, std::io::Error> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, Hide, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = stdout();
        let _ = execute!(
            out,
            Show,
            ResetColor,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

#[derive(Clone, Debug)]
struct CommandItem {
    id: &'static str,
    title: &'static str,
    shortcut: &'static str,
}

const COMMANDS: &[CommandItem] = &[
    CommandItem {
        id: "file.save",
        title: "File: Save",
        shortcut: "Ctrl+S",
    },
    CommandItem {
        id: "file.new_tab",
        title: "File: New Tab",
        shortcut: "Ctrl+T",
    },
    CommandItem {
        id: "file.close_tab",
        title: "File: Close Tab",
        shortcut: "Ctrl+W",
    },
    CommandItem {
        id: "file.next_tab",
        title: "File: Next Tab",
        shortcut: "Ctrl+Tab / Alt+Right",
    },
    CommandItem {
        id: "file.prev_tab",
        title: "File: Previous Tab",
        shortcut: "Alt+Left",
    },
    CommandItem {
        id: "edit.undo",
        title: "Edit: Undo",
        shortcut: "Ctrl+Z",
    },
    CommandItem {
        id: "edit.redo",
        title: "Edit: Redo",
        shortcut: "Ctrl+Y",
    },
    CommandItem {
        id: "view.which_key",
        title: "View: Which-Key Shortcuts",
        shortcut: "F1",
    },
    CommandItem {
        id: "view.toggle_line_numbers",
        title: "View: Toggle Line Numbers",
        shortcut: "",
    },
    CommandItem {
        id: "app.quit",
        title: "App: Quit",
        shortcut: "Ctrl+Q",
    },
];

pub struct App {
    pub editor: Editor,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub terminal_width: u16,
    pub terminal_height: u16,
    status_message: Option<(String, Instant)>,
    pub running: bool,
    show_line_numbers: bool,

    // Mouse interaction
    mouse_dragging: bool,
    last_click_time: Instant,
    last_click_pos: (u16, u16),
    click_count: u8,

    // Overlays: Command Palette & Which-Key
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub which_key_open: bool,

    // Syntax highlighters cache
    highlighters: HashMap<Language, SyntaxHighlighter>,
}

impl App {
    pub fn new(file_arg: Option<PathBuf>) -> Result<Self, std::io::Error> {
        let editor = if let Some(ref path) = file_arg {
            if path.exists() {
                Editor::open(path)?
            } else {
                let mut ed = Editor::new();
                ed.set_file_path(Some(path.clone()));
                ed
            }
        } else {
            Editor::new()
        };

        let (width, height) = terminal_size().unwrap_or((80, 24));

        Ok(Self {
            editor,
            scroll_row: 0,
            scroll_col: 0,
            terminal_width: width,
            terminal_height: height,
            status_message: None,
            running: true,
            show_line_numbers: true,

            mouse_dragging: false,
            last_click_time: Instant::now(),
            last_click_pos: (0, 0),
            click_count: 0,

            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            which_key_open: false,

            highlighters: HashMap::new(),
        })
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    fn text_area_top(&self) -> usize {
        1 // Row 0 is the Tab bar
    }

    fn text_area_height(&self) -> usize {
        // Row 0: Tab bar, Last row: Status bar
        self.terminal_height.saturating_sub(2) as usize
    }

    fn gutter_width(&self) -> usize {
        if !self.show_line_numbers {
            return 0;
        }
        let total_lines = self.editor.buffer().len_lines().max(1);
        let digits = format!("{}", total_lines).len();
        digits + 3 // digits + 1 space + '|' + 1 space
    }

    fn text_area_width(&self) -> usize {
        let gutter = self.gutter_width();
        (self.terminal_width as usize).saturating_sub(gutter)
    }

    fn adjust_viewport(&mut self) {
        let (cursor_row, cursor_col) = self.editor.cursor_line_col();
        let height = self.text_area_height().max(1);
        let width = self.text_area_width().max(1);

        if cursor_row < self.scroll_row {
            self.scroll_row = cursor_row;
        } else if cursor_row >= self.scroll_row + height {
            self.scroll_row = cursor_row.saturating_sub(height - 1);
        }

        if cursor_col < self.scroll_col {
            self.scroll_col = cursor_col;
        } else if cursor_col >= self.scroll_col + width {
            self.scroll_col = cursor_col.saturating_sub(width - 1);
        }
    }

    fn current_language(&self) -> Option<Language> {
        self.editor.file_path().and_then(Language::from_path)
    }

    fn compute_highlights(
        &mut self,
        start_byte: usize,
        end_byte: usize,
        text: &str,
    ) -> Vec<HighlightToken> {
        if let Some(lang) = self.current_language() {
            let highlighter = self
                .highlighters
                .entry(lang)
                .or_insert_with(|| SyntaxHighlighter::new(lang).unwrap());
            highlighter.update(text);
            highlighter.highlight_range(start_byte, end_byte, text)
        } else {
            Vec::new()
        }
    }

    pub fn render<W: Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.adjust_viewport();

        let gutter_w = self.gutter_width();
        let text_h = self.text_area_height();
        let text_w = self.text_area_width();
        let total_lines = self.editor.buffer().len_lines();
        let active_tab_idx = self.editor.active_tab_idx();

        queue!(writer, MoveTo(0, 0), Clear(ClearType::All))?;

        // 1. Render Tab Bar (Row 0)
        queue!(
            writer,
            MoveTo(0, 0),
            SetBackgroundColor(Color::Rgb {
                r: 30,
                g: 30,
                b: 30
            }),
            SetForegroundColor(Color::White)
        )?;

        let mut tab_header = String::new();
        for (i, tab) in self.editor.tabs().iter().enumerate() {
            let dirty = if tab.is_dirty { "*" } else { "" };
            let is_active = i == active_tab_idx;
            if is_active {
                tab_header.push_str(&format!(" [ {}: {}{} x ] ", i + 1, tab.name(), dirty));
            } else {
                tab_header.push_str(&format!("  {}: {}{} x   ", i + 1, tab.name(), dirty));
            }
        }
        tab_header.push_str(" [ + ]");

        let width = self.terminal_width as usize;
        let tab_bar_fill = format!("{:width$}", tab_header, width = width);
        let tab_bar_trimmed: String = tab_bar_fill.chars().take(width).collect();
        queue!(writer, Print(tab_bar_trimmed), ResetColor)?;

        // 2. Syntax tokens for the visible range
        let full_text = self
            .editor
            .buffer()
            .slice_to_cow(0, self.editor.buffer().len_bytes())
            .to_string();
        let start_byte = self.editor.buffer().line_to_byte(self.scroll_row);
        let end_line = (self.scroll_row + text_h).min(total_lines);
        let end_byte = if end_line < total_lines {
            self.editor.buffer().line_to_byte(end_line)
        } else {
            self.editor.buffer().len_bytes()
        };

        let tokens = self.compute_highlights(start_byte, end_byte, &full_text);
        let primary_cursor = *self.editor.cursors().primary();
        let selection_range = if primary_cursor.is_selection() {
            Some(primary_cursor.min()..primary_cursor.max())
        } else {
            None
        };

        // 3. Render Text Area (Rows 1..1+text_h)
        for screen_row in 0..text_h {
            let buffer_line = self.scroll_row + screen_row;
            let term_row = (self.text_area_top() + screen_row) as u16;
            queue!(writer, MoveTo(0, term_row))?;

            if buffer_line < total_lines {
                // Line numbers gutter
                if self.show_line_numbers {
                    queue!(
                        writer,
                        SetForegroundColor(Color::DarkGrey),
                        Print(format!(
                            "{:>width$} │ ",
                            buffer_line + 1,
                            width = gutter_w - 3
                        )),
                        ResetColor
                    )?;
                }

                // Render characters with syntax and selection styling
                if let Some(line) = self.editor.buffer().line(buffer_line) {
                    let line_start_byte = self.editor.buffer().line_to_byte(buffer_line);
                    let mut line_str = line.to_string();
                    if line_str.ends_with('\n') {
                        line_str.pop();
                        if line_str.ends_with('\r') {
                            line_str.pop();
                        }
                    }

                    let chars: Vec<char> = line_str.chars().collect();
                    let mut current_byte = line_start_byte;

                    for (col_idx, &c) in chars.iter().enumerate() {
                        let char_len = c.len_utf8();
                        let char_byte = current_byte;
                        current_byte += char_len;

                        if col_idx < self.scroll_col || col_idx >= self.scroll_col + text_w {
                            continue;
                        }

                        // Determine selection background
                        let is_selected = selection_range
                            .as_ref()
                            .is_some_and(|r| r.contains(&char_byte));
                        if is_selected {
                            queue!(
                                writer,
                                SetBackgroundColor(Color::DarkBlue),
                                SetForegroundColor(Color::White)
                            )?;
                        } else {
                            // Syntax color
                            let token_type = tokens
                                .iter()
                                .find(|t| char_byte >= t.start_byte && char_byte < t.end_byte)
                                .map(|t| t.token_type);

                            let fg = match token_type {
                                Some(TokenType::Keyword) => Color::Cyan,
                                Some(TokenType::Function) => Color::Blue,
                                Some(TokenType::Type) => Color::Yellow,
                                Some(TokenType::String) => Color::Green,
                                Some(TokenType::Number) => Color::Magenta,
                                Some(TokenType::Comment) => Color::DarkGrey,
                                Some(TokenType::Macro) => Color::Red,
                                _ => Color::Reset,
                            };
                            queue!(writer, SetForegroundColor(fg))?;
                        }

                        queue!(writer, Print(c), ResetColor)?;
                    }
                }
            } else {
                if self.show_line_numbers {
                    queue!(
                        writer,
                        SetForegroundColor(Color::DarkGrey),
                        Print(format!("{:>width$} ~ ", "~", width = gutter_w - 3)),
                        ResetColor
                    )?;
                }
            }
        }

        // 4. Render Status Bar (Last Row)
        let status_row = self.terminal_height.saturating_sub(1);
        queue!(
            writer,
            MoveTo(0, status_row),
            SetBackgroundColor(Color::DarkBlue),
            SetForegroundColor(Color::White)
        )?;

        let (cursor_row, cursor_col) = self.editor.cursor_line_col();
        let file_name = self.editor.active_tab().name();
        let dirty_flag = if self.editor.is_dirty() { " [*]" } else { "" };
        let lang_str = self
            .current_language()
            .map(|l| match l {
                Language::C => "C",
                Language::Rust => "Rust",
                Language::Python => "Python",
            })
            .unwrap_or("Plain Text");

        let left_status = format!(
            " LUME │ Tab {}/{} │ {}{} │ Ln {}, Col {} │ {}",
            active_tab_idx + 1,
            self.editor.tabs().len(),
            file_name,
            dirty_flag,
            cursor_row + 1,
            cursor_col + 1,
            lang_str
        );

        let right_status = if let Some((ref msg, time)) = self.status_message {
            if time.elapsed() < Duration::from_secs(4) {
                format!("{} ", msg)
            } else {
                "F1: Shortcuts │ Ctrl+P: Palette │ Ctrl+S: Save │ Ctrl+Q: Quit ".to_string()
            }
        } else {
            "F1: Shortcuts │ Ctrl+P: Palette │ Ctrl+S: Save │ Ctrl+Q: Quit ".to_string()
        };

        let spaces = width.saturating_sub(left_status.len() + right_status.len());
        let bar = format!(
            "{}{:width$}{}",
            left_status,
            "",
            right_status,
            width = spaces
        );
        let bar_trimmed: String = bar.chars().take(width).collect();
        queue!(writer, Print(bar_trimmed), ResetColor)?;

        // 5. Render Command Palette Modal (if open)
        if self.palette_open {
            self.render_command_palette(writer)?;
        }

        // 6. Render Which-Key Modal (if open)
        if self.which_key_open {
            self.render_which_key(writer)?;
        }

        // Position terminal cursor
        if !self.palette_open && !self.which_key_open {
            let term_cursor_row =
                (self.text_area_top() + cursor_row.saturating_sub(self.scroll_row)) as u16;
            let term_cursor_col = (gutter_w + cursor_col.saturating_sub(self.scroll_col)) as u16;
            queue!(writer, MoveTo(term_cursor_col, term_cursor_row), Show)?;
        } else {
            queue!(writer, Hide)?;
        }

        writer.flush()?;
        Ok(())
    }

    fn filtered_commands(&self) -> Vec<&'static CommandItem> {
        if self.palette_query.is_empty() {
            COMMANDS.iter().collect()
        } else {
            let q = self.palette_query.to_lowercase();
            COMMANDS
                .iter()
                .filter(|c| {
                    c.title.to_lowercase().contains(&q) || c.shortcut.to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    fn render_command_palette<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let box_w = (self.terminal_width as usize).clamp(30, 64);
        let box_h = 10;
        let start_x = (self.terminal_width as usize).saturating_sub(box_w) / 2;
        let start_y = (self.terminal_height as usize).saturating_sub(box_h) / 3;

        let commands = self.filtered_commands();

        // Background box
        for y in 0..box_h {
            queue!(
                writer,
                MoveTo(start_x as u16, (start_y + y) as u16),
                SetBackgroundColor(Color::Rgb {
                    r: 20,
                    g: 25,
                    b: 35
                }),
                SetForegroundColor(Color::White)
            )?;

            if y == 0 {
                let title = " Command Palette (Esc to close) ";
                let fill = box_w.saturating_sub(title.len());
                queue!(
                    writer,
                    SetForegroundColor(Color::Cyan),
                    Print(format!("{}{:fill$}", title, "", fill = fill))
                )?;
            } else if y == 1 {
                let prompt = format!(" > {}█", self.palette_query);
                let fill = box_w.saturating_sub(prompt.len());
                queue!(
                    writer,
                    SetForegroundColor(Color::White),
                    Print(format!("{}{:fill$}", prompt, "", fill = fill))
                )?;
            } else if y == 2 {
                queue!(
                    writer,
                    SetForegroundColor(Color::DarkGrey),
                    Print("─".repeat(box_w))
                )?;
            } else {
                let item_idx = y - 3;
                if item_idx < commands.len() {
                    let item = commands[item_idx];
                    let is_sel = item_idx == self.palette_selected;
                    let prefix = if is_sel { " > " } else { "   " };
                    let title = item.title;
                    let shortcut = item.shortcut;

                    let avail = box_w.saturating_sub(prefix.len() + shortcut.len() + 2);
                    let title_fit: String = title.chars().take(avail).collect();
                    let spaces =
                        box_w.saturating_sub(prefix.len() + title_fit.len() + shortcut.len() + 1);

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
                                b: 35
                            }),
                            SetForegroundColor(Color::White)
                        )?;
                    }

                    queue!(
                        writer,
                        Print(format!(
                            "{}{}{:spaces$}{}",
                            prefix,
                            title_fit,
                            "",
                            shortcut,
                            spaces = spaces
                        ))
                    )?;
                } else {
                    queue!(writer, Print(" ".repeat(box_w)))?;
                }
            }
        }

        queue!(writer, ResetColor)?;
        Ok(())
    }

    fn render_which_key<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let box_w = (self.terminal_width as usize).clamp(40, 76);
        let box_h = 8;
        let start_x = (self.terminal_width as usize).saturating_sub(box_w) / 2;
        let start_y = (self.terminal_height as usize).saturating_sub(box_h) / 2;

        let lines = [
            " LUME WHICH-KEY SHORTCUTS (Press any key or Esc to close) ",
            " [File]      Ctrl+S: Save   │ Ctrl+T: New Tab   │ Ctrl+W: Close Tab ",
            " [Nav]       Ctrl+Tab / Alt+Right: Next Tab     │ Alt+Left: Prev Tab ",
            " [Edit]      Ctrl+Z: Undo   │ Ctrl+Y: Redo      │ Backspace / Del ",
            " [Cursor]    Arrows: Move   │ Shift+Arrows: Select │ Home / End ",
            " [Mouse]     Click: Position │ 2xClick: Word │ 3xClick: Line │ Drag: Sel ",
            " [Features]  Ctrl+P: Command Palette │ F1: Which-Key Overlay ",
            " [Quit]      Ctrl+Q: Exit Lume ",
        ];

        for (i, line) in lines.iter().enumerate() {
            queue!(
                writer,
                MoveTo(start_x as u16, (start_y + i) as u16),
                SetBackgroundColor(Color::Rgb {
                    r: 25,
                    g: 30,
                    b: 45
                }),
                SetForegroundColor(if i == 0 { Color::Yellow } else { Color::White })
            )?;
            let fill = box_w.saturating_sub(line.len());
            queue!(writer, Print(format!("{}{:fill$}", line, "", fill = fill)))?;
        }

        queue!(writer, ResetColor)?;
        Ok(())
    }

    pub fn execute_command(&mut self, id: &str) {
        match id {
            "file.save" => match self.editor.save() {
                Ok(()) => self.set_status("File saved successfully"),
                Err(e) => self.set_status(format!("Save error: {}", e)),
            },
            "file.new_tab" => {
                let idx = self.editor.new_tab();
                self.set_status(format!("Opened new Tab {}", idx + 1));
            }
            "file.close_tab" => {
                let idx = self.editor.active_tab_idx();
                self.editor.close_tab(idx);
                self.set_status("Closed tab");
            }
            "file.next_tab" => self.editor.next_tab(),
            "file.prev_tab" => self.editor.prev_tab(),
            "edit.undo" => {
                if self.editor.undo() {
                    self.set_status("Undo");
                }
            }
            "edit.redo" => {
                if self.editor.redo() {
                    self.set_status("Redo");
                }
            }
            "view.which_key" => {
                self.which_key_open = true;
            }
            "view.toggle_line_numbers" => {
                self.show_line_numbers = !self.show_line_numbers;
                self.set_status(format!(
                    "Line numbers: {}",
                    if self.show_line_numbers { "ON" } else { "OFF" }
                ));
            }
            "app.quit" => {
                self.running = false;
            }
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> std::io::Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // If Which-Key overlay is open, any key dismisses it
        if self.which_key_open {
            self.which_key_open = false;
            if key.code == KeyCode::Esc || key.code == KeyCode::F(1) {
                return Ok(());
            }
        }

        // If Command Palette is open, route keys to palette
        if self.palette_open {
            match key.code {
                KeyCode::Esc => {
                    self.palette_open = false;
                }
                KeyCode::Up => {
                    if self.palette_selected > 0 {
                        self.palette_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    let total = self.filtered_commands().len();
                    if total > 0 && self.palette_selected + 1 < total {
                        self.palette_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let commands = self.filtered_commands();
                    if let Some(cmd) = commands.get(self.palette_selected) {
                        let id = cmd.id;
                        self.palette_open = false;
                        self.execute_command(id);
                    } else {
                        self.palette_open = false;
                    }
                }
                KeyCode::Backspace => {
                    self.palette_query.pop();
                    self.palette_selected = 0;
                }
                KeyCode::Char(c) if !ctrl && !alt => {
                    self.palette_query.push(c);
                    self.palette_selected = 0;
                }
                _ => {}
            }
            return Ok(());
        }

        // Normal Editor key handling
        match (key.code, ctrl, alt) {
            (KeyCode::Char('q'), true, false) => {
                self.running = false;
            }
            (KeyCode::Char('p'), true, false) => {
                self.palette_open = true;
                self.palette_query.clear();
                self.palette_selected = 0;
            }
            (KeyCode::F(1), false, false) => {
                self.which_key_open = true;
            }
            (KeyCode::Char('s'), true, false) => {
                self.execute_command("file.save");
            }
            (KeyCode::Char('t'), true, false) => {
                self.execute_command("file.new_tab");
            }
            (KeyCode::Char('w'), true, false) => {
                self.execute_command("file.close_tab");
            }
            (KeyCode::Tab, true, false) | (KeyCode::Right, false, true) => {
                self.editor.next_tab();
            }
            (KeyCode::Left, false, true) => {
                self.editor.prev_tab();
            }
            (KeyCode::Char('z'), true, false) => {
                self.execute_command("edit.undo");
            }
            (KeyCode::Char('y'), true, false) => {
                self.execute_command("edit.redo");
            }
            (KeyCode::Left, false, false) => self.editor.move_left(shift),
            (KeyCode::Right, false, false) => self.editor.move_right(shift),
            (KeyCode::Up, false, false) => self.editor.move_up(shift),
            (KeyCode::Down, false, false) => self.editor.move_down(shift),
            (KeyCode::Home, false, false) => self.editor.move_home(shift),
            (KeyCode::End, false, false) => self.editor.move_end(shift),
            (KeyCode::Backspace, false, false) => {
                let _ = self.editor.delete_backwards();
            }
            (KeyCode::Delete, false, false) => {
                let _ = self.editor.delete_forwards();
            }
            (KeyCode::Enter, false, false) => {
                let _ = self.editor.insert_newline();
            }
            (KeyCode::Tab, false, false) => {
                let _ = self.editor.insert_str("    ");
            }
            (KeyCode::Char(c), false, false) => {
                let _ = self.editor.insert_char(c);
            }
            _ => {}
        }

        Ok(())
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> std::io::Result<()> {
        let col = mouse.column as usize;
        let row = mouse.row as usize;
        let gutter_w = self.gutter_width();
        let status_row = self.terminal_height.saturating_sub(1) as usize;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Click in Tab Bar (Row 0)
                if row == 0 {
                    self.handle_tab_bar_click(col);
                    return Ok(());
                }

                // Click in text editing area
                if row >= self.text_area_top() && row < status_row {
                    let buffer_line = self.scroll_row + (row - self.text_area_top());
                    let buffer_col = self.scroll_col + col.saturating_sub(gutter_w);

                    let now = Instant::now();
                    if now.duration_since(self.last_click_time) < Duration::from_millis(400)
                        && self.last_click_pos == (mouse.column, mouse.row)
                    {
                        self.click_count = self.click_count.saturating_add(1);
                    } else {
                        self.click_count = 1;
                    }
                    self.last_click_time = now;
                    self.last_click_pos = (mouse.column, mouse.row);

                    if self.click_count == 2 {
                        self.editor
                            .set_cursor_from_line_col(buffer_line, buffer_col, false);
                        self.editor.select_word_at_cursor();
                        self.mouse_dragging = false;
                    } else if self.click_count >= 3 {
                        self.editor
                            .set_cursor_from_line_col(buffer_line, buffer_col, false);
                        self.editor.select_line_at_cursor();
                        self.mouse_dragging = false;
                    } else {
                        self.editor
                            .set_cursor_from_line_col(buffer_line, buffer_col, false);
                        self.mouse_dragging = true;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.mouse_dragging && row >= self.text_area_top() && row < status_row {
                    let buffer_line = self.scroll_row + (row - self.text_area_top());
                    let buffer_col = self.scroll_col + col.saturating_sub(gutter_w);
                    self.editor
                        .set_cursor_from_line_col(buffer_line, buffer_col, true);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_dragging = false;
            }
            MouseEventKind::ScrollUp => {
                self.scroll_row = self.scroll_row.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                let max_lines = self.editor.buffer().len_lines().saturating_sub(1);
                self.scroll_row = (self.scroll_row + 3).min(max_lines);
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_tab_bar_click(&mut self, col: usize) {
        let mut current_col = 0;
        for (i, tab) in self.editor.tabs().iter().enumerate() {
            let dirty = if tab.is_dirty { "*" } else { "" };
            let is_active = i == self.editor.active_tab_idx();
            let tab_text = if is_active {
                format!(" [ {}: {}{} x ] ", i + 1, tab.name(), dirty)
            } else {
                format!("  {}: {}{} x   ", i + 1, tab.name(), dirty)
            };

            let tab_len = tab_text.len();
            if col >= current_col && col < current_col + tab_len {
                // Check if clicked the 'x' button inside tab
                let close_button_offset = tab_text.find('x').unwrap_or(tab_len);
                if col >= current_col + close_button_offset - 1
                    && col <= current_col + close_button_offset + 1
                {
                    self.editor.close_tab(i);
                } else {
                    self.editor.switch_tab(i);
                }
                return;
            }
            current_col += tab_len;
        }

        // Check if clicked the '[ + ]' button
        if col >= current_col && col < current_col + 6 {
            self.editor.new_tab();
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("lume 0.1.0 (M1 functional MVP)");
        return Ok(());
    }

    let file_arg = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(PathBuf::from);

    if args.iter().any(|a| a == "--bench-startup") {
        let start = Instant::now();
        let mut app = App::new(file_arg)?;
        let mut sink = std::io::sink();
        app.render(&mut sink)?;
        let duration = start.elapsed();
        println!("Cold startup time: {:.2?}", duration);
        return Ok(());
    }

    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = stdout();
        let _ = execute!(
            out,
            Show,
            ResetColor,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        default_panic(info);
    }));

    let _guard = TerminalGuard::init()?;
    let mut app = App::new(file_arg)?;

    let (tx, rx) = crossbeam_channel::unbounded();

    let input_tx = tx.clone();
    std::thread::spawn(move || loop {
        let app_event = match event::read() {
            Ok(Event::Key(key)) => AppEvent::Input(key),
            Ok(Event::Mouse(mouse)) => AppEvent::Mouse(mouse),
            Ok(Event::Resize(w, h)) => AppEvent::Resize(w, h),
            _ => continue,
        };
        if input_tx.send(app_event).is_err() {
            break;
        }
    });

    let mut out = BufWriter::new(stdout());
    app.render(&mut out)?;

    while app.running {
        match rx.recv_timeout(Duration::from_millis(16)) {
            Ok(AppEvent::Input(key)) => {
                app.handle_key(key)?;
                app.render(&mut out)?;
            }
            Ok(AppEvent::Mouse(mouse)) => {
                app.handle_mouse(mouse)?;
                app.render(&mut out)?;
            }
            Ok(AppEvent::Resize(w, h)) => {
                app.terminal_width = w;
                app.terminal_height = h;
                app.render(&mut out)?;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // Autosave check on idle tick
                if app.editor.check_autosave() {
                    app.set_status("Autosaved");
                    app.render(&mut out)?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation_and_command_palette() {
        let mut app = App::new(None).unwrap();
        assert_eq!(app.editor.tabs().len(), 1);
        assert!(!app.palette_open);

        // Open palette
        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL))
            .unwrap();
        assert!(app.palette_open);

        // Type query 'save'
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE))
            .unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
            .unwrap();

        let filtered = app.filtered_commands();
        assert!(!filtered.is_empty());
        assert_eq!(filtered[0].id, "file.save");

        // Esc closes palette
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.palette_open);
    }

    #[test]
    fn test_app_which_key_toggle() {
        let mut app = App::new(None).unwrap();
        assert!(!app.which_key_open);

        // F1 opens which-key
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE))
            .unwrap();
        assert!(app.which_key_open);

        // Any key dismisses it
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!app.which_key_open);
    }

    #[test]
    fn test_app_mouse_tab_switching() {
        let mut app = App::new(None).unwrap();
        app.editor.insert_str("tab 1").unwrap();
        app.editor.new_tab();
        app.editor.insert_str("tab 2").unwrap();

        assert_eq!(app.editor.active_tab_idx(), 1);

        // Click on Tab 1 in tab bar (row 0, col 5)
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();

        assert_eq!(app.editor.active_tab_idx(), 0);
    }
}
