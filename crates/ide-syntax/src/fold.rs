//! Indentation and brace-based fold ranges. Independent of a live LSP server.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
}

/// Compute foldable ranges from source text.
/// Prefers brace pairs when braces are balanced; otherwise folds consecutive
/// indented blocks (Python/Nix-friendly).
pub fn fold_ranges(text: &str) -> Vec<FoldRange> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let brace = brace_folds(&lines);
    if !brace.is_empty() {
        return brace;
    }
    indent_folds(&lines)
}

fn brace_folds(lines: &[&str]) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    let mut stack: Vec<(usize, i32)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let mut depth_delta = 0_i32;
        let mut in_string = None;
        let mut escaped = false;
        for ch in line.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && in_string.is_some() {
                escaped = true;
                continue;
            }
            if let Some(quote) = in_string {
                if ch == quote {
                    in_string = None;
                }
                continue;
            }
            match ch {
                '"' | '\'' => in_string = Some(ch),
                '{' | '(' | '[' => depth_delta += 1,
                '}' | ')' | ']' => depth_delta -= 1,
                _ => {}
            }
        }
        if depth_delta > 0 {
            for _ in 0..depth_delta {
                stack.push((index, 1));
            }
        } else if depth_delta < 0 {
            let mut need = -depth_delta;
            while need > 0 {
                if let Some((start, _)) = stack.pop() {
                    if index > start {
                        ranges.push(FoldRange {
                            start_line: start,
                            end_line: index,
                        });
                    }
                }
                need -= 1;
            }
        }
    }
    ranges.sort_by_key(|range| (range.start_line, range.end_line));
    ranges.dedup();
    ranges
}

fn indent_folds(lines: &[&str]) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let indent = leading_indent(lines[index]);
        if lines[index].trim().is_empty() {
            index += 1;
            continue;
        }
        let mut end = index;
        let mut j = index + 1;
        while j < lines.len() {
            if lines[j].trim().is_empty() {
                j += 1;
                continue;
            }
            if leading_indent(lines[j]) > indent {
                end = j;
                j += 1;
            } else {
                break;
            }
        }
        if end > index {
            ranges.push(FoldRange {
                start_line: index,
                end_line: end,
            });
            index = end + 1;
        } else {
            index += 1;
        }
    }
    ranges
}

fn leading_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braces_fold_functions() {
        let text = "fn main() {\n    let x = 1;\n}\n";
        let ranges = fold_ranges(text);
        assert!(
            ranges.iter().any(|r| r.start_line == 0 && r.end_line == 2),
            "{ranges:?}"
        );
    }

    #[test]
    fn indent_folds_python_blocks() {
        let text = "def f():\n    a = 1\n    b = 2\nc = 3\n";
        let ranges = fold_ranges(text);
        assert!(
            ranges.iter().any(|r| r.start_line == 0 && r.end_line >= 2),
            "{ranges:?}"
        );
    }
}
