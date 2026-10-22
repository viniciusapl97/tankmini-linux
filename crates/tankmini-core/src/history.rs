//! Undo/redo for editor states.
//!
//! The editor keeps whole snapshots (a preset table is 1942 bytes, so a few
//! hundred snapshots cost well under a megabyte). Callers push the state
//! *before* a change; consecutive pushes with the same `group` key (e.g. one
//! knob drag) collapse into one undo step.

#[derive(Debug, Clone)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    group: Option<String>,
    limit: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            group: None,
            limit: limit.max(1),
        }
    }

    /// Record `before` as the state to return to. A `group` equal to the
    /// previous one merges this change into the previous step.
    pub fn record(&mut self, before: &T, group: Option<&str>) {
        let same_group = group.is_some() && group.map(str::to_string) == self.group;
        self.group = group.map(str::to_string);
        self.redo.clear();
        if same_group {
            return;
        }
        if self.undo.last() == Some(before) {
            return;
        }
        self.undo.push(before.clone());
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Close the current group so the next change starts a new step.
    pub fn seal(&mut self) {
        self.group = None;
    }

    /// Step back: returns the state to show and remembers `current` for redo.
    pub fn undo(&mut self, current: &T) -> Option<T> {
        let prev = self.undo.pop()?;
        self.redo.push(current.clone());
        self.group = None;
        Some(prev)
    }

    pub fn redo(&mut self, current: &T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        self.group = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.group = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_walks_both_ways() {
        let mut h = History::new(10);
        let mut v = 1;
        h.record(&v, None);
        v = 2;
        h.record(&v, None);
        v = 3;
        assert_eq!(h.undo(&v), Some(2));
        v = 2;
        assert_eq!(h.undo(&v), Some(1));
        v = 1;
        assert_eq!(h.undo(&v), None);
        assert_eq!(h.redo(&v), Some(2));
        v = 2;
        assert_eq!(h.redo(&v), Some(3));
        assert!(!h.can_redo());
    }

    #[test]
    fn a_drag_is_one_step() {
        let mut h = History::new(10);
        for before in [10, 11, 12, 13] {
            h.record(&before, Some("knob 1/0"));
        }
        assert_eq!(h.undo(&14), Some(10));
        assert!(!h.can_undo());
    }

    #[test]
    fn new_edit_clears_redo_and_limit_holds() {
        let mut h = History::new(3);
        for i in 0..6 {
            h.record(&i, None);
        }
        assert_eq!(h.undo(&6), Some(5));
        h.record(&5, None);
        assert!(!h.can_redo());
        let mut steps = 0;
        let mut cur = 99;
        while let Some(prev) = h.undo(&cur) {
            cur = prev;
            steps += 1;
        }
        assert_eq!(steps, 3);
    }
}
