//! The backend's error type and the checked index conversions it fails through.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

/// An error encountered during code emission.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmitError {
    /// The error message.
    pub message: String,
    /// The generated function the blocker fired in, and where it came from in
    /// source: `` `buildRegExpStr` at src/router/reg-exp-router/node.ts:1234..1560 ``.
    ///
    /// Emitter blockers are raised deep in emission — a list mutation, a tuple
    /// index, a coercion — and carried a message about the SHAPE only, with
    /// nothing about where it was. A whole-crate build of Hono printed
    /// `list unshift item must match the list element type` and stopped, and
    /// pinning that meant bisecting the manifest's `exclude` list one directory
    /// at a time (two rounds' worth of notes say so: H48's closing section and
    /// H54's opening one).
    ///
    /// It is set ONCE, by the function-emission entry points, on the way out. A
    /// blocker cannot know its site (the deep helpers have no reason to hold
    /// one) and the driver cannot know the message, so the two meet here.
    /// `Option` rather than a `String` because a synthesized function has no
    /// source of its own, and because a blocker raised outside function
    /// emission (a class shape, a prelude gate) has no function to name.
    pub site: Option<String>,
}

impl std::fmt::Display for EmitError {
    /// Formats the error message for display, naming the site when known.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.site {
            Some(site) => write!(formatter, "{} (in {site})", self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::fmt::Debug for EmitError {
    /// Formats the same text as [`Display`](std::fmt::Display).
    ///
    /// A failed `smelt build` reaches the terminal through `Box<dyn Error>`,
    /// which Rust's runtime prints with `Debug`. The derived form printed
    /// `EmitError { message: "list unshift item must match the list element
    /// type", site: Some("`build_reg_exp_str` at .../node.ts:4178..5003") }` --
    /// the site was there and buried in struct syntax. Delegating makes the one
    /// line a reader sees the readable one.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for EmitError {}

impl EmitError {
    /// Creates a new emit error with the given message.
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            site: None,
        }
    }

    /// Record the generated function a blocker fired in, if it has none yet.
    ///
    /// Idempotent on purpose: emission nests (a method emits a closure emits a
    /// call), so the innermost frame that knows a site wins and the outer ones
    /// leave it alone. That is why this is a field rather than text appended to
    /// the message — appending could not tell whether a site was already there
    /// without matching on the message.
    pub(crate) fn with_site(mut self, site: impl FnOnce() -> String) -> Self {
        if self.site.is_none() {
            self.site = Some(site());
        }
        self
    }
}

/// Converts a compact MIR identifier into a `usize` for indexing.
pub(crate) fn id_index(index: u32, context: &'static str) -> Result<usize, EmitError> {
    usize::try_from(index).map_err(|_err| EmitError::new(context))
}

/// Converts a `usize` into a compact MIR identifier.
pub(crate) fn compact_index(index: usize, context: &'static str) -> Result<u32, EmitError> {
    u32::try_from(index).map_err(|_err| EmitError::new(context))
}
