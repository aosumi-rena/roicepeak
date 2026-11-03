use crate::app::document::Project;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub(crate) struct BlockSelection {
    current: Option<u64>,
    members: BTreeSet<u64>,
}

impl BlockSelection {
    pub(crate) fn only(id: u64) -> Self {
        let mut selection = Self::default();
        selection.select_only(id);
        selection
    }

    pub(crate) fn current(&self) -> Option<u64> {
        self.current
    }

    pub(crate) fn contains(&self, id: u64) -> bool {
        self.members.contains(&id)
    }

    pub(crate) fn current_index(&self, project: &Project) -> Option<usize> {
        project
            .blocks
            .iter()
            .position(|block| Some(block.id) == self.current)
    }

    pub(crate) fn indices(&self, project: &Project) -> Vec<usize> {
        project
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| self.contains(block.id).then_some(index))
            .collect()
    }

    pub(crate) fn ids(&self, project: &Project) -> Vec<u64> {
        project
            .blocks
            .iter()
            .filter(|block| self.contains(block.id))
            .map(|block| block.id)
            .collect()
    }

    pub(crate) fn clear(&mut self) {
        self.current = None;
        self.members.clear();
    }

    pub(crate) fn select_only(&mut self, id: u64) {
        self.members.clear();
        self.set_current(id);
    }

    pub(crate) fn focus_current(&mut self, id: u64) {
        self.set_current(id);
    }

    fn set_current(&mut self, id: u64) {
        self.current = Some(id);
        self.members.insert(id);
    }

    pub(crate) fn prune(&mut self, project: &Project) {
        let existing: BTreeSet<_> = project.blocks.iter().map(|block| block.id).collect();
        self.members.retain(|id| existing.contains(id));
        self.current = self.current.filter(|id| existing.contains(id));
    }

    pub(crate) fn click(&mut self, project: &Project, index: usize, shift: bool, control: bool) {
        let Some(block) = project.blocks.get(index) else {
            return;
        };
        let id = block.id;
        if self.contains(id) {
            if control && self.members.len() >= 2 {
                let previous = self.current_index(project);
                self.members.remove(&id);
                self.repair_current(project, previous);
                return;
            }
            if !shift && !control {
                self.members.clear();
            }
        } else if shift {
            let (mut first, mut last) = (index, index);
            for selected in self.indices(project) {
                first = first.min(selected);
                last = last.max(selected);
            }
            self.members
                .extend(project.blocks[first..=last].iter().map(|block| block.id));
        } else if !control {
            self.members.clear();
        }
        self.set_current(id);
    }

    pub(crate) fn begin_rectangle(
        &mut self,
        project: &Project,
        nearest: Option<usize>,
        shift: bool,
        control: bool,
    ) {
        if !shift && !control {
            self.members.clear();
            if let Some(block) = nearest.and_then(|index| project.blocks.get(index)) {
                self.set_current(block.id);
            }
        }
    }

    pub(crate) fn finish_rectangle(
        &mut self,
        project: &Project,
        intersects: &BTreeSet<u64>,
        control: bool,
        nearest: Option<usize>,
    ) {
        if control {
            let previous = self.current_index(project);
            for id in intersects {
                if !self.members.remove(id) {
                    self.members.insert(*id);
                }
            }
            if self.members.is_empty() {
                if let Some(block) = nearest.and_then(|index| project.blocks.get(index)) {
                    self.set_current(block.id);
                }
            } else {
                self.repair_current(project, previous);
            }
        } else {
            self.members.extend(intersects);
        }
    }

    fn repair_current(&mut self, project: &Project, previous: Option<usize>) {
        if self.current.is_some_and(|id| self.contains(id)) {
            return;
        }
        let previous = previous.unwrap_or(0);
        let closest = self
            .indices(project)
            .into_iter()
            .min_by_key(|index| (index.abs_diff(previous), std::cmp::Reverse(*index)));
        if let Some(index) = closest {
            self.set_current(project.blocks[index].id);
        } else if let Some(id) = self
            .current
            .filter(|id| project.blocks.iter().any(|block| block.id == *id))
        {
            self.set_current(id);
        }
    }
}
