use anyhow::{Result, bail};

use super::types::Status;

/// Returns `Ok(())` when the transition `from -> to` is permitted, or an
/// error describing why it is not.
pub fn validate_transition(from: Status, to: Status) -> Result<()> {
    let valid = matches!(
        (from, to),
        (Status::Triage, Status::Todo)
            | (Status::Todo, Status::Ready)
            | (Status::Ready, Status::Running)
            | (Status::Running, Status::Done)
            | (Status::Running, Status::Blocked)
            | (Status::Running, Status::Ready)   // claim expired
            | (Status::Blocked, Status::Ready)
            | (Status::Blocked, Status::Done)    // abandoned / manually resolved
            | (Status::Done, Status::Archived)
    );

    if valid {
        Ok(())
    } else {
        bail!("invalid status transition: {} -> {}", from, to)
    }
}

#[cfg(test)]
#[path = "state_machine_tests.rs"]
mod tests;
