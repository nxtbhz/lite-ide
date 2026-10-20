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
        let root = root.canonicalize().unwrap_or(root);
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
        } else {
            if self.selected_idx >= self.items.len() {
                self.selected_idx = self.items.len() - 1;
            }
            if self.scroll_offset >= self.items.len() {
                self.scroll_offset = self.selected_idx;
            }
            if self.selected_idx < self.scroll_offset {
                self.scroll_offset = self.selected_idx;
            }
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
        let trimmed_input = rel_str.trim();
        if trimmed_input.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Folder name cannot be empty",
            ));
        }

        // Check if raw input is absolute or contains drive prefixes
        if rel.is_absolute()
            || trimmed_input.starts_with('/')
            || trimmed_input.starts_with('\\')
            || Path::new(trimmed_input).components().any(|c| {
                matches!(
                    c,
                    std::path::Component::Prefix(_) | std::path::Component::RootDir
                )
            })
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Absolute paths or drive prefixes are forbidden",
            ));
        }

        let trimmed = trimmed_input.trim_end_matches(['/', '\\']);
        if trimmed.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Folder name cannot be empty",
            ));
        }

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
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "Absolute paths or drive prefixes are forbidden",
                    ));
                }
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

        let normalized = if base_dir.is_absolute() || base_dir.starts_with(&self.root) {
            base_dir.join(trimmed)
        } else {
            self.root.join(base_dir).join(trimmed)
        };

        fs::create_dir_all(&normalized)?;
        let canon_target = normalized.canonicalize().unwrap_or(normalized.clone());

        if !canon_target.starts_with(&root_canon) {
            let _ = fs::remove_dir_all(&canon_target);
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Target directory resolved outside workspace root",
            ));
        }

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
        if let Some(pos) = self
            .items
            .iter()
            .position(|it| it.path == canon_target || it.path == normalized)
        {
            self.selected_idx = pos;
            if pos < self.scroll_offset {
                self.scroll_offset = pos;
            }
        }

        Ok(canon_target)
    }

    pub fn delete_folder<P: AsRef<Path>>(&mut self, path: P) -> std::io::Result<PathBuf> {
        let target = path.as_ref();
        let root_canon = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let target_canon = target.canonicalize()?;

        if target_canon == root_canon {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Cannot delete the workspace root directory",
            ));
        }

        if !target_canon.starts_with(&root_canon) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Cannot delete path outside workspace root",
            ));
        }

        if !target_canon.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Target is not a directory",
            ));
        }

        fs::remove_dir_all(&target_canon)?;

        // Prune deleted directory and all descendants from expanded_dirs
        self.expanded_dirs
            .retain(|p| !p.starts_with(&target_canon) && !p.starts_with(target));

        self.refresh();

        Ok(target_canon)
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

    #[test]
    fn test_delete_folder_recursive_and_prunes_expanded() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_rmdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut explorer = FileExplorer::new(tmp_dir.clone());
        let root_canon = std::fs::canonicalize(&tmp_dir).unwrap();

        // Create folder with contents: parent/child/grandchild
        let grandchild = explorer
            .create_folder(&root_canon, "parent/child/grandchild")
            .unwrap();
        let file_path = grandchild.join("nested.txt");
        std::fs::write(&file_path, "hello nested").unwrap();
        explorer.refresh();

        let parent_path = root_canon.join("parent");
        let child_path = parent_path.join("child");
        assert!(explorer.expanded_dirs.contains(&parent_path));
        assert!(explorer.expanded_dirs.contains(&child_path));

        // Delete parent folder
        let deleted = explorer.delete_folder(&parent_path).unwrap();
        assert_eq!(deleted, parent_path);
        assert!(!parent_path.exists());

        // Expanded dirs must have pruned parent, child, grandchild
        assert!(!explorer.expanded_dirs.contains(&parent_path));
        assert!(!explorer.expanded_dirs.contains(&child_path));
        assert!(!explorer.expanded_dirs.contains(&grandchild));

        // Items should be empty or clamped
        assert!(!explorer.items.iter().any(|it| it.path.starts_with(&parent_path)));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_delete_root_folder_prevented() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_root_del_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut explorer = FileExplorer::new(tmp_dir.clone());
        let root_canon = std::fs::canonicalize(&tmp_dir).unwrap();

        let res = explorer.delete_folder(&root_canon);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        assert!(tmp_dir.exists());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_create_folder_prevents_absolute_and_prefix_paths() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_abs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut explorer = FileExplorer::new(tmp_dir.clone());
        let root_canon = std::fs::canonicalize(&tmp_dir).unwrap();

        let res = explorer.create_folder(&root_canon, "/absolute_outside");
        assert!(res.is_err(), "Must reject absolute paths");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_explorer_delete_preserves_scroll_offset_no_blank_sidebar() {
        let tmp_dir = std::env::temp_dir().join(format!("lume_test_scroll_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut explorer = FileExplorer::new(tmp_dir.clone());
        let root_canon = std::fs::canonicalize(&tmp_dir).unwrap();

        // Create 10 directories
        for i in 0..10 {
            explorer.create_folder(&root_canon, format!("dir_{:02}", i)).unwrap();
        }
        assert_eq!(explorer.items.len(), 10);

        // Simulate scrolled viewport
        explorer.scroll_offset = 8;
        explorer.selected_idx = 9;

        // Delete multiple directories to shrink items to 3
        for i in 3..10 {
            let p = root_canon.join(format!("dir_{:02}", i));
            explorer.delete_folder(&p).unwrap();
        }
        assert_eq!(explorer.items.len(), 3);

        // scroll_offset must be clamped so it does not render a blank sidebar
        assert!(explorer.scroll_offset < explorer.items.len(), "scroll_offset was not clamped!");
        assert!(explorer.scroll_offset <= explorer.selected_idx, "scroll_offset must not exceed selected_idx");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
