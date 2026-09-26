//! Diagnostics: what the compiler reports about a source file.
//!
//! Every diagnostic has a stable `code` (see [`code`]), so tools such as the
//! language server key their quick fixes on the code rather than on the
//! wording of the message. `subject` and `suggestion` carry the name the
//! diagnostic is about and the name to use instead, when there is one.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Help,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Stable identifier of the kind of problem, e.g. `unknown-element`.
    pub code: &'static str,
    pub line: usize,
    pub column: Option<usize>,
    pub message: String,
    pub severity: Severity,
    pub source_line: Option<Box<str>>,
    /// The name the diagnostic is about, as written (`@nosuch` without the
    /// `@`, an attribute key, a definition's name).
    pub subject: Option<Box<str>>,
    /// The name to write instead of `subject` ("did you mean ...?").
    pub suggestion: Option<Box<str>>,
}

impl Diagnostic {
    pub fn new(code: &'static str, severity: Severity, line: usize, message: String) -> Self {
        Diagnostic {
            code,
            line,
            column: None,
            message,
            severity,
            source_line: None,
            subject: None,
            suggestion: None,
        }
    }

    pub fn error(code: &'static str, line: usize, message: String) -> Self {
        Self::new(code, Severity::Error, line, message)
    }

    pub fn warning(code: &'static str, line: usize, message: String) -> Self {
        Self::new(code, Severity::Warning, line, message)
    }

    pub fn column(mut self, column: usize) -> Self {
        self.column = Some(column);
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source_line = Some(source.into().into_boxed_str());
        self
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into().into_boxed_str());
        self
    }

    pub fn suggest(mut self, suggestion: Option<impl Into<String>>) -> Self {
        self.suggestion = suggestion.map(|s| s.into().into_boxed_str());
        self
    }
}

/// The diagnostic codes. A code names a kind of problem and never changes,
/// even when a message is reworded.
pub mod code {
    // Syntax: found while reading the file, before anything runs
    pub const UNCLOSED_BRACKET: &str = "unclosed-bracket";
    pub const UNEXPECTED_BODY: &str = "unexpected-body";
    pub const UNEXPECTED_ARGUMENT: &str = "unexpected-argument";
    pub const MISSING_ARGUMENT: &str = "missing-argument";
    pub const INVALID_LOOP: &str = "invalid-loop";
    /// A `@let` whose name, parameters or body don't fit its kind.
    pub const INVALID_DEFINITION: &str = "invalid-definition";
    pub const STRAY_ELSE: &str = "stray-else";

    // Names
    pub const UNKNOWN_ELEMENT: &str = "unknown-element";
    /// An attribute name htmlang doesn't know: a warning when CSS could
    /// have the name (it is written to the CSS as it is), an error otherwise.
    pub const UNKNOWN_ATTRIBUTE: &str = "unknown-attribute";
    /// A misspelled prefix (`hovr:color red`).
    pub const UNKNOWN_PREFIX: &str = "unknown-prefix";
    /// A prefix the attribute can't take: a second one (`md:hover:`), or
    /// one on an HTML attribute (`hover:required`).
    pub const INVALID_PREFIX: &str = "invalid-prefix";
    /// A style written without its value (`[padding]`).
    pub const MISSING_VALUE: &str = "missing-value";
    pub const HTML_ATTRIBUTE_FORM: &str = "html-attribute-form";
    pub const UNKNOWN_PAGE_ATTRIBUTE: &str = "unknown-page-attribute";
    pub const UNDEFINED_VARIABLE: &str = "undefined-variable";
    /// An attribute, or an attribute's name, made from a variable.
    pub const ATTRIBUTE_FROM_VARIABLE: &str = "attribute-from-variable";
    pub const DUPLICATE_ATTRIBUTE: &str = "duplicate-attribute";

    // Values
    /// A value that can't be written into the page's CSS (an error), or
    /// one that can't mean what it says (a warning).
    pub const INVALID_VALUE: &str = "invalid-value";
    pub const INVALID_COLOR: &str = "invalid-color";
    pub const INVALID_EXPRESSION: &str = "invalid-expression";
    pub const INVALID_JSON: &str = "invalid-json";

