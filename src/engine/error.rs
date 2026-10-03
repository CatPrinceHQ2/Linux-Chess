//! Engine errors with a friendly message for normal users and technical details for debugging.
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum EngineError {
    /// The executable could not be started at all.
    StartFailed { path: PathBuf, source: std::io::Error },
    /// The program started but did not complete the UCI handshake.
    NotUci { path: PathBuf, detail: String },
    /// The engine stopped responding.
    Timeout { what: &'static str },
    /// The engine process ended (while idle or while calculating).
    Exited { while_searching: bool, status: String, stderr_tail: Vec<String> },
    /// Writing to the engine failed (usually because it has exited).
    Io(std::io::Error),
    /// A setting was rejected before reaching the engine.
    InvalidOption(String),
    /// The requested operation is not possible in the engine's current state.
    Busy(&'static str),
    /// The engine produced something we could not make sense of.
    InvalidResponse(String),
    /// The position handed to the engine failed validation.
    InvalidPosition(String),
}

impl EngineError {
    /// Short, plain-language text suitable for a dialog or banner.
    pub fn user_message(&self) -> String {
        match self {
            EngineError::StartFailed { .. } => "The selected executable could not be started.".into(),
            EngineError::NotUci { .. } => "The program did not respond correctly to the UCI handshake.".into(),
            EngineError::Timeout { .. } => "The engine stopped responding.".into(),
            EngineError::Exited { while_searching: true, .. } => "The engine process stopped while calculating.".into(),
            EngineError::Exited { .. } => "The engine process exited unexpectedly.".into(),
            EngineError::Io(_) => "Could not communicate with the engine.".into(),
            EngineError::InvalidOption(m) => m.clone(),
            EngineError::Busy(_) => "The engine is busy. Stop the current search first.".into(),
            EngineError::InvalidResponse(_) => "The engine sent an invalid response.".into(),
            EngineError::InvalidPosition(_) => "The supplied FEN is invalid.".into(),
        }
    }

    /// Everything useful for a bug report, for an expandable "Technical details" section.
    pub fn technical_details(&self) -> String {
        match self {
            EngineError::StartFailed { path, source } => format!("Failed to spawn {}: {source}", path.display()),
            EngineError::NotUci { path, detail } => format!("{} failed the UCI handshake: {detail}", path.display()),
            EngineError::Timeout { what } => format!("Timed out waiting for: {what}"),
            EngineError::Exited { status, stderr_tail, .. } => {
                let mut s = format!("Exit status: {status}");
                if !stderr_tail.is_empty() {
                    s.push_str("\nLast stderr output:\n");
                    s.push_str(&stderr_tail.join("\n"));
                }
                s
            }
            EngineError::Io(e) => format!("I/O error: {e}"),
            EngineError::InvalidOption(m) => m.clone(),
            EngineError::Busy(w) => format!("Operation '{w}' rejected while the engine is busy"),
            EngineError::InvalidResponse(r) => format!("Unparseable engine output: {r}"),
            EngineError::InvalidPosition(r) => r.clone(),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for EngineError {}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e)
    }
}
