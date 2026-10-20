use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ExplorerItem {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_expanded: bool,
}

pub struct FileExplorer {
    pub root: PathBuf,
    pub expanded_dirs: HashSet<PathBuf>,
    pub items: Vec<ExplorerItem>,
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub is_visible: bool,
    pub width: usize,
}

impl FileExplorer {
    pub fn new(root: PathBuf) -> Self {
        let mut expanded_dirs = HashSet::new();
        expanded_dirs.insert(root.clone());

        let mut explorer = Self {
            root,
            expanded_dirs,
            items: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            is_visible: true,
            width: 26,
        };
        explorer.refresh();
        explorer
    }

    pub fn refresh(&mut self) {
        let mut items = Vec::new();
        Self::populate_tree(&self.root, 0, &self.expanded_dirs, &mut items);
        self.items = items;
        if self.items.is_empty() {
            self.selected_idx = 0;
            self.scroll_offset = 0;
        } else if self.selected_idx >= self.items.len() {
            self.selected_idx = self.items.len() - 1;
        }
    }

    fn populate_tree(
        dir: &Path,
        depth: usize,
        expanded_dirs: &HashSet<PathBuf>,
        out: &mut Vec<ExplorerItem>,
    ) {
        let read_dir = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };

        let mut entries: Vec<fs::DirEntry> = read_dir.filter_map(|e| e.ok()).collect();

