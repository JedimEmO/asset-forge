//! What a command hands back: nothing, or a failure with an exit code.
//!
//! Two failures, because a caller — a human, a `just` recipe, an agent
//! reading the status — needs to tell two things apart: *the check did not
//! hold* (exit 1; fix the library) and *the call could not be honoured as
//! written* (exit 2; fix the call). Clap's own parse errors already exit 2,
//! so a mistyped flag and a name that does not exist read the same way.

use std::process::ExitCode;

use forge_library::LibraryError;

/// A command that did not succeed.
#[derive(Debug)]
pub(crate) enum Failure {
    /// A gate did not hold, or work that was asked for could not be done:
    /// a failing verify, a clip that will not rebuild, a bake that failed.
    /// Exit 1.
    Failed(String),
    /// The call was refused as written: an unknown kind, a file that is not
    /// there, a name already in use, no project. The message says what does
    /// exist so the next call can be right. Exit 2.
    Refused(String),
}

impl Failure {
    /// A refusal, worded for whoever has to fix the call.
    pub(crate) fn refused(message: impl Into<String>) -> Self {
        Self::Refused(message.into())
    }

    /// A gate that did not hold.
    pub(crate) fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    /// A subcommand whose phase has not landed yet. Exit 1 rather than 2:
    /// the call is well-formed, the toolkit is what is behind.
    pub(crate) fn later(phase: &str, what: &str) -> Self {
        Self::Failed(format!("not yet: lands in {phase} ({what})"))
    }

    /// The text printed on stderr.
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Failed(message) | Self::Refused(message) => message,
        }
    }

    /// The process exit code.
    pub(crate) fn code(&self) -> ExitCode {
        match self {
            Self::Failed(_) => ExitCode::from(1),
            Self::Refused(_) => ExitCode::from(2),
        }
    }
}

impl From<LibraryError> for Failure {
    /// Which library errors are the caller's to fix and which are the
    /// library's. A collision, a bad name, a bad retime spec and a missing
    /// project are all "say it differently"; everything else is a file or a
    /// bake that is not what it should be.
    fn from(error: LibraryError) -> Self {
        match error {
            LibraryError::NoProject { .. }
            | LibraryError::WouldOverwrite { .. }
            | LibraryError::Rejected { .. }
            | LibraryError::Retime { .. } => Self::Refused(error.to_string()),
            _ => Self::Failed(error.to_string()),
        }
    }
}

impl From<forge_rig::RigError> for Failure {
    fn from(error: forge_rig::RigError) -> Self {
        Self::Failed(error.to_string())
    }
}

/// What every command returns.
pub(crate) type Outcome = Result<(), Failure>;
