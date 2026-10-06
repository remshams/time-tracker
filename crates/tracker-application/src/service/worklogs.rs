//! Worklog history queries and commands.

use chrono::{DateTime, Utc};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

use super::{
    TrackerApplication, WorklogOperations, WorklogQueries, canonical_timestamp,
    canonical_worklog_times,
};
use crate::{
    ApplicationError, GlobalWorklogCursor, GlobalWorklogPage, MoveCandidate, RepositoryError,
    TaskListItem, TrackerRepository, WorklogCursor, WorklogPage,
};

/// Selects move destinations from a caller's cached task snapshot.
/// Archived tasks and the source task are excluded even when the query is empty.
pub fn move_candidates_for_tasks(
    items: &[TaskListItem],
    source_task_id: TaskId,
    query: &str,
) -> Vec<MoveCandidate> {
    let mut matches = items
        .iter()
        .filter(|item| {
            !item.task.is_archived()
                && item.task.id() != source_task_id
                && crate::task_search::fuzzy_match(item.task.name().as_str(), query)
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|item| {
        crate::task_search::SearchRank::from_task_activity(
            item.task.id(),
            item.task.created_at(),
            item.task.updated_at(),
            item.latest_work_start,
        )
    });
    matches
        .into_iter()
        .map(|item| MoveCandidate {
            id: item.task.id(),
            name: item.task.name().to_string(),
        })
        .collect()
}

impl<R: TrackerRepository> WorklogQueries for TrackerApplication<R> {
    fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        let page = self.repository.global_worklog_page(after)?;
        self.adopt_snapshot(page.snapshot.clone())?;
        Ok(page)
    }

    fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        let page = self.repository.worklog_page(task_id, after)?;
        self.adopt_worklog_page_snapshot(task_id, page.snapshot.clone())?;
        Ok(page)
    }
}

impl<R: TrackerRepository> WorklogOperations for TrackerApplication<R> {
    fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        let expected = canonical_worklog_times(expected);
        let current = match self.repository.find_worklog(id) {
            Ok(Some(worklog)) => worklog,
            Ok(None) => {
                return Err(
                    self.recover_after_worklog_move(RepositoryError::WorklogNotFound { id })
                );
            }
            Err(error) => return Err(self.recover_after_worklog_move(error)),
        };
        if current.task_id() != expected_source_task_id || current.times() != expected {
            return Err(self.recover_after_worklog_move(RepositoryError::WorklogChanged { id }));
        }
        current.moved_to(destination_task_id)?;

        let movement = match self.repository.compare_and_move_worklog(
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        ) {
            Ok(movement) => movement,
            Err(error) => return Err(self.recover_after_worklog_move(error)),
        };
        self.adopt_worklog_move(expected_source_task_id, movement)
    }

    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        let expected = canonical_worklog_times(expected);
        let replacement = canonical_worklog_times(replacement);
        let occurred_at = canonical_timestamp(occurred_at);

        let current = match self.repository.find_worklog(id) {
            Ok(Some(worklog)) => worklog,
            Ok(None) => {
                return Err(
                    self.recover_after_worklog_correction(RepositoryError::WorklogNotFound { id })
                );
            }
            Err(error) => return Err(self.recover_after_worklog_correction(error)),
        };
        if current.times() != expected {
            return Err(
                self.recover_after_worklog_correction(RepositoryError::WorklogChanged { id })
            );
        }
        current.corrected(replacement, occurred_at)?;

        let correction =
            match self
                .repository
                .compare_and_set_worklog_times(id, expected, replacement)
            {
                Ok(correction) => correction,
                Err(error) => return Err(self.recover_after_worklog_correction(error)),
            };
        let worklog = self.adopt_worklog_correction(correction)?;
        Ok(worklog)
    }

    fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        let expected = canonical_worklog_times(expected);
        if expected.is_active() {
            return Err(ApplicationError::WorklogDeletionWrite {
                write: RepositoryError::WorklogIsActive { id },
            });
        }
        let deletion = match self.repository.compare_and_delete_completed_worklog(
            id,
            expected_task_id,
            expected,
        ) {
            Ok(deletion) => deletion,
            Err(error) => return Err(self.recover_after_worklog_deletion(error)),
        };
        let worklog = self.adopt_worklog_deletion(deletion);
        Ok(worklog)
    }
}
