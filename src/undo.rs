pub enum Action {
    Insert { idx: usize, content: String },
    Delete { idx: usize, content: String },
}

impl Action {
    pub fn execute(&self, rope: &mut ropey::Rope) -> usize {
        match self {
            Action::Insert { idx, content } => {
                rope.insert(*idx, &content);
                return content.len();
            }
            Action::Delete { idx, content } => {
                let end_idx = idx + content.len();
                rope.remove(idx..&end_idx);
                return content.len();
            }
        }
    }

    pub fn undo(&self, rope: &mut ropey::Rope) -> usize {
        match self {
            Action::Insert { idx, content } => {
                let end_idx = idx + content.len();
                rope.remove(idx..&end_idx);
                return content.len().saturating_sub(1);
            }
            Action::Delete { idx, content } => {
                rope.insert(*idx, &content);
                return content.len();
            }
        }
    }
}
