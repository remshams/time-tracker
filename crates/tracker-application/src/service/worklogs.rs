//! Worklog history queries and commands.

use chrono::{DateTime, Utc};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

use super::{
    TrackerApplication, WorklogOperations, WorklogQueries, canonical_timestamp,
    canonical_worklog_times,
};
use crate::{ApplicationError, RepositoryError, TrackerRepository, WorklogCursor, WorklogPage};

impl<R: TrackerRepository> WorklogQueries for TrackerApplication<R> {
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
