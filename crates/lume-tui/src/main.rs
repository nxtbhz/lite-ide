mod args;
mod explorer;
mod prompt;

use args::{parse_cli_args, print_help, CliCommand, CliTargets};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{
        disable_raw_mode, enable_raw_mode, size as terminal_size, Clear, ClearType,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use explorer::FileExplorer;
use lume_core::{Editor, TextBuffer};
use lume_syntax::{HighlightToken, Language, SyntaxRequest, SyntaxWorker, TokenType};
use prompt::{InputPrompt, PromptAction};
use std::collections::HashMap;
use std::env;
use std::io::{stdout, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

enum AppEvent {
    Input(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    FocusLost,
}

struct TerminalGuard;

impl TerminalGuard {
    fn init() -> Result<Self, std::io::Error> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(
            out,
            EnterAlternateScreen,
            Hide,
            EnableMouseCapture,
            EnableFocusChange
        )?;
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
            DisableFocusChange,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveFocus {
    Editor,
    Explorer,
}

#[derive(Clone, Debug)]
struct CommandItem {
    id: &'static str,
    title: &'static str,
    shortcut: &'static str,
}

const COMMANDS: &[CommandItem] = &[
    CommandItem {
        id: "file.open",
        title: "File: Open File...",
        shortcut: "Ctrl+O",
    },
    CommandItem {
        id: "file.save",
        title: "File: Save",
        shortcut: "Ctrl+S",
    },
    CommandItem {
        id: "file.save_as",
        title: "File: Save As...",
        shortcut: "Ctrl+Shift+S",
    },
    CommandItem {
        id: "view.toggle_explorer",
        title: "View: Toggle File Explorer",
        shortcut: "Ctrl+B / F2",
    },
    CommandItem {
        id: "language.set_mode",
        title: "Language: Change Language Mode",
        shortcut: "Ctrl+K M",
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
    pub explorer: FileExplorer,
    pub focus: ActiveFocus,
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

    // Overlays: Command Palette, Which-Key & Input Prompt
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub which_key_open: bool,
    pub prompt: Option<InputPrompt>,

    // Syntax worker & cache
    pub syntax_worker: SyntaxWorker,
    pub syntax_cache: HashMap<usize, (u64, Arc<Vec<HighlightToken>>)>,
    pub syntax_in_flight: HashMap<usize, u64>,
}

impl App {
    pub fn new(file_arg: Option<PathBuf>) -> Result<Self, std::io::Error> {
        let targets = match file_arg {
            Some(path) => {
                if path.is_dir() {
                    CliTargets {
                        workspace: Some(path),
                        files: Vec::new(),
                    }
                } else {
                    CliTargets {
                        workspace: None,
                        files: vec![path],
                    }
                }
            }
            None => CliTargets::default(),
        };
        Self::open_targets(targets)
    }

    pub fn from_paths<P: AsRef<std::path::Path>>(paths: &[P]) -> Result<Self, std::io::Error> {
        let str_args: Vec<String> = std::iter::once("lume".to_string())
            .chain(
                paths
                    .iter()
                    .map(|p| p.as_ref().to_string_lossy().to_string()),
            )
            .collect();
        match parse_cli_args(&str_args) {
            CliCommand::Run(targets) | CliCommand::BenchStartup(targets) => {
                Self::open_targets(targets)
            }
            _ => Self::open_targets(CliTargets::default()),
        }
    }

    pub fn open_targets(targets: CliTargets) -> Result<Self, std::io::Error> {
        let initial_cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let workspace_root = if let Some(ref ws) = targets.workspace {
            let resolved = if ws.is_absolute() {
                ws.clone()
            } else {
                initial_cwd.join(ws)
            };
            std::fs::canonicalize(&resolved).unwrap_or(resolved)
        } else {
            std::fs::canonicalize(&initial_cwd).unwrap_or(initial_cwd.clone())
        };

        let mut resolved_files = Vec::new();
        for file in targets.files {
            if file.is_absolute() {
                resolved_files.push(file);
            } else if initial_cwd.join(&file).exists() {
                resolved_files.push(initial_cwd.join(&file));
            } else if workspace_root.join(&file).exists() || targets.workspace.is_some() {
                resolved_files.push(workspace_root.join(&file));
            } else {
                resolved_files.push(initial_cwd.join(&file));
            }
        }

        let mut editor = Editor::new();
        let mut initial_status = None;

        if !resolved_files.is_empty() {
            let mut first = true;
            for file_path in &resolved_files {
                if file_path.is_file() {
                    if first {
                        match editor.open_in_current_tab(file_path) {
                            Ok(()) => {}
                            Err(e) => {
                                initial_status =
                                    Some(format!("Error opening {}: {}", file_path.display(), e));
                                editor.set_file_path(Some(file_path.clone()));
                            }
                        }
                        first = false;
                    } else {
                        match editor.open_tab(file_path) {
                            Ok(_) => {}
                            Err(e) => {
                                initial_status =
                                    Some(format!("Error opening {}: {}", file_path.display(), e));
                                let _ = editor.new_tab();
                                editor.set_file_path(Some(file_path.clone()));
                            }
                        }
                    }
                } else if !file_path.exists() {
                    if first {
                        editor.set_file_path(Some(file_path.clone()));
                        first = false;
                    } else {
                        let _ = editor.new_tab();
                        editor.set_file_path(Some(file_path.clone()));
                    }
                }
            }
            editor.switch_tab(0);
        }

        let explorer = FileExplorer::new(workspace_root.clone());
        let (width, height) = terminal_size().unwrap_or((80, 24));

        let mut app = Self {
            editor,
            explorer,
            focus: ActiveFocus::Editor,
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
            prompt: None,

            syntax_worker: SyntaxWorker::spawn(),
            syntax_cache: HashMap::new(),
            syntax_in_flight: HashMap::new(),
        };

        if let Some(msg) = initial_status {
            app.set_status(msg);
        } else if resolved_files.len() == 1 {
            if resolved_files[0].exists() {
                app.set_status(format!("Opened {}", resolved_files[0].display()));
            } else {
                app.set_status(format!("New file: {}", resolved_files[0].display()));
            }
        } else if resolved_files.len() > 1 {
            app.set_status(format!("Opened {} files", resolved_files.len()));
        } else if targets.workspace.is_some() {
            app.set_status(format!("Workspace: {}", workspace_root.display()));
        }

        Ok(app)
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn explorer_width(&self) -> usize {
        if self.explorer.is_visible {
            self.explorer.width
        } else {
            0
        }
    }

    pub fn text_area_left(&self) -> usize {
        self.explorer_width()
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
        digits + 3 // digits + 1 space + '│' + 1 space
    }

    fn text_area_width(&self) -> usize {
        let gutter = self.gutter_width();
        let exp_w = self.explorer_width();
        (self.terminal_width as usize).saturating_sub(gutter + exp_w)
    }

    pub fn adjust_viewport(&mut self) {
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

    pub fn current_language(&self) -> Option<Language> {
        if let Some(override_id) = self.editor.language_override() {
            Language::from_id(override_id)
        } else {
            self.editor.file_path().and_then(Language::from_path)
        }
    }

    pub fn has_pending_syntax_response(&mut self) -> bool {
        let mut updated = false;
        while let Some(res) = self.syntax_worker.try_recv_response() {
            let is_newer = self
                .syntax_cache
                .get(&res.buffer_id)
                .map(|(rev, _)| res.revision >= *rev)
                .unwrap_or(true);
            if is_newer {
                self.syntax_cache
                    .insert(res.buffer_id, (res.revision, Arc::new(res.tokens)));
                self.syntax_in_flight.remove(&res.buffer_id);
                if res.buffer_id == self.editor.active_tab().id {
                    updated = true;
                }
            }
        }
        updated
    }

    pub fn poll_async_saves(&mut self) -> bool {
        let events = self.editor.poll_async_saves();
        let mut updated = false;
        for ev in events {
            updated = true;
            match ev.result {
                Ok(()) => {
                    self.set_status("Autosaved");
                }
                Err(err) => {
                    self.set_status(format!("Save error: {}", err));
                }
            }
        }
        updated
    }

    fn update_syntax_highlights(&mut self) -> Arc<Vec<HighlightToken>> {
        let _ = self.has_pending_syntax_response();

        let active_tab = self.editor.active_tab();
        let tab_id = active_tab.id;
        let tab_rev = active_tab.revision;

        let needs_request = match self.syntax_cache.get(&tab_id) {
            Some((cached_rev, _)) => *cached_rev != tab_rev,
            None => true,
        };

        if needs_request && self.syntax_in_flight.get(&tab_id) != Some(&tab_rev) {
            if let Some(lang) = self.current_language() {
                let full_text = self.editor.buffer().rope().to_string();
                let full_len = full_text.len();
                self.syntax_in_flight.insert(tab_id, tab_rev);
                self.syntax_worker.send_request(SyntaxRequest {
                    buffer_id: tab_id,
                    revision: tab_rev,
                    language: lang,
                    text: full_text,
                    visible_range: 0..full_len,
                });
            }
        }

        self.syntax_cache
            .get(&tab_id)
            .map(|(_, tokens)| Arc::clone(tokens))
            .unwrap_or_else(|| Arc::new(Vec::new()))
    }

    pub fn render<W: Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        let gutter_w = self.gutter_width();
        let text_h = self.text_area_height();
        let text_w = self.text_area_width();
        let text_left = self.text_area_left();
        let total_lines = self.editor.buffer().len_lines();
        let active_tab_idx = self.editor.active_tab_idx();

        // Begin synchronized frame update to eliminate all screen tearing and flickering
        writer.write_all(b"\x1b[?2026h")?;
        // Keep terminal cursor hidden during entire painting phase to avoid cursor teleporting across cells
        queue!(writer, Hide, MoveTo(0, 0))?;

        // 1. Render File Explorer (if visible)
        if self.explorer.is_visible {
            let is_exp_focused = self.focus == ActiveFocus::Explorer;
            self.explorer.render(
                writer,
                0,
                self.terminal_height.saturating_sub(1) as usize,
                is_exp_focused,
            )?;
        }

        // 2. Render Tab Bar (Row 0, starting from text_left)
        queue!(
            writer,
            MoveTo(text_left as u16, 0),
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

        let tab_bar_w = (self.terminal_width as usize).saturating_sub(text_left);
        let tab_bar_fill = format!("{:width$}", tab_header, width = tab_bar_w);
        let tab_bar_trimmed: String = tab_bar_fill.chars().take(tab_bar_w).collect();
        queue!(writer, Print(tab_bar_trimmed), ResetColor)?;

        // 3. Syntax tokens from background worker cache (re-parsed asynchronously off UI thread)
        let tokens = self.update_syntax_highlights();
        let primary_cursor = *self.editor.cursors().primary();
        let selection_range = if primary_cursor.is_selection() {
            Some(primary_cursor.min()..primary_cursor.max())
        } else {
            None
        };

        // 4. Render Text Area (Rows 1..1+text_h)
        for screen_row in 0..text_h {
            let buffer_line = self.scroll_row + screen_row;
            let term_row = (self.text_area_top() + screen_row) as u16;
            queue!(writer, MoveTo(text_left as u16, term_row))?;

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

                    let line_end_byte = line_start_byte + line_str.len();
                    let start_tok = tokens.partition_point(|t| t.end_byte <= line_start_byte);
                    let rest_tokens = &tokens[start_tok..];
                    let end_tok = rest_tokens.partition_point(|t| t.start_byte < line_end_byte);
                    let line_tokens = &rest_tokens[..end_tok];

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
                            // Syntax color via binary search on current line's tokens
                            let token_type = line_tokens
                                .binary_search_by(|t| {
                                    if char_byte < t.start_byte {
                                        std::cmp::Ordering::Greater
                                    } else if char_byte >= t.end_byte {
                                        std::cmp::Ordering::Less
                                    } else {
                                        std::cmp::Ordering::Equal
                                    }
                                })
                                .ok()
                                .map(|idx| line_tokens[idx].token_type);

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
            } else if self.show_line_numbers {
                queue!(
                    writer,
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("{:>width$} ~ ", "~", width = gutter_w - 3)),
                    ResetColor
                )?;
            }

            queue!(writer, Clear(ClearType::UntilNewLine))?;
        }

        // 5. Render Status Bar (Last Row)
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
            .map(|l| l.name())
            .unwrap_or("Plain Text");

        let exp_indicator = if self.explorer.is_visible {
            "[Explorer ON] "
        } else {
            "[Explorer OFF] "
        };

        let left_status = format!(
            " LUME │ {}{} │ Tab {}/{} │ {}{} │ Ln {}, Col {} │ {}",
            exp_indicator,
            if self.focus == ActiveFocus::Explorer {
                "(Explorer Active)"
            } else {
                "(Editor)"
            },
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
                "F2: Explorer │ Ctrl+O: Open │ Ctrl+S: Save │ Ctrl+P: Palette │ F1: Help "
                    .to_string()
            }
        } else {
            "F2: Explorer │ Ctrl+O: Open │ Ctrl+S: Save │ Ctrl+P: Palette │ F1: Help ".to_string()
        };

        let width = self.terminal_width as usize;
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

        // 6. Overlays
        if let Some(ref prompt) = self.prompt {
            prompt.render(writer, self.terminal_width, self.terminal_height)?;
        } else if self.palette_open {
            self.render_command_palette(writer)?;
        } else if self.which_key_open {
            self.render_which_key(writer)?;
        }

        // 7. Position terminal cursor
        if self.prompt.is_none() && !self.palette_open && !self.which_key_open {
            if self.focus == ActiveFocus::Editor {
                let in_view = cursor_row >= self.scroll_row
                    && cursor_row < self.scroll_row + text_h
                    && cursor_col >= self.scroll_col
                    && cursor_col < self.scroll_col + text_w;

                if in_view {
                    let term_cursor_row =
                        (self.text_area_top() + cursor_row - self.scroll_row) as u16;
                    let term_cursor_col =
                        (text_left + gutter_w + cursor_col - self.scroll_col) as u16;
                    queue!(writer, MoveTo(term_cursor_col, term_cursor_row), Show)?;
                } else {
                    queue!(writer, Hide)?;
                }
            } else {
                // Focus in explorer: hide terminal cursor, row selection handles highlight
                queue!(writer, Hide)?;
            }
        }

        // End synchronized frame update
        writer.write_all(b"\x1b[?2026l")?;
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
        let box_w = (self.terminal_width as usize).clamp(36, 68);
        let box_h = 11;
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
        let box_w = (self.terminal_width as usize).clamp(44, 80);
        let box_h = 9;
        let start_x = (self.terminal_width as usize).saturating_sub(box_w) / 2;
        let start_y = (self.terminal_height as usize).saturating_sub(box_h) / 2;

        let lines = [
            " LUME SHORTCUTS CHEAT SHEET (Press any key or Esc to close) ",
            " [File]      Ctrl+O: Open   │ Ctrl+S: Save      │ Ctrl+Shift+S: Save As ",
            " [Tabs]      Ctrl+T: New    │ Ctrl+W: Close     │ Ctrl+Tab / Alt+Left/Right: Switch ",
            " [Explorer]  Ctrl+B / F2: Toggle Explorer       │ Up/Down/Enter/Left/Right: Nav ",
            " [Edit]      Ctrl+Z: Undo   │ Ctrl+Y: Redo      │ Backspace / Del ",
            " [Cursor]    Arrows: Move   │ Shift+Arrows: Select │ Home / End ",
            " [Mouse]     Click: Position/Open │ 2xClick: Word │ 3xClick: Line │ Drag: Sel ",
            " [Features]  Ctrl+P: Command Palette            │ F1: Which-Key Overlay ",
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
            "file.open" => {
                self.prompt = Some(InputPrompt::open_file());
            }
            "file.save" => {
                if self.editor.file_path().is_some() {
                    match self.editor.save() {
                        Ok(()) => self.set_status("File saved successfully"),
                        Err(e) => self.set_status(format!("Save error: {}", e)),
                    }
                } else {
                    self.prompt = Some(InputPrompt::save_as(None));
                }
            }
            "file.save_as" => {
                let default_name = self
                    .editor
                    .file_path()
                    .map(|p| p.to_string_lossy().to_string());
                self.prompt = Some(InputPrompt::save_as(default_name.as_deref()));
            }
            "view.toggle_explorer" => {
                self.explorer.toggle_visible();
                if self.explorer.is_visible {
                    self.focus = ActiveFocus::Explorer;
                } else {
                    self.focus = ActiveFocus::Editor;
                }
                self.set_status(format!(
                    "File Explorer: {}",
                    if self.explorer.is_visible {
                        "OPEN"
                    } else {
                        "CLOSED"
                    }
                ));
            }
            "language.set_mode" => {
                let langs: Vec<&'static str> = Language::all().iter().map(|l| l.name()).collect();
                self.prompt = Some(InputPrompt::set_language(&langs));
            }
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
        self.adjust_viewport();
    }

    pub fn handle_deleted_folder_tabs(&mut self, folder_path: &std::path::Path) {
        let folder_canon = folder_path.canonicalize().unwrap_or_else(|_| folder_path.to_path_buf());
        for tab in self.editor.tabs_mut() {
            if let Some(ref p) = tab.file_path {
                let p_canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                if p_canon.starts_with(&folder_canon) {
                    let _ = lume_core::Journal::remove(p);
                    tab.file_path = None;
                    tab.is_dirty = true;
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> std::io::Result<()> {
        if key.kind == KeyEventKind::Release {
            return Ok(());
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // 1. If Which-Key overlay is open, any key dismisses it
        if self.which_key_open {
            self.which_key_open = false;
            if key.code == KeyCode::Esc || key.code == KeyCode::F(1) {
                return Ok(());
            }
        }

        // 2. If an Input Prompt is open (Open File, Save As, Language Mode)
        if let Some(mut prompt) = self.prompt.take() {
            match key.code {
                KeyCode::Esc => {
                    self.set_status("Cancelled");
                }
                KeyCode::Enter => {
                    let val = prompt.value.trim().to_string();
                    match prompt.action {
                        PromptAction::OpenFile => {
                            if !val.is_empty() {
                                let raw_p = PathBuf::from(&val);
                                let path = if raw_p.is_absolute() {
                                    raw_p
                                } else if self.explorer.root.join(&raw_p).exists() {
                                    self.explorer.root.join(&raw_p)
                                } else {
                                    raw_p
                                };
                                if path.exists() {
                                    match self.editor.open_or_load(&path) {
                                        Ok(_) => {
                                            self.focus = ActiveFocus::Editor;
                                            self.set_status(format!("Opened {}", path.display()));
                                        }
                                        Err(e) => self.set_status(format!("Open error: {}", e)),
                                    }
                                } else {
                                    self.set_status(format!("File not found: {}", path.display()));
                                }
                            }
                        }
                        PromptAction::SaveFileAs => {
                            if !val.is_empty() {
                                let raw_p = PathBuf::from(&val);
                                let path = if raw_p.is_absolute() {
                                    raw_p
                                } else {
                                    self.explorer.root.join(raw_p)
                                };
                                match self.editor.save_as(&path) {
                                    Ok(()) => {
                                        self.explorer.refresh();
                                        self.set_status(format!("Saved to {}", path.display()));
                                    }
                                    Err(e) => self.set_status(format!("Save error: {}", e)),
                                }
                            }
                        }
                        PromptAction::SetLanguage => {
                            let chosen = if !prompt.suggestions.is_empty()
                                && prompt.selected_suggestion < prompt.suggestions.len()
                                && (val.is_empty()
                                    || prompt.suggestions[prompt.selected_suggestion]
                                        .to_lowercase()
                                        .starts_with(&val.to_lowercase()))
                            {
                                prompt.suggestions[prompt.selected_suggestion].clone()
                            } else {
                                val.clone()
                            };

                            if let Some(lang) = Language::from_id(&chosen) {
                                self.editor
                                    .set_language_override(Some(lang.name().to_string()));
                                self.syntax_cache.remove(&self.editor.active_tab().id);
                                self.syntax_in_flight.remove(&self.editor.active_tab().id);
                                self.set_status(format!("Language mode: {}", lang.name()));
                            } else if chosen.to_lowercase() == "plain text" {
                                self.editor.set_language_override(None);
                                self.syntax_cache.remove(&self.editor.active_tab().id);
                                self.syntax_in_flight.remove(&self.editor.active_tab().id);
                                self.set_status("Language mode: Plain Text");
                            } else {
                                self.set_status(format!("Unknown language: {}", chosen));
                            }
                        }
                        PromptAction::CreateFolder { parent_dir } => {
                            if !val.is_empty() {
                                match self.explorer.create_folder(&parent_dir, &val) {
                                    Ok(created) => {
                                        self.set_status(format!("Created folder: {}", created.display()));
                                    }
                                    Err(e) => self.set_status(format!("Create folder error: {}", e)),
                                }
                            }
                        }
                        PromptAction::ConfirmDeleteFolder { path } => {
                            let should_delete = val.is_empty() || val.eq_ignore_ascii_case("y") || val.eq_ignore_ascii_case("yes");
                            if should_delete {
                                match self.explorer.delete_folder(&path) {
                                    Ok(deleted) => {
                                        self.handle_deleted_folder_tabs(&deleted);
                                        self.set_status(format!("Deleted folder: {}", deleted.display()));
                                    }
                                    Err(e) => self.set_status(format!("Delete error: {}", e)),
                                }
                            } else {
                                self.set_status("Deletion cancelled");
                            }
                        }
                    }
                }
                KeyCode::Tab => {
                    prompt.autocomplete();
                    self.prompt = Some(prompt);
                }
                KeyCode::Backspace => {
                    prompt.handle_backspace();
                    self.prompt = Some(prompt);
                }
                KeyCode::Left => {
                    prompt.move_left();
                    self.prompt = Some(prompt);
                }
                KeyCode::Right => {
                    prompt.move_right();
                    self.prompt = Some(prompt);
                }
                KeyCode::Up => {
                    if prompt.selected_suggestion > 0 {
                        prompt.selected_suggestion -= 1;
                    }
                    self.prompt = Some(prompt);
                }
                KeyCode::Down => {
                    if !prompt.suggestions.is_empty()
                        && prompt.selected_suggestion + 1 < prompt.suggestions.len()
                    {
                        prompt.selected_suggestion += 1;
                    }
                    self.prompt = Some(prompt);
                }
                KeyCode::Char(c) if !ctrl && !alt => {
                    prompt.handle_char(c);
                    self.prompt = Some(prompt);
                }
                _ => {
                    self.prompt = Some(prompt);
                }
            }
            return Ok(());
        }

        // 3. If Command Palette is open, route keys to palette
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

        // 4. Global Shortcuts
        if ctrl && !alt {
            match key.code {
                KeyCode::Char('q') => {
                    self.running = false;
                    return Ok(());
                }
                KeyCode::Char('p') => {
                    self.palette_open = true;
                    self.palette_query.clear();
                    self.palette_selected = 0;
                    return Ok(());
                }
                KeyCode::Char('o') => {
                    self.execute_command("file.open");
                    return Ok(());
                }
                KeyCode::Char('s') => {
                    if shift {
                        self.execute_command("file.save_as");
                    } else {
                        self.execute_command("file.save");
                    }
                    return Ok(());
                }
                KeyCode::Char('S') => {
                    self.execute_command("file.save_as");
                    return Ok(());
                }
                KeyCode::Char('b') | KeyCode::Char('e') => {
                    self.execute_command("view.toggle_explorer");
                    return Ok(());
                }
                KeyCode::Char('t') => {
                    self.execute_command("file.new_tab");
                    return Ok(());
                }
                KeyCode::Char('w') => {
                    self.execute_command("file.close_tab");
                    return Ok(());
                }
                KeyCode::Tab => {
                    self.editor.next_tab();
                    return Ok(());
                }
                KeyCode::Char('z') => {
                    self.execute_command("edit.undo");
                    return Ok(());
                }
                KeyCode::Char('y') => {
                    self.execute_command("edit.redo");
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::F(1) => {
                self.which_key_open = true;
                return Ok(());
            }
            KeyCode::F(2) => {
                self.execute_command("view.toggle_explorer");
                return Ok(());
            }
            KeyCode::Right if alt => {
                self.editor.next_tab();
                return Ok(());
            }
            KeyCode::Left if alt => {
                self.editor.prev_tab();
                return Ok(());
            }
            _ => {}
        }

        // 5. If File Explorer is Focused
        if self.focus == ActiveFocus::Explorer && self.explorer.is_visible {
            match key.code {
                KeyCode::Up => {
                    self.explorer.move_up();
                }
                KeyCode::Down => {
                    self.explorer.move_down(self.text_area_height());
                }
                KeyCode::PageUp => {
                    for _ in 0..self.text_area_height().max(1) {
                        self.explorer.move_up();
                    }
                }
                KeyCode::PageDown => {
                    for _ in 0..self.text_area_height().max(1) {
                        self.explorer.move_down(self.text_area_height());
                    }
                }
                KeyCode::Left => {
                    self.explorer.collapse_selected();
                }
                KeyCode::Right => {
                    self.explorer.expand_selected();
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some(path) = self.explorer.activate_selected() {
                        match self.editor.open_or_load(&path) {
                            Ok(_) => {
                                self.focus = ActiveFocus::Editor;
                                self.adjust_viewport();
                                self.set_status(format!("Opened {}", path.display()));
                            }
                            Err(e) => self.set_status(format!("Open error: {}", e)),
                        }
                    }
                }
                KeyCode::Tab | KeyCode::Esc => {
                    self.focus = ActiveFocus::Editor;
                }
                _ => {}
            }
            return Ok(());
        }

        // 6. Normal Editor key handling
        match (key.code, ctrl, alt) {
            (KeyCode::Left, false, false) => self.editor.move_left(shift),
            (KeyCode::Right, false, false) => self.editor.move_right(shift),
            (KeyCode::Up, false, false) => self.editor.move_up(shift),
            (KeyCode::Down, false, false) => self.editor.move_down(shift),
            (KeyCode::PageUp, false, false) => {
                let h = self.text_area_height().max(1);
                self.editor.page_up(h, shift);
            }
            (KeyCode::PageDown, false, false) => {
                let h = self.text_area_height().max(1);
                self.editor.page_down(h, shift);
            }
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

        self.adjust_viewport();
        Ok(())
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> std::io::Result<()> {
        let col = mouse.column as usize;
        let row = mouse.row as usize;
        let exp_w = self.explorer_width();
        let gutter_w = self.gutter_width();
        let status_row = self.terminal_height.saturating_sub(1) as usize;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Click in status bar (Row status_row)
                if row == status_row {
                    // Click on language area (Ln ..., Col ... | <Lang>)
                    self.execute_command("language.set_mode");
                    return Ok(());
                }

                // Click in Tab Bar (Row 0)
                if row == 0 {
                    if col < exp_w {
                        // Click in explorer header: toggle focus or refresh
                        self.focus = ActiveFocus::Explorer;
                        self.explorer.refresh();
                    } else {
                        self.focus = ActiveFocus::Editor;
                        self.handle_tab_bar_click(col.saturating_sub(exp_w));
                    }
                    return Ok(());
                }

                // Click in main body (Rows 1..status_row)
                if row >= self.text_area_top() && row < status_row {
                    if col < exp_w && self.explorer.is_visible {
                        // Click in File Explorer
                        self.focus = ActiveFocus::Explorer;
                        let rel_row = row - self.text_area_top();
                        if let Some(path) = self.explorer.click_at(rel_row) {
                            match self.editor.open_or_load(&path) {
                                Ok(_) => {
                                    self.focus = ActiveFocus::Editor;
                                    self.set_status(format!("Opened {}", path.display()));
                                }
                                Err(e) => self.set_status(format!("Open error: {}", e)),
                            }
                        }
                        return Ok(());
                    }

                    // Click in Editor text area
                    self.focus = ActiveFocus::Editor;
                    let text_left = self.text_area_left();
                    let buffer_line = self.scroll_row + (row - self.text_area_top());
                    let buffer_col = self
                        .scroll_col
                        .saturating_add(col.saturating_sub(text_left + gutter_w));

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
                    let text_left = self.text_area_left();
                    if col >= text_left {
                        let buffer_line = self.scroll_row + (row - self.text_area_top());
                        let buffer_col = self
                            .scroll_col
                            .saturating_add(col.saturating_sub(text_left + gutter_w));
                        self.editor
                            .set_cursor_from_line_col(buffer_line, buffer_col, true);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_dragging = false;
            }
            MouseEventKind::ScrollUp => {
                if col < exp_w && self.explorer.is_visible {
                    self.explorer.move_up();
                } else {
                    self.scroll_row = self.scroll_row.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if col < exp_w && self.explorer.is_visible {
                    self.explorer.move_down(self.text_area_height());
                } else {
                    let total_lines = self.editor.buffer().len_lines();
                    let max_lines = total_lines.saturating_sub(1);
                    self.scroll_row = (self.scroll_row + 3).min(max_lines);
                }
            }
            MouseEventKind::ScrollLeft => {
                self.scroll_col = self.scroll_col.saturating_sub(3);
            }
            MouseEventKind::ScrollRight => {
                self.scroll_col = self.scroll_col.saturating_add(3);
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
                // Check if clicked the 'x' button inside tab (always located 4 characters before the end)
                let close_button_offset = tab_len.saturating_sub(4);
                if col >= current_col + close_button_offset - 1
                    && col <= current_col + close_button_offset + 1
                {
                    self.editor.close_tab(i);
                } else {
                    self.editor.switch_tab(i);
                }
                self.adjust_viewport();
                return;
            }
            current_col += tab_len;
        }

        // Check if clicked the '[ + ]' button
        if col >= current_col && col < current_col + 6 {
            self.editor.new_tab();
            self.adjust_viewport();
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let cli_cmd = parse_cli_args(&args);

    let targets = match cli_cmd {
        CliCommand::Help => {
            print_help();
            return Ok(());
        }
        CliCommand::Version => {
            println!("lume 0.1.0 (M1 functional MVP)");
            return Ok(());
        }
        CliCommand::BenchStartup(targets) => {
            let start = Instant::now();
            let mut app = App::open_targets(targets)?;
            let mut sink = std::io::sink();
            app.render(&mut sink)?;
            let duration = start.elapsed();
            println!("Cold startup time: {:.2?}", duration);
            return Ok(());
        }
        CliCommand::Run(targets) => targets,
    };

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
    let mut app = App::open_targets(targets)?;

    let (tx, rx) = crossbeam_channel::unbounded();

    let input_tx = tx.clone();
    std::thread::spawn(move || loop {
        let app_event = match event::read() {
            Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => AppEvent::Input(key),
            Ok(Event::Mouse(mouse)) => AppEvent::Mouse(mouse),
            Ok(Event::Resize(w, h)) => AppEvent::Resize(w, h),
            Ok(Event::FocusLost) => AppEvent::FocusLost,
            _ => continue,
        };
        if input_tx.send(app_event).is_err() {
            break;
        }
    });

    let mut out = BufWriter::with_capacity(128 * 1024, stdout());
    app.adjust_viewport();
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
                app.adjust_viewport();
                app.render(&mut out)?;
            }
            Ok(AppEvent::FocusLost) => {
                if app.editor.is_dirty() && app.editor.file_path().is_some() {
                    if let Err(e) = app.editor.save_async() {
                        app.set_status(format!("Save error: {}", e));
                    }
                    let _ = app.poll_async_saves();
                    app.render(&mut out)?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                let has_syntax_update = app.has_pending_syntax_response();
                let autosaved = app.editor.check_autosave();
                let has_save_event = app.poll_async_saves();
                if autosaved || has_syntax_update || has_save_event {
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
    use std::path::Path;

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
        assert!(filtered.iter().any(|c| c.id == "file.save"));

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
    fn test_app_file_explorer_toggle() {
        let mut app = App::new(None).unwrap();
        assert!(app.explorer.is_visible);

        // F2 toggles explorer
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
            .unwrap();
        assert!(!app.explorer.is_visible);

        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
            .unwrap();
        assert!(app.explorer.is_visible);
    }

    #[test]
    fn test_app_prompt_save_and_open() {
        let mut app = App::new(None).unwrap();
        app.editor.insert_str("test file content").unwrap();

        // Ctrl+S triggers Save As prompt on untitled buffer
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
            .unwrap();
        assert!(app.prompt.is_some());

        // Type target filename
        let target_path = "/tmp/lume_test_save.txt";
        for c in target_path.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        }

        // Enter submits save
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(app.prompt.is_none());
        assert_eq!(app.editor.file_path(), Some(Path::new(target_path)));
        assert!(!app.editor.is_dirty());

        // Open File prompt
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
            .unwrap();
        assert!(app.prompt.is_some());

        for c in target_path.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert!(app.prompt.is_none());

        let _ = std::fs::remove_file(target_path);
    }

    #[test]
    fn test_app_language_override() {
        let mut app = App::new(None).unwrap();
        assert_eq!(app.current_language(), None);

        // Set language mode via command
        app.execute_command("language.set_mode");
        assert!(app.prompt.is_some());

        for c in "Rust".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.current_language(), Some(Language::Rust));
    }

    #[test]
    fn test_app_mouse_tab_switching() {
        let mut app = App::new(None).unwrap();
        app.editor.insert_str("tab 1").unwrap();
        app.editor.new_tab();
        app.editor.insert_str("tab 2").unwrap();

        assert_eq!(app.editor.active_tab_idx(), 1);

        // Explorer is visible by default with width = app.text_area_left()
        let offset = app.text_area_left();

        // Click on Tab 1 title area in tab bar (row 0, col 5 inside tab bar)
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: (offset + 5) as u16,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();

        assert_eq!(app.editor.active_tab_idx(), 0);
    }

    #[test]
    fn test_app_mouse_tab_name_containing_x_does_not_close() {
        let mut app = App::new(None).unwrap();
        app.editor
            .set_file_path(Some(PathBuf::from("/src/explorer.rs")));
        app.editor.new_tab();
        app.editor
            .set_file_path(Some(PathBuf::from("/src/index.html")));

        assert_eq!(app.editor.active_tab_idx(), 1);
        let offset = app.text_area_left();

        // Click on the 'x' character in the file name 'explorer.rs' in Tab 1
        // Tab 1 text: "  1: explorer.rs x   "
        // 'x' in 'explorer' is at offset ~6 in the tab text
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: (offset + 6) as u16,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();

        // Tab was selected, not closed!
        assert_eq!(app.editor.tabs().len(), 2);
        assert_eq!(app.editor.active_tab_idx(), 0);
    }

    #[test]
    fn test_app_syntax_worker_async_highlighting() {
        let mut app = App::new(None).unwrap();
        app.editor.insert_str("fn main() { let x = 42; }").unwrap();
        app.editor.set_language_override(Some("Rust".to_string()));

        // First render triggers async syntax request
        let mut sink = std::io::sink();
        app.render(&mut sink).unwrap();

        // Wait for background worker to produce tokens (up to 500ms)
        for _ in 0..50 {
            if app.has_pending_syntax_response() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let tokens = app.update_syntax_highlights();
        assert!(
            !tokens.is_empty(),
            "SyntaxWorker should have generated tokens"
        );

        let has_keyword = tokens.iter().any(|t| t.token_type == TokenType::Keyword);
        let has_function = tokens.iter().any(|t| t.token_type == TokenType::Function);
        assert!(has_keyword, "Expected keyword token (e.g. fn / let)");
        assert!(has_function, "Expected function token (e.g. main)");
    }

    #[test]
    fn test_app_ignores_key_release_events() {
        let mut app = App::new(None).unwrap();
        let press =
            KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Press);
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );

        app.handle_key(press).unwrap();
        app.handle_key(release).unwrap();

        assert_eq!(
            app.editor
                .buffer()
                .slice_to_cow(0, app.editor.buffer().len_bytes()),
            "a"
        );
    }

    #[test]
    fn test_app_mouse_scroll_preserves_viewport_across_renders() {
        let mut app = App::new(None).unwrap();
        let content = (1..=100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        app.editor.insert_str(&content).unwrap();

        // Move cursor to line 0
        app.editor.move_lines(-100, false);
        app.adjust_viewport();
        assert_eq!(app.editor.cursor_line_col().0, 0);
        assert_eq!(app.scroll_row, 0);

        // Scroll down via trackpad / mouse
        let text_left = app.text_area_left();
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: text_left as u16 + 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();

        assert_eq!(app.scroll_row, 3);

        // Rendering MUST NOT revert scroll_row back to cursor position (0)
        let mut sink = std::io::sink();
        app.render(&mut sink).unwrap();
        assert_eq!(
            app.scroll_row, 3,
            "render() must not snap scroll_row back to cursor_row"
        );

        // Scroll left/right
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollRight,
            column: text_left as u16 + 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
        assert_eq!(app.scroll_col, 3);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollLeft,
            column: text_left as u16 + 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
        assert_eq!(app.scroll_col, 0);

        // Typing a character brings the viewport back to the cursor
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(
            app.scroll_row, 0,
            "Typing must adjust viewport back to cursor"
        );
    }

    #[test]
    fn test_app_page_up_and_page_down() {
        let mut app = App::new(None).unwrap();
        let content = (1..=100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        app.editor.insert_str(&content).unwrap();
        app.editor.move_lines(-100, false);
        app.adjust_viewport();

        assert_eq!(app.editor.cursor_line_col().0, 0);

        // Page Down
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .unwrap();
        let h = app.text_area_height();
        assert_eq!(app.editor.cursor_line_col().0, h);

        // Page Up
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(app.editor.cursor_line_col().0, 0);
    }

    #[test]
    fn test_app_open_directory_workspace() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_ws_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let sub_file = tmp_dir.join("workspace_sample.txt");
        std::fs::write(&sub_file, "sample in ws").unwrap();

        let app = App::from_paths(&[&tmp_dir]).unwrap();
        assert_eq!(app.explorer.root, std::fs::canonicalize(&tmp_dir).unwrap());
        // Editor should have 1 untitled clean tab
        assert_eq!(app.editor.tabs().len(), 1);
        assert!(app.editor.file_path().is_none());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_app_open_single_existing_file() {
        let app = App::from_paths(&["Cargo.toml"]).unwrap();
        assert_eq!(app.editor.tabs().len(), 1);
        assert!(app.editor.file_path().is_some());
        assert!(app.editor.buffer().len_bytes() > 0);
    }

    #[test]
    fn test_app_open_single_new_file() {
        let new_file_name = format!("new_file_{}.txt", std::process::id());
        let app = App::from_paths(&[&new_file_name]).unwrap();
        assert_eq!(app.editor.tabs().len(), 1);
        assert!(app.editor.file_path().is_some());
        assert_eq!(app.editor.buffer().len_bytes(), 0);
    }

    #[test]
    fn test_app_open_multiple_files_tabs() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_multi_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let file1 = tmp_dir.join("file1.txt");
        let file2 = tmp_dir.join("file2.txt");
        std::fs::write(&file1, "file1 content").unwrap();
        std::fs::write(&file2, "file2 content").unwrap();

        let app = App::from_paths(&[&file1, &file2]).unwrap();
        assert_eq!(app.editor.tabs().len(), 2);
        assert_eq!(app.editor.active_tab_idx(), 0);

        // Tab 1
        assert_eq!(
            app.editor
                .buffer()
                .slice_to_cow(0, app.editor.buffer().len_bytes()),
            "file1 content"
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_app_open_directory_and_file() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_ws_file_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let file = tmp_dir.join("hello.txt");
        std::fs::write(&file, "hello world").unwrap();

        let app = App::from_paths(&[&tmp_dir, &file]).unwrap();
        assert_eq!(app.explorer.root, std::fs::canonicalize(&tmp_dir).unwrap());
        assert_eq!(app.editor.tabs().len(), 1);
        assert_eq!(
            app.editor
                .buffer()
                .slice_to_cow(0, app.editor.buffer().len_bytes()),
            "hello world"
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_app_open_wildcard_glob() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_glob_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let f1 = tmp_dir.join("test_a.txt");
        let f2 = tmp_dir.join("test_b.txt");
        std::fs::write(&f1, "content a").unwrap();
        std::fs::write(&f2, "content b").unwrap();

        let pattern = format!("{}/*.txt", tmp_dir.display());
        let app = App::from_paths(&[&pattern]).unwrap();
        assert_eq!(app.editor.tabs().len(), 2);
        assert_eq!(app.editor.active_tab_idx(), 0);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
