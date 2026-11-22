//! F07 ephemeral lenses. Modal host overlays with a bounded stack.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensKind {
    CallGraph,
    Dependencies,
    Symbols,
    ProcessTree,
    Syscall,
    HexDiff,
}

impl LensKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::CallGraph => "call graph",
            Self::Dependencies => "dependencies",
            Self::Symbols => "symbols",
            Self::ProcessTree => "process tree",
            Self::Syscall => "syscall monitor",
            Self::HexDiff => "hex diff",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LensSession {
    pub kind: LensKind,
    pub pane_id: usize,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct LensStack {
    stack: Vec<LensSession>,
}

const MAX_NEST: usize = 3;

impl LensStack {
    pub fn push(&mut self, kind: LensKind, pane_id: usize) -> LensSession {
        while self.stack.len() >= MAX_NEST {
            self.stack.remove(0);
        }
        let session = LensSession {
            kind,
            pane_id,
            lines: vec![
                format!("lens: {}", kind.title()),
                "Esc closes and restores focus".into(),
            ],
        };
        self.stack.push(session.clone());
        session
    }

    pub fn pop(&mut self) -> Option<LensSession> {
        self.stack.pop()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn current(&self) -> Option<&LensSession> {
        self.stack.last()
    }
}

/// Pure lens transition for Bend differential tests.
pub fn next_nest(depth: usize) -> usize {
    if depth >= MAX_NEST {
        MAX_NEST
    } else {
        depth + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nest_is_bounded() {
        assert_eq!(next_nest(0), 1);
        assert_eq!(next_nest(3), 3);
        let mut stack = LensStack::default();
        for _ in 0..5 {
            stack.push(LensKind::Symbols, 0);
        }
        assert_eq!(stack.len(), 3);
    }
}
