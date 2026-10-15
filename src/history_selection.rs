/// History is ordered newest first. Browsing never changes its order.
#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Older,
    Newer,
}

#[derive(Clone, Default, Debug)]
pub struct HistorySelection {
    selected: Option<u128>,
    newest: Option<u128>,
}

impl HistorySelection {
    pub fn reset(&mut self) {
        self.selected = None;
        self.newest = None;
    }

    pub fn step(&mut self, ids: &[u128], direction: Direction) -> Option<(u128, usize)> {
        if self.newest != ids.first().copied() {
            self.reset();
            self.newest = ids.first().copied();
        }
        if ids.is_empty() {
            return None;
        }
        let current = self
            .selected
            .and_then(|id| ids.iter().position(|entry| *entry == id))
            .unwrap_or(0);
        let next = match direction {
            Direction::Older => (current + 1).min(ids.len() - 1),
            Direction::Newer => current.saturating_sub(1),
        };
        self.selected = Some(ids[next]);
        Some((ids[next], next))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionOutcome {
    Waiting,
    Cancel,
    Paste,
}

pub struct PendingSelection {
    pub id: u128,
    pub target: usize,
    pub sequence: u32,
    pub modifiers: u8,
    pub previous: HistorySelection,
}

impl PendingSelection {
    pub fn outcome(&self, held: u8, sequence: u32, target: usize) -> SelectionOutcome {
        if sequence != self.sequence || target != self.target {
            SelectionOutcome::Cancel
        } else if held & self.modifiers == self.modifiers {
            SelectionOutcome::Waiting
        } else {
            SelectionOutcome::Paste
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_global_commands_walk_back_and_forward_without_wrapping() {
        let mut cursor = HistorySelection::default();
        let ids = [30, 20, 10];
        assert_eq!(cursor.step(&ids, Direction::Older), Some((20, 1)));
        assert_eq!(cursor.step(&ids, Direction::Older), Some((10, 2)));
        assert_eq!(cursor.step(&ids, Direction::Older), Some((10, 2)));
        assert_eq!(cursor.step(&ids, Direction::Newer), Some((20, 1)));
        assert_eq!(cursor.step(&ids, Direction::Newer), Some((30, 0)));
        assert_eq!(cursor.step(&ids, Direction::Newer), Some((30, 0)));
    }

    #[test]
    fn external_copy_resets_browsing_even_when_content_is_identical() {
        let mut cursor = HistorySelection::default();
        cursor.step(&[30, 20, 10], Direction::Older);
        cursor.step(&[30, 20, 10], Direction::Older);
        assert_eq!(
            cursor.step(&[40, 30, 20, 10], Direction::Older),
            Some((30, 1))
        );
    }

    #[test]
    fn empty_pruned_and_single_entry_histories_are_safe() {
        let mut cursor = HistorySelection::default();
        assert_eq!(cursor.step(&[], Direction::Older), None);
        assert_eq!(cursor.step(&[30], Direction::Older), Some((30, 0)));
        cursor.step(&[30, 20, 10], Direction::Older);
        cursor.step(&[30, 20, 10], Direction::Older);
        assert_eq!(cursor.step(&[30, 20], Direction::Newer), Some((30, 0)));
    }
    #[test]
    fn selection_waits_for_release_and_cancels_if_context_changed() {
        let pending = PendingSelection {
            id: 20,
            target: 42,
            sequence: 9,
            modifiers: 5,
            previous: HistorySelection::default(),
        };
        assert_eq!(pending.outcome(5, 9, 42), SelectionOutcome::Waiting);
        assert_eq!(pending.outcome(1, 9, 42), SelectionOutcome::Paste);
        assert_eq!(pending.outcome(0, 9, 42), SelectionOutcome::Paste);
        assert_eq!(pending.outcome(0, 10, 42), SelectionOutcome::Cancel);
        assert_eq!(pending.outcome(0, 9, 0), SelectionOutcome::Cancel);
    }
}
