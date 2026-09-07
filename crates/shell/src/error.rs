//! Engine-neutral script failures and the host diagnostic sink.
//!
//! Style resolution still uses [`ShellError`]. Execution failures that cross the
//! engine boundary use [`ScriptFailure`], which keeps phase, category, and
//! optional location instead of flattening into a string.

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_HOST_FAILURE_ID: AtomicU64 = AtomicU64::new(1);

/// Maximum UTF-8 bytes retained for a failure message.
pub const MAX_FAILURE_MESSAGE_BYTES: usize = 512;
/// Maximum UTF-8 bytes retained for a script stack.
pub const MAX_FAILURE_STACK_BYTES: usize = 2048;
/// Maximum UTF-8 bytes retained for an entry name.
pub const MAX_FAILURE_ENTRY_BYTES: usize = 128;
/// Maximum UTF-8 bytes retained for a source path or file name.
pub const MAX_FAILURE_FILE_BYTES: usize = 256;

/// An error that will surface to the script author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellError(String);

impl ShellError {
    pub fn runtime(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShellError {}

pub type Result<T> = std::result::Result<T, ShellError>;

/// When script execution failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptPhase {
    Load,
    Init,
    Render,
    Callback,
}

impl ScriptPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Init => "init",
            Self::Render => "render",
            Self::Callback => "callback",
        }
    }
}

/// Why script execution failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptFailureCategory {
    Exception,
    ExecutionBudget,
    Engine,
}

impl ScriptFailureCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exception => "exception",
            Self::ExecutionBudget => "execution_budget",
            Self::Engine => "engine",
        }
    }
}

/// A source location extracted from a typed exception or stack, if present.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptSourceLocation {
    file: String,
    line: Option<u32>,
    column: Option<u32>,
}

impl ScriptSourceLocation {
    pub(crate) fn new(file: impl Into<String>, line: Option<u32>, column: Option<u32>) -> Self {
        Self {
            file: bound_text(&file.into(), MAX_FAILURE_FILE_BYTES),
            line,
            column,
        }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn line(&self) -> Option<u32> {
        self.line
    }

    pub fn column(&self) -> Option<u32> {
        self.column
    }
}

/// An engine-neutral record of one script failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptFailure {
    phase: ScriptPhase,
    category: ScriptFailureCategory,
    message: String,
    entry: Option<String>,
    location: Option<ScriptSourceLocation>,
    stack: Option<String>,
    correlation_id: String,
}

impl ScriptFailure {
    pub(crate) fn new(
        phase: ScriptPhase,
        category: ScriptFailureCategory,
        message: impl Into<String>,
        correlation_id: impl Into<String>,
    ) -> Self {
        let message = bound_text(&message.into(), MAX_FAILURE_MESSAGE_BYTES);
        let message = if message.is_empty() && category == ScriptFailureCategory::ExecutionBudget {
            "script execution exceeded its time budget".to_owned()
        } else if message.is_empty() {
            "script failed".to_owned()
        } else {
            message
        };
        Self {
            phase,
            category,
            message,
            entry: None,
            location: None,
            stack: None,
            correlation_id: correlation_id.into(),
        }
    }

    pub(crate) fn with_entry(mut self, entry: impl Into<String>) -> Self {
        let entry = bound_text(&entry.into(), MAX_FAILURE_ENTRY_BYTES);
        if !entry.is_empty() {
            self.entry = Some(entry);
        }
        self
    }

    pub(crate) fn with_location(mut self, location: ScriptSourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub(crate) fn with_stack(mut self, stack: impl Into<String>) -> Self {
        let stack = bound_text(&stack.into(), MAX_FAILURE_STACK_BYTES);
        if !stack.is_empty() {
            self.stack = Some(stack);
        }
        self
    }

    /// Recovers a structured failure from an engine [`anyhow::Error`].
    #[must_use]
    pub fn from_error(error: &anyhow::Error) -> Self {
        error.downcast_ref::<Self>().cloned().unwrap_or_else(|| {
            Self::new(
                ScriptPhase::Load,
                ScriptFailureCategory::Engine,
                error.to_string(),
                format!(
                    "host-{}",
                    NEXT_HOST_FAILURE_ID.fetch_add(1, Ordering::Relaxed)
                ),
            )
        })
    }

    pub fn phase(&self) -> ScriptPhase {
        self.phase
    }

    pub fn category(&self) -> ScriptFailureCategory {
        self.category
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn entry(&self) -> Option<&str> {
        self.entry.as_deref()
    }

    pub fn location(&self) -> Option<&ScriptSourceLocation> {
        self.location.as_ref()
    }

    pub fn stack(&self) -> Option<&str> {
        self.stack.as_deref()
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    /// Category and a validated main-module line without the thrown payload.
    ///
    /// Arbitrary exception strings can include host data. A host that installed
    /// a [`DiagnosticSink`] should present this summary in the default overlay
    /// and keep the raw message in the sanitized diagnostic record instead.
    pub fn safe_summary(&self) -> String {
        let mut text = format!("{} {}", self.phase.as_str(), self.category.as_str());
        if let Some(location) = self
            .location
            .as_ref()
            .filter(|location| canonical_main_module(location.file()))
        {
            text.push_str(" at ");
            text.push_str("main.js");
            if let Some(line) = location.line {
                text.push(':');
                text.push_str(&line.to_string());
            }
        }
        text
    }
}

impl fmt::Display for ScriptFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(stack) = &self.stack {
            formatter.write_str("\n")?;
            formatter.write_str(stack)?;
        }
        Ok(())
    }
}

impl std::error::Error for ScriptFailure {}

/// Receives owned failure records from the runtime.
///
/// Implementations must not re-enter JavaScript and must not mutate GPUI
/// synchronously. Append to host state and wake an observer instead.
pub trait DiagnosticSink: 'static {
    fn report(&self, failure: ScriptFailure);
}

