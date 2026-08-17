//! 会话侧栏使用的纯逻辑视图模型。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use crate::{GroupedSession, SessionMetrics, SessionSummary, group_sessions};

#[derive(Debug, Clone, Default)]
pub struct RunningSessionOverlay {
    ids: HashSet<String>,
}

impl RunningSessionOverlay {
    pub fn new(ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            ids: ids.into_iter().collect(),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionView {
    pub id: String,
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub title: String,
    pub modified: SystemTime,
    pub message_count: usize,
    pub metrics: SessionMetrics,
    pub branch: Option<String>,
    pub running: bool,
    pub children: Vec<SessionView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSessionView {
    pub key: String,
    pub root: PathBuf,
    pub modified: SystemTime,
    pub sessions: Vec<SessionView>,
}

pub fn build_session_view(
    sessions: impl IntoIterator<Item = SessionSummary>,
    running: &RunningSessionOverlay,
) -> Vec<ProjectSessionView> {
    let mut projects = Vec::new();
    for group in group_sessions(sessions) {
        let modified = group
            .sessions
            .iter()
            .map(|item| item.session.modified)
            .max()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        projects.push(ProjectSessionView {
            key: group.key,
            root: group.root,
            modified,
            sessions: build_tree(group.sessions, running),
        });
    }
    projects.sort_by_key(|project| std::cmp::Reverse(project.modified));
    projects
}

fn build_tree(sessions: Vec<GroupedSession>, running: &RunningSessionOverlay) -> Vec<SessionView> {
    let by_id: HashMap<String, GroupedSession> = sessions
        .into_iter()
        .map(|item| (item.session.id.clone(), item))
        .collect();
    let mut children = HashMap::<String, Vec<String>>::new();
    let mut roots = Vec::new();

    for item in by_id.values() {
        let Some(parent) = valid_parent(&item.session, &by_id) else {
            roots.push(item.session.id.clone());
            continue;
        };
        children
            .entry(parent)
            .or_default()
            .push(item.session.id.clone());
    }

    fn sort_ids(ids: &mut [String], by_id: &HashMap<String, GroupedSession>) {
        ids.sort_by_key(|id| std::cmp::Reverse(by_id[id].session.modified));
    }
    sort_ids(&mut roots, &by_id);
    for ids in children.values_mut() {
        sort_ids(ids, &by_id);
    }

    roots
        .into_iter()
        .map(|id| build_node(&id, &by_id, &children, running))
        .collect()
}

fn valid_parent(
    summary: &SessionSummary,
    by_id: &HashMap<String, GroupedSession>,
) -> Option<String> {
    let parent = summary.parent_session_id.as_ref()?;
    if !by_id.contains_key(parent) || parent == &summary.id {
        return None;
    }
    let mut current = parent.as_str();
    let mut visited = HashSet::from([summary.id.as_str()]);
    while let Some(next) = by_id
        .get(current)
        .and_then(|item| item.session.parent_session_id.as_deref())
    {
        if !visited.insert(current) || next == summary.id {
            return None;
        }
        if !by_id.contains_key(next) {
            break;
        }
        current = next;
    }
    Some(parent.clone())
}

fn build_node(
    id: &str,
    by_id: &HashMap<String, GroupedSession>,
    children: &HashMap<String, Vec<String>>,
    running: &RunningSessionOverlay,
) -> SessionView {
    let item = &by_id[id];
    let summary = &item.session;
    let title = summary
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            let fallback = summary.first_message.trim();
            (!fallback.is_empty() && fallback != "(no messages)").then_some(fallback)
        })
        .unwrap_or(&summary.id)
        .to_owned();
    SessionView {
        id: summary.id.clone(),
        path: summary.path.clone(),
        cwd: summary.cwd.clone(),
        title,
        modified: summary.modified,
        message_count: summary.message_count,
        metrics: summary.metrics.clone(),
        branch: item
            .project
            .is_worktree
            .then(|| item.project.branch.clone())
            .flatten(),
        running: running.contains(&summary.id),
        children: children
            .get(id)
            .into_iter()
            .flatten()
            .map(|child| build_node(child, by_id, children, running))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn summary(id: &str, parent: Option<&str>, seconds: u64) -> SessionSummary {
        SessionSummary {
            path: PathBuf::from(format!("{id}.jsonl")),
            revision: crate::SessionRevision {
                len: 0,
                modified: SystemTime::UNIX_EPOCH,
                fingerprint: 0,
            },
            id: id.to_owned(),
            cwd: PathBuf::from("missing-project"),
            name: None,
            created: UNIX_EPOCH,
            modified: UNIX_EPOCH + Duration::from_secs(seconds),
            message_count: 2,
            first_message: format!("title {id}"),
            parent_session_path: None,
            parent_session_id: parent.map(str::to_owned),
            metrics: SessionMetrics::default(),
        }
    }

    #[test]
    fn builds_sorted_tree_and_applies_running_overlay() {
        let running = RunningSessionOverlay::new(["child".to_owned()]);
        let projects = build_session_view(
            [summary("root", None, 1), summary("child", Some("root"), 2)],
            &running,
        );
        assert_eq!(projects[0].sessions[0].id, "root");
        assert_eq!(projects[0].sessions[0].message_count, 2);
        assert_eq!(projects[0].sessions[0].children[0].id, "child");
        assert!(projects[0].sessions[0].children[0].running);
    }

    #[test]
    fn cycles_become_roots_instead_of_recursing() {
        let projects = build_session_view(
            [summary("a", Some("b"), 1), summary("b", Some("a"), 2)],
            &RunningSessionOverlay::default(),
        );
        assert_eq!(projects[0].sessions.len(), 2);
    }
}
