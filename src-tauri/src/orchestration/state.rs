use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub enum ReviewRunState {
    Created,
    RunningAgents,
    Cleaning,
    ReadyForReview,
    Submitting,
    Submitted,
    Failed,
}

#[allow(dead_code)]
impl ReviewRunState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::RunningAgents => "running_agents",
            Self::Cleaning => "cleaning",
            Self::ReadyForReview => "ready",
            Self::Submitting => "submitting",
            Self::Submitted => "submitted",
            Self::Failed => "failed",
        }
    }

    pub fn can_transition_to(&self, next: &ReviewRunState) -> bool {
        matches!(
            (self, next),
            (Self::Created, Self::RunningAgents)
                | (Self::RunningAgents, Self::Cleaning)
                | (Self::Cleaning, Self::ReadyForReview)
                | (Self::ReadyForReview, Self::Submitting)
                | (Self::Submitting, Self::Submitted)
                // Any non-terminal state can fail
                | (Self::Created, Self::Failed)
                | (Self::RunningAgents, Self::Failed)
                | (Self::Cleaning, Self::Failed)
                | (Self::ReadyForReview, Self::Failed)
                | (Self::Submitting, Self::Failed)
        )
    }
}

impl std::fmt::Display for ReviewRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        assert!(ReviewRunState::Created.can_transition_to(&ReviewRunState::RunningAgents));
        assert!(ReviewRunState::RunningAgents.can_transition_to(&ReviewRunState::Cleaning));
        assert!(ReviewRunState::Cleaning.can_transition_to(&ReviewRunState::ReadyForReview));
        assert!(ReviewRunState::ReadyForReview.can_transition_to(&ReviewRunState::Submitting));
        assert!(ReviewRunState::Submitting.can_transition_to(&ReviewRunState::Submitted));
    }

    #[test]
    fn test_any_can_fail() {
        assert!(ReviewRunState::Created.can_transition_to(&ReviewRunState::Failed));
        assert!(ReviewRunState::RunningAgents.can_transition_to(&ReviewRunState::Failed));
        assert!(ReviewRunState::Cleaning.can_transition_to(&ReviewRunState::Failed));
        assert!(ReviewRunState::Submitting.can_transition_to(&ReviewRunState::Failed));
    }

    #[test]
    fn test_invalid_transitions() {
        assert!(!ReviewRunState::Created.can_transition_to(&ReviewRunState::Cleaning));
        assert!(!ReviewRunState::Created.can_transition_to(&ReviewRunState::ReadyForReview));
        assert!(!ReviewRunState::Submitted.can_transition_to(&ReviewRunState::Failed));
        assert!(!ReviewRunState::Failed.can_transition_to(&ReviewRunState::Created));
        assert!(!ReviewRunState::RunningAgents.can_transition_to(&ReviewRunState::Submitting));
    }
}
