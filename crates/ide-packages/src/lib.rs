//! Package plans and project-config discovery.
//! Planning never installs into the host or a global profile.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSuggestion {
    pub system: &'static str,
    pub path: PathBuf,
    pub kind: &'static str,
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

pub fn gradle_plan(before: &str, after: &str) -> PackagePlan {
    PackagePlan {
        ecosystem: "gradle",
        add: dep_lines_added(before, after, "implementation"),
        remove: dep_lines_removed(before, after, "implementation"),
        executes: false,
    }
}

pub fn maven_plan(before: &str, after: &str) -> PackagePlan {
    PackagePlan {
        ecosystem: "maven",
        add: xml_artifacts_added(before, after),
        remove: xml_artifacts_removed(before, after),
        executes: false,
    }
}

/// Editable project files a user can open for build/debug suggestions.
pub fn project_suggestions(root: &Path) -> Vec<ProjectSuggestion> {
    let mut out = Vec::new();
    let candidates: &[(&str, &str, &str)] = &[
        ("cargo", "Cargo.toml", "manifest"),
        ("cargo", "Cargo.lock", "lockfile"),
        ("nix", "flake.nix", "flake"),
        ("nix", "shell.nix", "shell"),
        ("nix", "default.nix", "expression"),
        ("gradle", "build.gradle", "build"),
        ("gradle", "build.gradle.kts", "build"),
        ("gradle", "settings.gradle", "settings"),
        ("gradle", "settings.gradle.kts", "settings"),
        ("maven", "pom.xml", "pom"),
        ("cmake", "CMakeLists.txt", "cmake"),
        ("make", "Makefile", "makefile"),
        ("make", "GNUmakefile", "makefile"),
        ("make", "makefile", "makefile"),
        ("zig", "build.zig", "build"),
        ("swiftpm", "Package.swift", "package"),
        ("javascript", "package.json", "package"),
        ("meson", "meson.build", "meson"),
        ("bazel", "WORKSPACE", "workspace"),
        ("bazel", "MODULE.bazel", "module"),
        ("intellij", ".idea/misc.xml", "idea"),
        ("intellij", ".idea/modules.xml", "idea"),
        ("intellij", ".idea/workspace.xml", "idea"),
    ];
    for (system, rel, kind) in candidates {
        let path = root.join(rel);
        if path.is_file() {
            out.push(ProjectSuggestion {
                system,
                path,
                kind,
            });
        }
    }
    if let Ok(entries) = root.read_dir() {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".xcodeproj") && entry.path().is_dir() {
                let pbx = entry.path().join("project.pbxproj");
                if pbx.is_file() {
                    out.push(ProjectSuggestion {
                        system: "xcodebuild",
                        path: pbx,
                        kind: "xcodeproj",
                    });
                }
            }
            if name.ends_with(".xcworkspace") && entry.path().is_dir() {
                out.push(ProjectSuggestion {
                    system: "xcodebuild",
                    path: entry.path(),
                    kind: "xcworkspace",
                });
            }
        }
    }
    out
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

fn dep_lines_added(before: &str, after: &str, marker: &str) -> Vec<String> {
    let prev = marked_deps(before, marker);
    let next = marked_deps(after, marker);
    next.difference(&prev).cloned().collect()
}

fn dep_lines_removed(before: &str, after: &str, marker: &str) -> Vec<String> {
    let prev = marked_deps(before, marker);
    let next = marked_deps(after, marker);
    prev.difference(&next).cloned().collect()
}

fn marked_deps(text: &str, marker: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.contains(marker))
        .map(str::to_string)
        .collect()
}

fn xml_artifacts_added(before: &str, after: &str) -> Vec<String> {
    let prev = xml_artifacts(before);
    let next = xml_artifacts(after);
    next.difference(&prev).cloned().collect()
}

fn xml_artifacts_removed(before: &str, after: &str) -> Vec<String> {
    let prev = xml_artifacts(before);
    let next = xml_artifacts(after);
    prev.difference(&next).cloned().collect()
}

fn xml_artifacts(text: &str) -> BTreeSet<String> {
    let mut arts = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<artifactId>") {
            if let Some(name) = rest.strip_suffix("</artifactId>") {
                arts.insert(name.to_string());
            }
        }
    }
    arts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[test]
    fn gradle_and_maven_plans_track_deps() {
        let gradle = gradle_plan(
            "implementation 'com.a:a:1'\n",
            "implementation 'com.a:a:1'\nimplementation 'com.b:b:2'\n",
        );
        assert!(gradle.add.iter().any(|line| line.contains("com.b")));
        assert!(!gradle.executes);
        let maven = maven_plan(
            "<artifactId>a</artifactId>\n",
            "<artifactId>a</artifactId>\n<artifactId>b</artifactId>\n",
        );
        assert_eq!(maven.add, vec!["b".to_string()]);
    }

    #[test]
    fn project_suggestions_cover_major_markers() {
        let root = std::env::temp_dir().join(format!(
            "posit-pkg-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".idea")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(root.join("flake.nix"), "{}\n").unwrap();
        fs::write(root.join("pom.xml"), "<project/>\n").unwrap();
        fs::write(root.join("CMakeLists.txt"), "project(x)\n").unwrap();
        fs::write(root.join("Makefile"), "all:\n\t@echo hi\n").unwrap();
        fs::write(root.join("build.gradle"), "plugins{}\n").unwrap();
        fs::write(root.join(".idea/misc.xml"), "<project/>\n").unwrap();
        let xcode = root.join("App.xcodeproj");
        fs::create_dir_all(&xcode).unwrap();
        fs::write(xcode.join("project.pbxproj"), "// !$*UTF8*$\n").unwrap();

        let suggestions = project_suggestions(&root);
        let systems: BTreeSet<_> = suggestions.iter().map(|s| s.system).collect();
        for need in [
            "cargo",
            "nix",
            "maven",
            "cmake",
            "make",
            "gradle",
            "intellij",
            "xcodebuild",
        ] {
            assert!(systems.contains(need), "missing {need} in {systems:?}");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
