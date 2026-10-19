#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CursorAffinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub anchor: usize,
    pub head: usize,
    pub affinity: CursorAffinity,
}

impl Cursor {
    pub fn new(head: usize) -> Self {
        Self {
            anchor: head,
            head,
            affinity: CursorAffinity::Downstream,
        }
    }

    pub fn with_anchor(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            affinity: CursorAffinity::Downstream,
        }
    }

    #[inline]
    pub fn is_selection(&self) -> bool {
        self.anchor != self.head
    }

    #[inline]
    pub fn min(&self) -> usize {
        self.anchor.min(self.head)
    }

    #[inline]
    pub fn max(&self) -> usize {
        self.anchor.max(self.head)
    }

    #[inline]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.min()..self.max()
    }

    pub fn move_to(&mut self, pos: usize) {
        self.anchor = pos;
        self.head = pos;
    }

    pub fn select_to(&mut self, pos: usize) {
        self.head = pos;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CursorSet {
    cursors: Vec<Cursor>,
    primary_idx: usize,
}

impl Default for CursorSet {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorSet {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::new(0)],
            primary_idx: 0,
        }
    }

    pub fn single(head: usize) -> Self {
        Self {
            cursors: vec![Cursor::new(head)],
            primary_idx: 0,
        }
    }

    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    pub fn cursors_mut(&mut self) -> &mut [Cursor] {
        &mut self.cursors
    }

    pub fn primary(&self) -> &Cursor {
        &self.cursors[self.primary_idx]
    }

    pub fn primary_mut(&mut self) -> &mut Cursor {
        &mut self.cursors[self.primary_idx]
    }

    pub fn set_single(&mut self, head: usize) {
        self.cursors.clear();
        self.cursors.push(Cursor::new(head));
        self.primary_idx = 0;
    }

    pub fn set_cursors(&mut self, cursors: Vec<Cursor>) {
        if cursors.is_empty() {
            self.set_single(0);
        } else {
            self.cursors = cursors;
            self.primary_idx = 0;
            self.normalize();
        }
    }

    pub fn add_cursor(&mut self, cursor: Cursor) {
        self.cursors.push(cursor);
        self.normalize();
    }

    /// Sort cursors by position and merge overlapping selections
    pub fn normalize(&mut self) {
        if self.cursors.len() <= 1 {
            return;
        }

        // Sort by min position
        self.cursors.sort_by_key(|c| (c.min(), c.max()));

        let mut merged: Vec<Cursor> = Vec::with_capacity(self.cursors.len());
        for cursor in self.cursors.drain(..) {
            if let Some(last) = merged.last_mut() {
                // If touching or overlapping
                if cursor.min() <= last.max() {
                    let new_min = last.min().min(cursor.min());
                    let new_max = last.max().max(cursor.max());
                    // Preserve direction of the last active cursor
                    if last.head >= last.anchor {
                        last.anchor = new_min;
                        last.head = new_max;
                    } else {
                        last.anchor = new_max;
                        last.head = new_min;
                    }
                    continue;
                }
            }
            merged.push(cursor);
        }

        self.cursors = merged;
        if self.primary_idx >= self.cursors.len() {
            self.primary_idx = self.cursors.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_cursor() {
        let mut cursor = Cursor::new(10);
        assert_eq!(cursor.head, 10);
        assert_eq!(cursor.anchor, 10);
        assert!(!cursor.is_selection());

        cursor.select_to(20);
        assert!(cursor.is_selection());
        assert_eq!(cursor.min(), 10);
        assert_eq!(cursor.max(), 20);

        cursor.move_to(5);
        assert!(!cursor.is_selection());
        assert_eq!(cursor.head, 5);
    }

    #[test]
    fn test_cursor_set_normalize() {
        let mut set = CursorSet::new();
        // Add cursors that overlap: (0..5), (3..8), (12..15)
        set.add_cursor(Cursor::with_anchor(0, 5));
        set.add_cursor(Cursor::with_anchor(3, 8));
        set.add_cursor(Cursor::with_anchor(12, 15));

        // After normalization, (0..5) and (3..8) should merge into (0..8)
        let cursors = set.cursors();
        assert_eq!(cursors.len(), 2);
        assert_eq!(cursors[0].min(), 0);
        assert_eq!(cursors[0].max(), 8);
        assert_eq!(cursors[1].min(), 12);
        assert_eq!(cursors[1].max(), 15);
    }
}
