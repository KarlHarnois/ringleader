use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use super::process_figures::ProcessFigures;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSample {
    taken_at: Duration,
    processes: BTreeMap<u32, ProcessFigures>,
}

impl ProcessSample {
    pub fn new(taken_at: Duration, processes: Vec<ProcessFigures>) -> Self {
        Self {
            taken_at,
            processes: processes
                .into_iter()
                .map(|process| (process.pid(), process))
                .collect(),
        }
    }

    pub fn figures_of(&self, pid: u32) -> Option<&ProcessFigures> {
        self.processes.get(&pid)
    }

    pub fn time_since(&self, earlier: &ProcessSample) -> Duration {
        self.taken_at.saturating_sub(earlier.taken_at)
    }

    pub fn trees_from(&self, roots: &[u32]) -> Vec<u32> {
        let children = self.children_by_parent();
        let mut tree = Vec::new();
        let mut reached = HashSet::new();
        let mut waiting: Vec<u32> = roots
            .iter()
            .copied()
            .filter(|root| self.processes.contains_key(root))
            .collect();
        while let Some(pid) = waiting.pop() {
            if reached.insert(pid) {
                tree.push(pid);
                waiting.extend(children.get(&pid).into_iter().flatten());
            }
        }
        tree
    }

    fn children_by_parent(&self) -> HashMap<u32, Vec<u32>> {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for process in self.processes.values() {
            children
                .entry(process.parent())
                .or_default()
                .push(process.pid());
        }
        children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SampleFixture;

    impl SampleFixture {
        const PANE: u32 = 100;
        const AGENT: u32 = 101;
        const TOOL: u32 = 102;
        const STRANGER: u32 = 200;
        const STRANGER_CHILD: u32 = 201;

        fn process(pid: u32, parent: u32) -> ProcessFigures {
            ProcessFigures::new(pid, parent, "example-program")
        }

        fn pane_with_grandchild_beside_a_stranger() -> ProcessSample {
            ProcessSample::new(
                Duration::ZERO,
                vec![
                    Self::process(Self::PANE, 1),
                    Self::process(Self::AGENT, Self::PANE),
                    Self::process(Self::TOOL, Self::AGENT),
                    Self::process(Self::STRANGER, 1),
                    Self::process(Self::STRANGER_CHILD, Self::STRANGER),
                ],
            )
        }

        fn sorted(mut pids: Vec<u32>) -> Vec<u32> {
            pids.sort_unstable();
            pids
        }
    }

    #[test]
    fn a_tree_holds_the_pane_process_and_every_descendant_of_it() {
        let sample = SampleFixture::pane_with_grandchild_beside_a_stranger();

        let tree = sample.trees_from(&[SampleFixture::PANE]);

        assert_eq!(
            SampleFixture::sorted(tree),
            [
                SampleFixture::PANE,
                SampleFixture::AGENT,
                SampleFixture::TOOL
            ]
        );
    }

    #[test]
    fn a_tree_holds_no_process_outside_it() {
        let sample = SampleFixture::pane_with_grandchild_beside_a_stranger();

        let tree = sample.trees_from(&[SampleFixture::PANE]);

        assert!(!tree.contains(&SampleFixture::STRANGER));
        assert!(!tree.contains(&SampleFixture::STRANGER_CHILD));
    }

    #[test]
    fn trees_of_several_panes_hold_each_process_once() {
        let sample = SampleFixture::pane_with_grandchild_beside_a_stranger();

        let tree = sample.trees_from(&[SampleFixture::PANE, SampleFixture::AGENT]);

        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn a_pane_process_the_sample_lacks_roots_no_tree() {
        let sample = SampleFixture::pane_with_grandchild_beside_a_stranger();

        assert!(sample.trees_from(&[999]).is_empty());
    }

    #[test]
    fn the_time_since_an_earlier_sample_is_the_time_between_them() {
        let earlier = ProcessSample::new(Duration::from_millis(500), Vec::new());
        let later = ProcessSample::new(Duration::from_millis(1700), Vec::new());

        assert_eq!(later.time_since(&earlier), Duration::from_millis(1200));
    }
}