/// A callback that is no longer live. Not a script failure.
#[derive(Debug)]
pub(crate) struct InactiveCallback;

impl fmt::Display for InactiveCallback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("inactive script callback")
    }
}

impl std::error::Error for InactiveCallback {}

/// A callback failure already delivered to the installed diagnostic sink.
///
/// Its display is safe for component adapters that render an error inline, and
/// the materializer can recognize it without reporting the failure twice.
#[derive(Debug)]
pub(crate) struct ReportedScriptFailure(ScriptFailure);

impl ReportedScriptFailure {
    pub(crate) fn new(failure: ScriptFailure) -> Self {
        Self(failure)
    }

    pub(crate) fn failure(&self) -> &ScriptFailure {
        &self.0
    }
}

impl fmt::Display for ReportedScriptFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.safe_summary())
    }
}

impl std::error::Error for ReportedScriptFailure {}

pub(crate) fn bound_text(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_owned()
}

/// Best-effort location from a QuickJS stack, used only when the exception
/// object does not carry file and line fields.
pub(crate) fn location_from_stack(stack: &str) -> Option<ScriptSourceLocation> {
    for line in stack.lines() {
        let trimmed = line.trim();
        let payload = trimmed.strip_prefix("at ").unwrap_or(trimmed);
        let (file, rest) = file_and_position(payload)?;
        if file.is_empty() || file == "<anonymous>" {
            continue;
        }
        return Some(ScriptSourceLocation::new(file, rest.0, rest.1));
    }
    None
}

pub(crate) fn entry_from_stack(stack: &str) -> Option<String> {
    let line = stack.lines().next()?.trim();
    let payload = line.strip_prefix("at ").unwrap_or(line);
    let name = payload.split_whitespace().next().unwrap_or(payload);
    if name == "at" || name.starts_with('/') || name.contains(':') {
        return None;
    }
    let name = name.trim_end_matches('(');
    if name.is_empty() {
        None
    } else {
        Some(bound_text(name, MAX_FAILURE_ENTRY_BYTES))
    }
}

fn file_and_position(payload: &str) -> Option<(&str, (Option<u32>, Option<u32>))> {
    let target = if let Some(start) = payload.rfind('(') {
        payload.get(start + 1..)?.trim_end_matches(')')
    } else {
        payload
    };
    let (file, line, column) = split_file_line_column(target)?;
    Some((file, (line, column)))
}

fn split_file_line_column(target: &str) -> Option<(&str, Option<u32>, Option<u32>)> {
    let Some((without_last, last)) = take_trailing_number(target) else {
        return Some((target, None, None));
    };
    if let Some((file, line)) = take_trailing_number(without_last) {
        Some((file, Some(line), Some(last)))
    } else {
        Some((without_last, Some(last), None))
    }
}

fn canonical_main_module(file: &str) -> bool {
    if file == "main.js" {
        return true;
    }
    file.strip_prefix("main.js?v=").is_some_and(|version| {
        !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn take_trailing_number(input: &str) -> Option<(&str, u32)> {
    let at = input.rfind(':')?;
    let number = input.get(at + 1..)?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((input.get(..at)?, number.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::{entry_from_stack, location_from_stack};

    #[test]
    fn safe_summary_omits_the_thrown_payload() {
        let failure = super::ScriptFailure::new(
            super::ScriptPhase::Render,
            super::ScriptFailureCategory::Exception,
            "secret token sk-abcdefghijklmnopqrstuvwxyz at /home/mwatts/secret.key",
            "1",
        )
        .with_entry("PRIVATE_ENTRY")
        .with_location(super::ScriptSourceLocation::new(
            "main.js",
            Some(4),
            Some(1),
        ));
        let summary = failure.safe_summary();
        assert!(summary.contains("render"));
        assert!(summary.contains("exception"));
        assert!(summary.contains("main.js:4"));
        assert!(!summary.contains("PRIVATE_ENTRY"), "{summary}");
        assert!(
            !summary.contains("sk-abcdefghijklmnopqrstuvwxyz"),
            "{summary}"
        );
        assert!(!summary.contains("/home/mwatts"), "{summary}");
    }

    #[test]
    fn safe_summary_omits_unvalidated_entry_and_location_names() {
        let failure = super::ScriptFailure::new(
            super::ScriptPhase::Callback,
            super::ScriptFailureCategory::Exception,
            "PRIVATE_MESSAGE",
            "2",
        )
        .with_entry("PRIVATE_ENTRY")
        .with_location(super::ScriptSourceLocation::new(
            "/private/PRIVATE_FILE.js",
            Some(8),
            Some(2),
        ));

        let summary = failure.safe_summary();
        assert_eq!(summary, "callback exception");
    }

    #[test]
    fn unrelated_host_failures_receive_distinct_correlation_ids() {
        let first = super::ScriptFailure::from_error(&anyhow::anyhow!("first"));
        let second = super::ScriptFailure::from_error(&anyhow::anyhow!("second"));
        assert_ne!(first.correlation_id(), second.correlation_id());
    }

    #[test]
    fn stack_location_reads_the_first_file_line_and_column() {
        let stack = "    at render (main.js:12:4)\n    at init (main.js:3:1)\n";
        let location = location_from_stack(stack).expect("location");
        assert_eq!(location.file(), "main.js");
        assert_eq!(location.line(), Some(12));
        assert_eq!(location.column(), Some(4));
        assert_eq!(entry_from_stack(stack).as_deref(), Some("render"));
    }
}
