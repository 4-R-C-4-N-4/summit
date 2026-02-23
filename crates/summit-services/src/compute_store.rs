use crate::compute_types::{TaskResult, TaskStatus, TaskSubmit};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Full state of a compute task.
#[derive(Debug, Clone)]
pub struct ComputeTask {
    /// Original submission.
    pub submit: TaskSubmit,
    /// Current status.
    pub status: TaskStatus,
    /// Result, populated when the task completes.
    pub result: Option<TaskResult>,
    /// Unix ms when the task was first submitted.
    pub submitted_at: u64,
    /// Unix ms when the status was last changed.
    pub updated_at: u64,
}

/// In-memory store for compute tasks.
#[derive(Clone, Default)]
pub struct ComputeStore {
    /// task_id → ComputeTask
    tasks: Arc<DashMap<String, ComputeTask>>,
    /// peer pubkey → list of task_ids they submitted
    peer_tasks: Arc<DashMap<[u8; 32], Vec<String>>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ComputeStore {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(DashMap::new()),
            peer_tasks: Arc::new(DashMap::new()),
        }
    }

    /// Store a new task submission. Duplicate `task_id`s are silently ignored.
    pub fn submit(&self, peer_pubkey: [u8; 32], submit: TaskSubmit) {
        let task_id = submit.task_id.clone();
        self.tasks.entry(task_id.clone()).or_insert_with(|| ComputeTask {
            submit,
            status: TaskStatus::Queued,
            result: None,
            submitted_at: now_ms(),
            updated_at: now_ms(),
        });
        self.peer_tasks
            .entry(peer_pubkey)
            .or_default()
            .push(task_id);
    }

    /// Record a peer acknowledgment and update task status.
    pub fn ack(&self, task_id: &str, status: TaskStatus) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.status = status;
            task.updated_at = now_ms();
        }
    }

    /// Update task status.
    pub fn update_status(&self, task_id: &str, status: TaskStatus) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.status = status;
            task.updated_at = now_ms();
        }
    }

    /// Store a task result and mark the task as Completed.
    pub fn store_result(&self, result: TaskResult) {
        if let Some(mut task) = self.tasks.get_mut(&result.task_id) {
            task.status = TaskStatus::Completed;
            task.updated_at = now_ms();
            task.result = Some(result);
        }
    }

    /// Get all task_ids submitted by a peer.
    pub fn tasks_for_peer(&self, peer_pubkey: &[u8; 32]) -> Vec<String> {
        self.peer_tasks
            .get(peer_pubkey)
            .map(|ids| ids.clone())
            .unwrap_or_default()
    }

    /// Look up a task by id.
    pub fn get_task(&self, task_id: &str) -> Option<ComputeTask> {
        self.tasks.get(task_id).map(|t| t.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute_types::TaskSubmit;

    fn make_submit(task_id: &str) -> TaskSubmit {
        TaskSubmit {
            task_id: task_id.to_string(),
            sender: "a".repeat(64),
            timestamp: 100,
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn new_creates_empty_store() {
        let store = ComputeStore::new();
        let peer = [1u8; 32];
        assert!(store.tasks_for_peer(&peer).is_empty());
        assert!(store.get_task("nonexistent").is_none());
    }

    #[test]
    fn submit_and_get_task() {
        let store = ComputeStore::new();
        let peer = [1u8; 32];
        store.submit(peer, make_submit("task-1"));

        let task = store.get_task("task-1").unwrap();
        assert_eq!(task.submit.task_id, "task-1");
        assert_eq!(task.status, TaskStatus::Queued);
        assert!(task.result.is_none());
    }

    #[test]
    fn duplicate_submit_is_ignored() {
        let store = ComputeStore::new();
        let peer = [1u8; 32];
        store.submit(peer, make_submit("task-1"));
        store.submit(peer, make_submit("task-1")); // duplicate

        // Only one task in the map
        assert!(store.get_task("task-1").is_some());
    }

    #[test]
    fn tasks_for_peer() {
        let store = ComputeStore::new();
        let peer = [2u8; 32];
        store.submit(peer, make_submit("a"));
        store.submit(peer, make_submit("b"));

        let ids = store.tasks_for_peer(&peer);
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
    }

    #[test]
    fn update_status_changes_status() {
        let store = ComputeStore::new();
        let peer = [1u8; 32];
        store.submit(peer, make_submit("task-1"));
        store.update_status("task-1", TaskStatus::Running);

        assert_eq!(store.get_task("task-1").unwrap().status, TaskStatus::Running);
    }

    #[test]
    fn store_result_marks_completed() {
        let store = ComputeStore::new();
        let peer = [1u8; 32];
        store.submit(peer, make_submit("task-1"));

        let result = TaskResult {
            task_id: "task-1".to_string(),
            result: serde_json::json!({ "output": 42 }),
            elapsed_ms: 500,
        };
        store.store_result(result);

        let task = store.get_task("task-1").unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.result.is_some());
        assert_eq!(task.result.unwrap().elapsed_ms, 500);
    }
}
