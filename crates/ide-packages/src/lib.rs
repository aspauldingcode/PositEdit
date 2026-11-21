//! Package plans. Planning never installs into the host or a global profile.
use std::collections::BTreeSet;

pub const COMPONENT: &str = "ide-packages";
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagePlan {
    pub ecosystem: &'static str,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub executes: bool,
}

pub fn cargo_plan(before: &str, after: &str) -> PackagePlan {
    let previous = cargo_deps(before);
    let next = cargo_deps(after);
    PackagePlan {
        ecosystem: "cargo",
        add: next.difference(&previous).cloned().collect(),
        remove: previous.difference(&next).cloned().collect(),
        executes: false,
    }
}

pub fn nix_plan(before: &str, after: &str) -> PackagePlan {
    let previous = nix_packages(before);
    let next = nix_packages(after);
    PackagePlan {
        ecosystem: "nix",
        add: next.difference(&previous).cloned().collect(),
        remove: previous.difference(&next).cloned().collect(),
        executes: false,
    }
}

fn cargo_deps(manifest: &str) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = trimmed == "[dependencies]";
            continue;
        }
        if in_deps {
            if let Some((name, _)) = trimmed.split_once('=') {
                let name = name.trim();
                if !name.is_empty() {
                    deps.insert(name.to_string());
                }
            }
        }
    }
    deps
}

fn nix_packages(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut index = 0;
    while let Some(relative) = text[index..].find("pkgs.") {
        let start = index + relative + "pkgs.".len();
        let rest = &text[start..];
        let end = rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
            .unwrap_or(rest.len());
        if end > 0 {
            names.insert(rest[..end].to_string());
        }
        index = start + end;
        if index >= text.len() {
            break;
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_and_nix_plans_do_not_execute() {
        let cargo = cargo_plan(
            "[dependencies]\nserde = \"1\"\n",
            "[dependencies]\nserde = \"1\"\ntokio = \"1\"\n",
        );
        assert_eq!(cargo.add, vec!["tokio".to_string()]);
        assert!(!cargo.executes);
        let nix = nix_plan("pkgs.ripgrep", "pkgs.ripgrep pkgs.fd");
        assert_eq!(nix.add, vec!["fd".to_string()]);
        assert!(nix.remove.is_empty());
        assert!(!nix.executes);
    }
}