        // Sort: directories first, then alphabetically
        entries.sort_by(|a, b| {
            let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        for entry in entries {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            // Ignore hidden files and build/repo artifacts
            if file_name.starts_with('.') && file_name != ".gitignore" {
                continue;
            }
            if file_name == "target"
                || file_name == "node_modules"
                || file_name.ends_with(".lume_tmp")
            {
                continue;
            }

            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let is_expanded = is_dir && expanded_dirs.contains(&path);

            out.push(ExplorerItem {
                path: path.clone(),
                name: file_name,
                is_dir,
                depth,
                is_expanded,
            });

            if is_dir && is_expanded {
                Self::populate_tree(&path, depth + 1, expanded_dirs, out);
            }
        }
    }

    pub fn toggle_visible(&mut self) {
        self.is_visible = !self.is_visible;
        if self.is_visible {
            self.refresh();
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
            if self.selected_idx < self.scroll_offset {
                self.scroll_offset = self.selected_idx;
            }
        }
    }

    pub fn move_down(&mut self, max_rows: usize) {
        if !self.items.is_empty() && self.selected_idx + 1 < self.items.len() {
            self.selected_idx += 1;
            if self.selected_idx >= self.scroll_offset + max_rows {
                self.scroll_offset = self.selected_idx.saturating_sub(max_rows - 1);
            }
        }
    }

    pub fn activate_selected(&mut self) -> Option<PathBuf> {
        if self.items.is_empty() || self.selected_idx >= self.items.len() {
            return None;
        }

        let item = &self.items[self.selected_idx];
        if item.is_dir {
            let path = item.path.clone();
            if self.expanded_dirs.contains(&path) {
                self.expanded_dirs.remove(&path);
            } else {
                self.expanded_dirs.insert(path);
            }
            self.refresh();
            None
        } else {
            Some(item.path.clone())
        }
    }

    pub fn collapse_selected(&mut self) {
        if self.items.is_empty() || self.selected_idx >= self.items.len() {
            return;
        }
        let item = &self.items[self.selected_idx];
        if item.is_dir && item.is_expanded {
            let path = item.path.clone();
            self.expanded_dirs.remove(&path);
            self.refresh();
        } else if item.depth > 0 {
            // Find parent directory
            let target_depth = item.depth - 1;
            for i in (0..self.selected_idx).rev() {
                if self.items[i].is_dir && self.items[i].depth == target_depth {
                    self.selected_idx = i;
                    if self.selected_idx < self.scroll_offset {
                        self.scroll_offset = self.selected_idx;
                    }
                    break;
                }
            }
        }
    }

    pub fn expand_selected(&mut self) {
        if self.items.is_empty() || self.selected_idx >= self.items.len() {
            return;
        }
        let item = &self.items[self.selected_idx];
        if item.is_dir && !item.is_expanded {
            let path = item.path.clone();
            self.expanded_dirs.insert(path);
            self.refresh();
        }
    }

    pub fn click_at(&mut self, row_relative: usize) -> Option<PathBuf> {
        let target_idx = self.scroll_offset + row_relative;
        if target_idx < self.items.len() {
            self.selected_idx = target_idx;
            self.activate_selected()
        } else {
            None
        }
    }

    pub fn selected_item(&self) -> Option<&ExplorerItem> {
        self.items.get(self.selected_idx)
    }

    pub fn target_folder_for_creation(&self) -> PathBuf {
        if let Some(item) = self.selected_item() {
            if item.is_dir {
                item.path.clone()
            } else {
                item.path.parent().unwrap_or(&self.root).to_path_buf()
            }
        } else {
            self.root.clone()
        }
    }

    pub fn create_folder<P: AsRef<Path>>(
        &mut self,
        base_dir: &Path,
        rel_path: P,
    ) -> std::io::Result<PathBuf> {
        let rel = rel_path.as_ref();
        let rel_str = rel.to_string_lossy();
        let trimmed = rel_str.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Folder name cannot be empty",
            ));
        }

        let target = base_dir.join(trimmed);
        let normalized = if target.is_absolute() {
            target
        } else {
            self.root.join(target)
        };

        // Prevent escaping root workspace
        let root_canon = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        if let Ok(canon_base) = base_dir.canonicalize() {
            if !canon_base.starts_with(&root_canon) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Base directory is outside workspace root",
                ));
            }
        }

        let mut depth_check = base_dir.to_path_buf();
        for comp in Path::new(trimmed).components() {
            match comp {
                std::path::Component::ParentDir => {
                    if !depth_check.pop() || !depth_check.starts_with(&root_canon) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Path traversal escaping workspace root is forbidden",
                        ));
                    }
                }
                std::path::Component::Normal(c) => depth_check.push(c),
                _ => {}
            }
        }

        fs::create_dir_all(&normalized)?;
        let canon_target = normalized.canonicalize().unwrap_or(normalized);

        // Auto-expand all ancestor directories from root down to created folder
        let mut curr = canon_target.parent();
        while let Some(parent) = curr {
            self.expanded_dirs.insert(parent.to_path_buf());
            if parent == root_canon || !parent.starts_with(&root_canon) {
                break;
            }
            curr = parent.parent();
        }

        self.refresh();

        // Select the newly created folder in items
        if let Some(pos) = self.items.iter().position(|it| it.path == canon_target) {
            self.selected_idx = pos;
        }

        Ok(canon_target)
    }

    pub fn render<W: Write>(
        &self,
        writer: &mut W,
        top_row: u16,
        height: usize,
        is_focused: bool,
    ) -> std::io::Result<()> {
        if !self.is_visible || height == 0 {
            return Ok(());
        }

        let width = self.width;
        let content_w = width.saturating_sub(1); // last column is the vertical border

        // Header (Row top_row)
        queue!(
            writer,
            MoveTo(0, top_row),
            SetBackgroundColor(Color::Rgb {
                r: 25,
                g: 30,
                b: 40
            }),
            SetForegroundColor(if is_focused {
                Color::Yellow
            } else {
                Color::White
            })
        )?;

        let root_name = self
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_else(|| self.root.to_str().unwrap_or("WORKSPACE"));
        let header_raw = format!(" 📁 {} [F2]", root_name);
        let header_text = truncate_to_width(&header_raw, content_w);
        let fill = content_w.saturating_sub(display_width(&header_text));
        queue!(
            writer,
            Print(format!("{}{:fill$}", header_text, "", fill = fill)),
            SetForegroundColor(Color::DarkGrey),
            Print("│"),
            ResetColor
        )?;

        // Rows
        for i in 0..height.saturating_sub(1) {
            let item_idx = self.scroll_offset + i;
            let screen_row = top_row + 1 + i as u16;
            queue!(writer, MoveTo(0, screen_row))?;

            if item_idx < self.items.len() {
                let item = &self.items[item_idx];
                let is_sel = item_idx == self.selected_idx;

                if is_sel {
                    let bg = if is_focused {
                        Color::Rgb {
                            r: 35,
                            g: 55,
                            b: 90,
                        }
                    } else {
                        Color::Rgb {
                            r: 30,
                            g: 35,
                            b: 45,
                        }
                    };
                    queue!(writer, SetBackgroundColor(bg))?;
                } else {
                    queue!(
                        writer,
                        SetBackgroundColor(Color::Rgb {
                            r: 18,
                            g: 20,
                            b: 26
                        })
                    )?;
                }

                let indent = "  ".repeat(item.depth.min(6));
                let icon = if item.is_dir {
                    if item.is_expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    get_file_icon(&item.name)
                };

                let prefix = format!("{}{}", indent, icon);
                let avail_name = content_w.saturating_sub(display_width(&prefix));
                let name_truncated = truncate_to_width(&item.name, avail_name);
                let line_text = format!("{}{}", prefix, name_truncated);
                let line_fill = content_w.saturating_sub(display_width(&line_text));

                let fg = if is_sel {
                    if is_focused {
                        Color::Yellow
                    } else {
                        Color::White
                    }
                } else if item.is_dir {
                    Color::Cyan
                } else {
                    get_file_color(&item.name)
                };

                queue!(
                    writer,
                    SetForegroundColor(fg),
                    Print(format!("{}{:fill$}", line_text, "", fill = line_fill)),
                    SetBackgroundColor(Color::Reset),
                    SetForegroundColor(Color::DarkGrey),
                    Print("│"),
                    ResetColor
                )?;
            } else {
                queue!(
                    writer,
                    SetBackgroundColor(Color::Rgb {
                        r: 18,
                        g: 20,
                        b: 26
                    }),
                    Print(" ".repeat(content_w)),
                    SetBackgroundColor(Color::Reset),
                    SetForegroundColor(Color::DarkGrey),
                    Print("│"),
                    ResetColor
                )?;
            }
        }

        Ok(())
    }
}

