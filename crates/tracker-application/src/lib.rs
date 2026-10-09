//! Backend-neutral application use cases for tasks and time tracking.

pub mod calendar_reports;
pub mod task_search;

mod error;
mod model;
mod repository;
mod service;

pub use error::{
    ApplicationError, ApplicationFailure, ApplicationFailureCategory, ApplicationFailureSource,
};
pub use model::{
    ClearActiveTaskOutcome, GlobalWorklogCursor, GlobalWorklogPage, MoveCandidate, ReportRow,
    ReportTotals, SetActiveTaskOutcome, TaskListItem, TaskOrdering, WORKLOG_PAGE_SIZE,
    WorklogCursor, WorklogPage, WorklogPageSnapshot,
};
pub use repository::{
    ActiveTrackingRead, InactiveTaskArchive, InactiveTaskPreviewRead, InactiveTaskRepository,
    ReportRead, ReportRepository, RepositoryError, TaskRepository, TrackerRepository,
    TrackingRepository, WorklogCorrection, WorklogDeletion, WorklogMove, WorklogRepository,
};
pub use service::move_candidates_for_tasks;
pub use service::{
    InactiveTaskOperations, ReportQueries, TaskOperations, TaskQueries, TrackerApplication,
    TrackerApplicationService, TrackingOperations, WorklogOperations, WorklogQueries,
};

#[cfg(test)]
mod tests;
