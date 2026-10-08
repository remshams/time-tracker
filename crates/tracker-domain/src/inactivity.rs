//! Inactivity period used when selecting tasks for bulk archiving.

/// A positive number of complete 24-hour days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InactivityPeriod(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("inactivity period must be at least one day")]
pub struct InactivityPeriodError;

impl InactivityPeriod {
    pub fn new(days: u32) -> Result<Self, InactivityPeriodError> {
        if days == 0 {
            Err(InactivityPeriodError)
        } else {
            Ok(Self(days))
        }
    }

    pub fn days(self) -> u32 {
        self.0
    }
}

impl Default for InactivityPeriod {
    fn default() -> Self {
        Self(14)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_positive_days_and_preserves_the_chosen_period() {
        assert_eq!(InactivityPeriod::new(0), Err(InactivityPeriodError));
        for days in [1, 7, 14, 30, u32::MAX] {
            assert_eq!(InactivityPeriod::new(days).unwrap().days(), days);
        }
        assert_eq!(InactivityPeriod::default().days(), 14);
        assert_eq!(
            InactivityPeriodError.to_string(),
            "inactivity period must be at least one day"
        );
    }
}
