//! Character, byte, UTF-16, and grapheme offsets for one document.
//! Offsets that fall inside a scalar or an extended grapheme cluster are errors.
use unicode_segmentation::UnicodeSegmentation;

use crate::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetUnit {
    /// Unicode scalar values. This is the coordinate system used by [`crate::Edit`].
    Char,
    /// UTF-8 bytes.
    Utf8,
    /// UTF-16 code units, the LSP default.
    Utf16,
    /// Extended grapheme clusters.
    Grapheme,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionError {
    OutOfRange,
    /// Byte or UTF-16 offset lands inside a Unicode scalar.
    MidScalar,
    /// Character offset lands inside an extended grapheme cluster.
    MidGrapheme,
}

impl Document {
    pub fn char_to_unit(&self, char_idx: usize, unit: OffsetUnit) -> Result<usize, PositionError> {
        if char_idx > self.len_chars() {
            return Err(PositionError::OutOfRange);
        }
        match unit {
            OffsetUnit::Char => Ok(char_idx),
            OffsetUnit::Utf8 => Ok(self.rope().char_to_byte(char_idx)),
            OffsetUnit::Utf16 => Ok(self.rope().char_to_utf16_cu(char_idx)),
            OffsetUnit::Grapheme => {
                let bounds = grapheme_bounds(&self.to_text());
                bounds
                    .iter()
                    .position(|start| *start == char_idx)
                    .ok_or(PositionError::MidGrapheme)
            }
        }
    }

    pub fn unit_to_char(&self, offset: usize, unit: OffsetUnit) -> Result<usize, PositionError> {
        match unit {
            OffsetUnit::Char => {
                if offset > self.len_chars() {
                    Err(PositionError::OutOfRange)
                } else {
                    Ok(offset)
                }
            }
            OffsetUnit::Utf8 => {
                let rope = self.rope();
                if offset > rope.len_bytes() {
                    return Err(PositionError::OutOfRange);
                }
                let char_idx = rope.byte_to_char(offset);
                if rope.char_to_byte(char_idx) != offset {
                    return Err(PositionError::MidScalar);
                }
                Ok(char_idx)
            }
            OffsetUnit::Utf16 => {
                let rope = self.rope();
                if offset > rope.len_utf16_cu() {
                    return Err(PositionError::OutOfRange);
                }
                let char_idx = rope.utf16_cu_to_char(offset);
                if rope.char_to_utf16_cu(char_idx) != offset {
                    return Err(PositionError::MidScalar);
                }
                Ok(char_idx)
            }
            OffsetUnit::Grapheme => {
                let bounds = grapheme_bounds(&self.to_text());
                bounds.get(offset).copied().ok_or(PositionError::OutOfRange)
            }
        }
    }
}

fn grapheme_bounds(text: &str) -> Vec<usize> {
    let mut bounds = Vec::with_capacity(text.chars().count() + 1);
    let mut chars = 0;
    bounds.push(0);
    for cluster in text.graphemes(true) {
        chars += cluster.chars().count();
        bounds.push(chars);
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    fn sample() -> Document {
        // a + combining acute, crab emoji, newline.
        Document::new("a\u{0301}🦀\n")
    }

    #[test]
    fn maps_scalar_byte_utf16_and_grapheme() {
        let doc = sample();
        assert_eq!(doc.len_chars(), 4);
        assert_eq!(doc.char_to_unit(0, OffsetUnit::Grapheme), Ok(0));
        assert_eq!(doc.char_to_unit(2, OffsetUnit::Grapheme), Ok(1));
        assert_eq!(doc.char_to_unit(3, OffsetUnit::Grapheme), Ok(2));
        assert_eq!(doc.char_to_unit(4, OffsetUnit::Grapheme), Ok(3));
        assert_eq!(doc.unit_to_char(1, OffsetUnit::Grapheme), Ok(2));
        assert_eq!(
            doc.char_to_unit(2, OffsetUnit::Utf8),
            Ok(1 + "\u{0301}".len())
        );
        assert_eq!(doc.char_to_unit(2, OffsetUnit::Utf16), Ok(2));
        assert_eq!(doc.char_to_unit(3, OffsetUnit::Utf16), Ok(4));
        let crab_at = doc.char_to_unit(2, OffsetUnit::Utf8).unwrap();
        assert_eq!(doc.unit_to_char(crab_at, OffsetUnit::Utf8), Ok(2));
        assert_eq!(doc.unit_to_char(4, OffsetUnit::Utf16), Ok(3));
    }

    #[test]
    fn rejects_mid_scalar_and_mid_grapheme() {
        let doc = sample();
        let crab_at = doc.char_to_unit(2, OffsetUnit::Utf8).unwrap();
        assert_eq!(
            doc.unit_to_char(crab_at + 1, OffsetUnit::Utf8),
            Err(PositionError::MidScalar)
        );
        assert_eq!(
            doc.unit_to_char(3, OffsetUnit::Utf16),
            Err(PositionError::MidScalar)
        );
        assert_eq!(
            doc.char_to_unit(1, OffsetUnit::Grapheme),
            Err(PositionError::MidGrapheme)
        );
        assert_eq!(
            doc.unit_to_char(9, OffsetUnit::Grapheme),
            Err(PositionError::OutOfRange)
        );
        assert_eq!(
            doc.unit_to_char(doc.rope().len_bytes() + 1, OffsetUnit::Utf8),
            Err(PositionError::OutOfRange)
        );
    }
}
