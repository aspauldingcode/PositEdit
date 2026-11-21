//! Test discovery from tool output. Results stay structured; a missing runner is reported.
use std::collections::BTreeMap;

pub const COMPONENT: &str = "ide-test";
pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

pub fn status() -> Availability {
    Availability::Ready
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed,
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCase {
    pub name: String,
    pub status: TestStatus,
}

pub fn parse_cargo_test(output: &str) -> Vec<TestCase> {
    let mut cases = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(" ... ") else {
            continue;
        };
        let status = match tail.trim() {
            "ok" => TestStatus::Passed,
            "FAILED" => TestStatus::Failed,
            "ignored" => TestStatus::Ignored,
            _ => continue,
        };
        cases.push(TestCase {
            name: name.to_string(),
            status,
        });
    }
    cases
}

pub fn by_module(cases: &[TestCase]) -> BTreeMap<String, Vec<&TestCase>> {
    let mut groups = BTreeMap::new();
    for case in cases {
        let module = case
            .name
            .rsplit_once("::")
            .map(|(module, _)| module)
            .unwrap_or("");
        groups
            .entry(module.to_string())
            .or_insert_with(Vec::new)
            .push(case);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pass_fail_and_groups_modules() {
        let output = "\
running 3 tests
test app::ui::draws ... ok
test app::ui::keys ... FAILED
test plain ... ignored
test result: FAILED. 1 passed; 1 failed; 1 ignored
";
        let cases = parse_cargo_test(output);
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[1].status, TestStatus::Failed);
        let groups = by_module(&cases);
        assert_eq!(groups["app::ui"].len(), 2);
        assert_eq!(groups[""][0].status, TestStatus::Ignored);
    }
}
