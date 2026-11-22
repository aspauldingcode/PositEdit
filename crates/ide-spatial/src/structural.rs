//! F04 lossless structural edits via Tree-sitter byte ranges.

use ide_core::Edit;
use ide_syntax::NodeRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralOp {
    Select,
    FoldSignature,
    MoveUp,
    MoveDown,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuralEdit {
    pub op: StructuralOp,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuralPreview {
    pub op: StructuralOp,
    pub revision: u64,
    pub edits: Vec<Edit>,
    pub ambiguous: bool,
    pub summary: String,
}

pub fn preview(
    text: &str,
    revision: u64,
    node: Option<NodeRange>,
    op: StructuralOp,
) -> StructuralPreview {
    let Some(node) = node else {
        return StructuralPreview {
            op,
            revision,
            edits: Vec::new(),
            ambiguous: true,
            summary: "no node at cursor".into(),
        };
    };
    if node.end_byte > text.len() || node.start_byte > node.end_byte {
        return StructuralPreview {
            op,
            revision,
            edits: Vec::new(),
            ambiguous: true,
            summary: "node range outside document".into(),
        };
    }
    let slice = &text[node.start_byte..node.end_byte];
    if slice.is_empty() {
        return StructuralPreview {
            op,
            revision,
            edits: Vec::new(),
            ambiguous: true,
            summary: "empty node".into(),
        };
    }
    // Convert UTF-8 bytes to character offsets for Document::apply.
    let start_chars = text[..node.start_byte].chars().count();
    let end_chars = start_chars + slice.chars().count();
    let (edits, summary, ambiguous) = match op {
        StructuralOp::Select => (
            vec![Edit {
                start: start_chars,
                end: end_chars,
                replacement: slice.to_string(),
            }],
            format!("select {}..{}", node.start_byte, node.end_byte),
            false,
        ),
        StructuralOp::FoldSignature => {
            let first = slice.lines().next().unwrap_or(slice);
            (
                vec![Edit {
                    start: start_chars,
                    end: end_chars,
                    replacement: format!("{first} {{ /* folded */ }}"),
                }],
                "fold to signature".into(),
                false,
            )
        }
        StructuralOp::Duplicate => (
            vec![Edit {
                start: end_chars,
                end: end_chars,
                replacement: format!("\n{slice}"),
            }],
            "duplicate node".into(),
            false,
        ),
        StructuralOp::MoveUp | StructuralOp::MoveDown => {
            // Without a sibling range the move is ambiguous; refuse rather than guess.
            (
                Vec::new(),
                "move requires an unambiguous sibling".into(),
                true,
            )
        }
    };
    StructuralPreview {
        op,
        revision,
        edits,
        ambiguous,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_duplicate_is_lossless() {
        let text = "fn café() {\n  1\n}\n";
        let node = NodeRange {
            start_byte: 0,
            end_byte: text.len(),
            kind: "function_item".into(),
        };
        let preview = preview(text, 3, Some(node), StructuralOp::Duplicate);
        assert!(!preview.ambiguous);
        assert_eq!(preview.edits.len(), 1);
        assert!(preview.edits[0].replacement.contains("café"));
    }

    #[test]
    fn ambiguous_move_is_rejected() {
        let text = "a\nb\n";
        let node = NodeRange {
            start_byte: 0,
            end_byte: 2,
            kind: "expr".into(),
        };
        let preview = preview(text, 1, Some(node), StructuralOp::MoveUp);
        assert!(preview.ambiguous);
        assert!(preview.edits.is_empty());
    }
}
