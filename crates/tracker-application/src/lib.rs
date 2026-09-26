//! Backend-neutral application use cases for tasks and time tracking.

mod error;
mod model;
mod repository;
mod service;

pub use error::{ApplicationError, ApplicationFailure, ApplicationFailureCategory};
pub use model::{
    ClearActiveTaskOutcome, GlobalWorklogCursor, GlobalWorklogPage, ReportRow, ReportTotals,
    SetActiveTaskOutcome, TaskListItem, TaskOrdering, WORKLOG_PAGE_SIZE, WorklogCursor,
    WorklogPage, WorklogPageSnapshot,
};
pub use repository::{
    ReportRead, ReportRepository, RepositoryError, TaskRepository, TrackerRepository,
    TrackerSnapshot, TrackingRepository, WorklogCorrection, WorklogDeletion, WorklogMove,
    WorklogRepository,
};
pub use service::{
    ReportQueries, TaskOperations, TaskQueries, TrackerApplication, TrackerApplicationService,
    TrackingOperations, WorklogOperations, WorklogQueries,
};

#[cfg(test)]
mod tests;