fn get_file_icon(name: &str) -> &'static str {
    if let Some(ext) = Path::new(name).extension().and_then(|s| s.to_str()) {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => "🦀 ",
            "c" | "h" => "🅒 ",
            "cpp" | "hpp" | "cc" | "cu" => "➕ ",
            "py" => "🐍 ",
            "java" => "☕ ",
            "js" | "ts" | "jsx" | "tsx" => "🅙 ",
            "s" | "asm" => "⚙ ",
            "toml" | "json" | "yaml" | "yml" => "📋 ",
            "md" => "📝 ",
            _ => "  ",
        }
    } else {
        "  "
    }
}

fn get_file_color(name: &str) -> Color {
    if let Some(ext) = Path::new(name).extension().and_then(|s| s.to_str()) {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Color::Rgb {
                r: 230,
                g: 120,
                b: 60,
            },
            "c" | "h" | "cpp" | "hpp" | "cc" | "cu" => Color::Rgb {
                r: 90,
                g: 160,
                b: 240,
            },
            "py" => Color::Rgb {
                r: 240,
                g: 210,
                b: 80,
            },
            "java" => Color::Rgb {
                r: 230,
                g: 90,
                b: 80,
            },
            "js" | "ts" => Color::Rgb {
                r: 240,
                g: 220,
                b: 70,
            },
            "s" | "asm" => Color::Rgb {
                r: 180,
                g: 140,
                b: 220,
            },
            "toml" | "json" | "yaml" | "yml" => Color::Rgb {
                r: 100,
                g: 210,
                b: 180,
            },
            "md" => Color::Rgb {
                r: 140,
                g: 200,
                b: 240,
            },
            _ => Color::White,
        }
    } else {
        Color::White
    }
}

