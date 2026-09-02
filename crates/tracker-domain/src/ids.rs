//! Stable identifiers for tasks and worklogs.
//!
//! New identifiers are UUIDv7, so sorting by identifier approximates creation
//! order. Parsing accepts any UUID text form; the domain only ever generates
//! v7 values.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new UUIDv7 identifier from the current wall clock.
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID without checking its version.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the wrapped UUID.
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// The identifier of a reusable task.
    TaskId
);

id_type!(
    /// The identifier of a worklog.
    WorklogId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_task_ids_are_v7_and_unique() {
        let first = TaskId::generate();
        let second = TaskId::generate();
        assert_eq!(first.as_uuid().get_version_num(), 7);
        assert_eq!(second.as_uuid().get_version_num(), 7);
        assert_ne!(first, second);
    }

    #[test]
    fn generated_worklog_ids_are_v7_and_unique() {
        let first = WorklogId::generate();
        let second = WorklogId::generate();
        assert_eq!(first.as_uuid().get_version_num(), 7);
        assert_ne!(first, second);
    }

    #[test]
    fn id_display_round_trips_through_parse() {
        let id = TaskId::generate();
        let parsed: TaskId = id.to_string().parse().expect("display output parses");
        assert_eq!(parsed, id);
        assert_eq!(parsed.as_uuid(), id.as_uuid());
    }

    #[test]
    fn from_uuid_wraps_the_given_uuid() {
        let uuid = Uuid::now_v7();
        let id = WorklogId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn parsing_rejects_invalid_text() {
        assert!("not-a-uuid".parse::<TaskId>().is_err());
    }

    #[test]
    fn ids_order_by_their_uuid_bytes() {
        let low: TaskId = "00000000-0000-7000-8000-000000000001".parse().unwrap();
        let high: TaskId = "00000000-0000-7000-8000-000000000002".parse().unwrap();
        assert!(low < high);
    }
}
