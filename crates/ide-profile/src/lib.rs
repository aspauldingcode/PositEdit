//! Profiler adapters. A missing sampler is reported; folded stacks from a real capture can be read.
use std::collections::BTreeMap;

pub const COMPONENT: &str = "ide-profile";
pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileNode {
    pub name: String,
    pub samples: u64,
}

pub fn sampler_status(tool: &str) -> Availability {
    match tool {
        "perf" | "sample" | "xctrace" | "samply" if tool_on_path(tool) => Availability::Ready,
        "perf" | "sample" | "xctrace" | "samply" => Availability::NotInstalled,
        _ => Availability::Unsupported,
    }
}

pub fn parse_folded(text: &str) -> Vec<ProfileNode> {
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    for line in text.lines() {
        let Some((stack, count)) = line.rsplit_once(' ') else {
            continue;
        };
        let Ok(samples) = count.parse::<u64>() else {
            continue;
        };
        if let Some(frame) = stack.split(';').next_back() {
            *totals.entry(frame.to_string()).or_insert(0) += samples;
        }
    }
    let mut nodes: Vec<_> = totals
        .into_iter()
        .map(|(name, samples)| ProfileNode { name, samples })
        .collect();
    nodes.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| left.name.cmp(&right.name))
    });
    nodes
}

fn tool_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_stacks_sum_leaf_samples() {
        let nodes = parse_folded("main;draw 4\nmain;draw 1\nmain;parse 2\n");
        assert_eq!(
            nodes[0],
            ProfileNode {
                name: "draw".into(),
                samples: 5
            }
        );
        assert_eq!(nodes[1].name, "parse");
    }

    #[test]
    fn unknown_sampler_is_not_ready() {
        assert_eq!(sampler_status("not-a-profiler"), Availability::Unsupported);
    }
}