    // Definitions and functions
    pub const UNUSED_VARIABLE: &str = "unused-variable";
    pub const UNUSED_BUNDLE: &str = "unused-bundle";
    pub const UNUSED_FUNCTION: &str = "unused-function";
    pub const RECURSIVE_CALL: &str = "recursive-call";
    pub const NO_SINGLE_ROOT: &str = "no-single-root";
    /// A call that leaves out a parameter without a default.
    pub const MISSING_PARAMETER: &str = "missing-parameter";
    /// A parameter passed `name=value`: parameters are written `name value`.
    pub const PARAMETER_FORM: &str = "parameter-form";
    /// A function named like a built-in element (which it replaces) or a
    /// directive (which it can't replace).
    pub const SHADOWS_BUILT_IN: &str = "shadows-built-in";
    /// A `@slot NAME` block at a call whose function has no such slot.
    pub const UNKNOWN_SLOT: &str = "unknown-slot";
    /// Content passed to a function whose body has no `@children`.
    pub const UNEXPECTED_CONTENT: &str = "unexpected-content";
    /// A `@slot` or `@children` where it marks or fills nothing: outside
    /// a function's body, inside an element at a call, or inline in text.
    pub const MISPLACED_SLOT: &str = "misplaced-slot";
    /// A slot name that isn't one name (`@slot my footer`).
    pub const INVALID_SLOT_NAME: &str = "invalid-slot-name";

    // Files
    pub const UNREADABLE_FILE: &str = "unreadable-file";
    pub const CIRCULAR_INCLUDE: &str = "circular-include";
    pub const UNSET_ENVIRONMENT: &str = "unset-environment";

    // Context: an attribute or element where it has no effect
    pub const NO_EFFECT: &str = "no-effect";

    // Accessibility
    pub const MISSING_ALT: &str = "missing-alt";
    pub const MISSING_INPUT_TYPE: &str = "missing-input-type";
    pub const MISSING_LINK_TEXT: &str = "missing-link-text";
    pub const MISSING_LABEL: &str = "missing-label";
    pub const MISSING_TITLE: &str = "missing-title";
    pub const MISSING_BUTTON_TEXT: &str = "missing-button-text";
    pub const MISSING_CAPTIONS: &str = "missing-captions";
    pub const LOW_CONTRAST: &str = "low-contrast";
    pub const POSITIVE_TABINDEX: &str = "positive-tabindex";

    // `htmlang lint`
    pub const DEEP_NESTING: &str = "deep-nesting";
    pub const EMPTY_CONTAINER: &str = "empty-container";
    pub const MISSING_BUTTON_TYPE: &str = "missing-button-type";

    /// A bug in the compiler itself.
    pub const INTERNAL: &str = "internal";

    /// Every code, for tools and tests.
    pub const ALL: &[&str] = &[
        UNCLOSED_BRACKET,
        UNEXPECTED_BODY,
        UNEXPECTED_ARGUMENT,
        MISSING_ARGUMENT,
        INVALID_LOOP,
        INVALID_DEFINITION,
        STRAY_ELSE,
        UNKNOWN_ELEMENT,
        UNKNOWN_ATTRIBUTE,
        UNKNOWN_PREFIX,
        INVALID_PREFIX,
        MISSING_VALUE,
        HTML_ATTRIBUTE_FORM,
        UNKNOWN_PAGE_ATTRIBUTE,
        UNDEFINED_VARIABLE,
        ATTRIBUTE_FROM_VARIABLE,
        DUPLICATE_ATTRIBUTE,
        INVALID_VALUE,
        INVALID_COLOR,
        INVALID_EXPRESSION,
        INVALID_JSON,
        UNUSED_VARIABLE,
        UNUSED_BUNDLE,
        UNUSED_FUNCTION,
        RECURSIVE_CALL,
        NO_SINGLE_ROOT,
        MISSING_PARAMETER,
        PARAMETER_FORM,
        SHADOWS_BUILT_IN,
        UNKNOWN_SLOT,
        UNEXPECTED_CONTENT,
        MISPLACED_SLOT,
        INVALID_SLOT_NAME,
        UNREADABLE_FILE,
        CIRCULAR_INCLUDE,
        UNSET_ENVIRONMENT,
        NO_EFFECT,
        MISSING_ALT,
        MISSING_INPUT_TYPE,
        MISSING_LINK_TEXT,
        MISSING_LABEL,
        MISSING_TITLE,
        MISSING_BUTTON_TEXT,
        MISSING_CAPTIONS,
        LOW_CONTRAST,
        POSITIVE_TABINDEX,
        DEEP_NESTING,
        EMPTY_CONTAINER,
        MISSING_BUTTON_TYPE,
        INTERNAL,
    ];
}