fn char_display_width(c: char) -> usize {
    if c < ' '
        || ('\u{007f}'..='\u{009f}').contains(&c)
        || ('\u{0300}'..='\u{036f}').contains(&c)
        || ('\u{fe00}'..='\u{fe0f}').contains(&c)
    {
        0
    } else if ('\u{1100}'..='\u{115f}').contains(&c)
        || ('\u{2e80}'..='\u{303e}').contains(&c)
        || ('\u{3040}'..='\u{a4cf}').contains(&c)
        || ('\u{ac00}'..='\u{d7a3}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
        || ('\u{fe10}'..='\u{fe6f}').contains(&c)
        || ('\u{ff00}'..='\u{ff60}').contains(&c)
        || ('\u{ffe0}'..='\u{ffe6}').contains(&c)
        || c == '\u{2615}'
        || c == '\u{2795}'
        || ('\u{1f300}'..='\u{1f9ff}').contains(&c)
        || ('\u{1fa00}'..='\u{1faff}').contains(&c)
        || ('\u{20000}'..='\u{2fffd}').contains(&c)
        || ('\u{30000}'..='\u{3fffd}').contains(&c)
    {
        2
    } else {
        1
    }
}

fn display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_display_width(c);
        if used + w > max_width {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explorer_creation_and_refresh() {
        let current = std::env::current_dir().unwrap();
        let explorer = FileExplorer::new(current);
        assert!(!explorer.items.is_empty());
        assert!(explorer.is_visible);
    }

    #[test]
    fn test_explorer_unicode_icon_border_alignment() {
        let content_w: usize = 25;
        let test_names = [
            ("src", true, true),
            ("crates", true, false),
            ("main.rs", false, false),
            ("Cargo.toml", false, false),
            ("README.md", false, false),
            ("boot.s", false, false),
            ("header.h", false, false),
        ];

        for (name, is_dir, is_expanded) in test_names {
            let icon = if is_dir {
                if is_expanded {
                    "▼ "
                } else {
                    "▶ "
                }
            } else {
                get_file_icon(name)
            };
            let prefix = format!("  {}", icon);
            let avail_name = content_w.saturating_sub(display_width(&prefix));
            let name_truncated = truncate_to_width(name, avail_name);
            let line_text = format!("{}{}", prefix, name_truncated);
            let line_fill = content_w.saturating_sub(display_width(&line_text));
            let formatted = format!("{}{:fill$}│", line_text, "", fill = line_fill);
            assert_eq!(
                display_width(&formatted),
                content_w + 1,
                "Row for '{}' had misaligned border",
                name
            );
        }
    }

    #[test]
    fn test_create_single_and_nested_folder() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_mkdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut explorer = FileExplorer::new(tmp_dir.clone());
        let root_canon = std::fs::canonicalize(&tmp_dir).unwrap();

        // 1. Create a single folder in root
        let created_single = explorer.create_folder(&root_canon, "alpha").unwrap();
        assert!(created_single.exists());
        assert!(created_single.is_dir());
        assert!(explorer.items.iter().any(|item| item.name == "alpha" && item.is_dir));

        // 2. Create nested folders inside alpha: alpha/beta/gamma
        let created_nested = explorer.create_folder(&created_single, "beta/gamma").unwrap();
        assert!(created_nested.exists());
        assert!(created_nested.is_dir());

        // Both beta and gamma's parents must be expanded
        assert!(explorer.expanded_dirs.contains(&created_single));
        let beta_path = created_single.join("beta");
        assert!(explorer.expanded_dirs.contains(&beta_path));

        // Selection should point to the new directory
        let sel_item = explorer.selected_item().expect("Must have selected item");
        assert_eq!(sel_item.name, "gamma");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_create_folder_prevents_escaping_root() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_escape_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut explorer = FileExplorer::new(tmp_dir.clone());
        let root_canon = std::fs::canonicalize(&tmp_dir).unwrap();

        let res = explorer.create_folder(&root_canon, "../outside_secret");
        assert!(res.is_err(), "Must reject path traversal escaping root");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
