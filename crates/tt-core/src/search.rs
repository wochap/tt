//! Fuzzy task search over title, tag names, and metadata values.

use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::model::{Task, View};

/// One scored match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit<'a> {
    pub task: &'a Task,
    pub score: u32,
}

/// Text a task is matched against; also used as picker labels.
#[must_use]
pub fn haystacks(view: &View, task: &Task) -> Vec<String> {
    let mut fields = vec![task.title.clone(), format!("#{}", task.seq)];
    fields.extend(
        task.tags
            .iter()
            .filter_map(|id| view.workspace.tags.get(id))
            .map(|tag| tag.name.clone()),
    );
    fields.extend(task.metadata.values().cloned());
    if let Some(project) = task.project.and_then(|id| view.workspace.projects.get(&id)) {
        fields.push(project.name.clone());
    }
    fields
}

/// Tasks matching `query` (fuzzy, case-insensitive) ordered by best score,
/// ties broken by lowest seq. `filter` selects candidate tasks.
pub fn search<'a>(view: &'a View, query: &str, filter: impl Fn(&Task) -> bool) -> Vec<Hit<'a>> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buffer = Vec::new();
    let mut hits: Vec<Hit<'a>> = view
        .workspace
        .tasks
        .values()
        .filter(|task| filter(task))
        .filter_map(|task| {
            let fields = haystacks(view, task);
            let mut best = None::<u32>;
            for field in fields.iter().chain(std::iter::once(&fields.join(" "))) {
                if let Some(score) = pattern.score(Utf32Str::new(field, &mut buffer), &mut matcher)
                {
                    best = Some(best.map_or(score, |current| current.max(score)));
                }
            }
            best.map(|score| Hit { task, score })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.task.seq.cmp(&b.task.seq)));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskState;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn add(view: &mut View, seq: u64, title: &str, metadata: &[(&str, &str)]) {
        let task = Task {
            id: Uuid::now_v7(),
            seq,
            title: title.into(),
            description: String::new(),
            tags: Default::default(),
            project: None,
            metadata: metadata
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
            state: TaskState::Open,
            previous_seqs: Vec::new(),
            created: Utc::now(),
            updated: Utc::now(),
        };
        view.workspace.tasks.insert(task.id, task);
    }

    #[test]
    fn ticket_id_in_metadata_ranks_first() {
        let mut view = View::default();
        add(&mut view, 1, "Project planning 2023", &[]);
        add(&mut view, 2, "Login bug", &[("ticket", "PROJ-123")]);
        add(&mut view, 3, "Write docs", &[]);
        let hits = search(&view, "proj123", |_| true);
        assert_eq!(hits[0].task.seq, 2);
        assert!(hits.iter().all(|hit| hit.task.seq != 3));
    }

    #[test]
    fn filter_and_empty_results() {
        let mut view = View::default();
        add(&mut view, 1, "alpha", &[]);
        assert!(search(&view, "alpha", |task| task.seq != 1).is_empty());
        assert!(search(&view, "zzz", |_| true).is_empty());
    }
}
