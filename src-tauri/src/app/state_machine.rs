use crate::app::error::{AppError, AppResult};

/// Recording session states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
    Completed,
    Failed,
}

impl RecordingState {
    pub fn as_str(self) -> &'static str {
        match self {
            RecordingState::Idle => "idle",
            RecordingState::Recording => "recording",
            RecordingState::Processing => "processing",
            RecordingState::Completed => "completed",
            RecordingState::Failed => "failed",
        }
    }
}

/// State machine guarding valid recording transitions.
#[derive(Debug)]
pub struct RecordingStateMachine {
    state: RecordingState,
}

impl RecordingStateMachine {
    pub fn new() -> Self {
        Self {
            state: RecordingState::Idle,
        }
    }

    pub fn state(&self) -> RecordingState {
        self.state
    }

    pub fn start(&mut self) -> AppResult<()> {
        match self.state {
            RecordingState::Idle | RecordingState::Completed | RecordingState::Failed => {
                self.state = RecordingState::Recording;
                Ok(())
            }
            _ => Err(AppError::InvalidState {
                current: self.state.as_str(),
                action: "start",
            }),
        }
    }

    pub fn stop(&mut self) -> AppResult<()> {
        match self.state {
            RecordingState::Recording => {
                self.state = RecordingState::Processing;
                Ok(())
            }
            _ => Err(AppError::InvalidState {
                current: self.state.as_str(),
                action: "stop",
            }),
        }
    }

    pub fn complete(&mut self) -> AppResult<()> {
        match self.state {
            RecordingState::Processing => {
                self.state = RecordingState::Completed;
                Ok(())
            }
            _ => Err(AppError::InvalidState {
                current: self.state.as_str(),
                action: "complete",
            }),
        }
    }

    pub fn fail(&mut self) {
        self.state = RecordingState::Failed;
    }
}

impl Default for RecordingStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_machine_starts_idle() {
        let machine = RecordingStateMachine::new();

        assert_eq!(machine.state(), RecordingState::Idle);
    }

    #[test]
    fn can_start_stop_and_complete_recording() {
        let mut machine = RecordingStateMachine::new();

        machine.start().unwrap();
        assert_eq!(machine.state(), RecordingState::Recording);

        machine.stop().unwrap();
        assert_eq!(machine.state(), RecordingState::Processing);

        machine.complete().unwrap();
        assert_eq!(machine.state(), RecordingState::Completed);
    }

    #[test]
    fn cannot_stop_when_idle() {
        let mut machine = RecordingStateMachine::new();

        let error = machine.stop().unwrap_err();

        assert_eq!(
            error,
            AppError::InvalidState {
                current: "idle",
                action: "stop"
            }
        );
    }

    #[test]
    fn failed_state_can_start_again() {
        let mut machine = RecordingStateMachine::new();
        machine.fail();

        machine.start().unwrap();

        assert_eq!(machine.state(), RecordingState::Recording);
    }
}
