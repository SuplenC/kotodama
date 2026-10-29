//! Views an Extension pushes on top of the search, and the jobs a progress View
//! follows. The Daemon passes them on to the Front end unchanged.
//!
//! # Owns
//!
//! - [`View`], [`ViewContent`] and the forms' fields, [`Job`] and its state.
//!
//! # Not here
//!
//! - The messages that carry them: `message.rs` and `front_end.rs`.

use serde::Deserialize;
use serde::Serialize;

use crate::Fraction;
use crate::Item;

/// A screen pushed on top of the search. Esc pops it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    /// Unique among the Extension's open Views; updating or popping a View names
    /// it by this id.
    pub id: String,
    pub title: String,
    pub content: ViewContent,
}

/// What a View shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewContent {
    /// Items to pick from; their Actions run like a search Result's, and
    /// Contributed actions are offered on them by Item kind. Typing
    /// filters them: the Daemon matches them as it does the search, without
    /// asking the Extension, unless `filters_itself` is set.
    List {
        items: Vec<Item>,
        /// The Extension filters the list itself: the Daemon sends it what is
        /// typed as [`ToExtension::FilterView`](crate::ToExtension::FilterView),
        /// and it answers with an `update_view` holding the Items to show.
        #[serde(default)]
        filters_itself: bool,
    },
    /// Fields to fill in. Submitting sends the values back to the Extension.
    Form {
        fields: Vec<FormField>,
        /// The submit button's label.
        submit: String,
    },
    /// Text to read, with a table of labels and values beside it.
    Detail {
        markdown: String,
        #[serde(default)]
        metadata: Vec<MetadataEntry>,
    },
    /// Follows the job with this id as the Extension reports its progress.
    Progress { job: String },
}

/// One field of a form View. The Daemon checks a submitted form against its
/// fields before the Extension gets it: a required field is filled in, a
/// number is one the field takes, a date is a date, a choice is among the
/// options and a text matches the `pattern`. A form that fails stays open, and
/// the Front end is told which fields are wrong and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    /// The key this field's value has in the submitted values.
    pub id: String,
    pub label: String,
    pub input: FormInput,
    /// The form may be submitted with it empty. A required field, the
    /// default, can't be: a blank text, a missing number or date, or no
    /// option chosen. A checkbox always has a value, and a description none,
    /// so neither is ever missing.
    #[serde(default)]
    pub optional: bool,
    /// A regular expression, in the syntax of Rust's `regex` crate, that the
    /// whole of a text or text area field's value must match; other fields
    /// ignore it. An optional field left empty isn't checked. The Daemon drops
    /// a pattern that isn't a regular expression and tells the Extension.
    #[serde(default)]
    pub pattern: Option<String>,
}

impl FormField {
    /// A required field `id`, labelled `label`, with no pattern; the setters
    /// below change that.
    ///
    /// ```
    /// use kotodama_protocol::FormField;
    /// use kotodama_protocol::FormInput;
    ///
    /// let minutes = FormInput::Number {
    ///     min: Some(1.0),
    ///     max: Some(120.0),
    ///     step: Some(1.0),
    ///     default: Some(25.0),
    /// };
    /// let field = FormField::new("minutes", "Minutes", minutes).optional();
    /// assert!(field.optional);
    /// ```
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>, input: FormInput) -> Self {
        FormField {
            id: id.into(),
            label: label.into(),
            input,
            optional: false,
            pattern: None,
        }
    }

    /// Lets the form be submitted with this field empty.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Has the whole of a text value match `pattern`, a regular expression.
    #[must_use]
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.pattern = Some(pattern.into());
        self
    }
}

/// A form field's control, and so the type of its submitted value: a string for
/// text, a text area, a select, a date and a file, a number for a number, a
/// boolean for a checkbox, a list of strings for a multi-select, and nothing
/// for a description.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormInput {
    /// A line of text.
    Text {
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default)]
        default: Option<String>,
        /// Hides what is typed, for passwords and tokens.
        #[serde(default)]
        secret: bool,
    },
    /// A checkbox, unchecked unless `default` says otherwise.
    Checkbox {
        #[serde(default)]
        default: bool,
    },
    /// One of a list of options.
    Select {
        options: Vec<SelectOption>,
        /// The `value` of the option selected at first.
        #[serde(default)]
        default: Option<String>,
    },
    /// A path to a file or a folder, which a Front end may offer to pick.
    /// The value submitted reaches the Extension as an absolute path: the
    /// Daemon expands a leading `~` and puts the home directory before a
    /// relative one.
    File {
        /// The files it takes, by extension (`conf`, without the dot) or by
        /// MIME type (`application/pdf`); empty takes any file.
        #[serde(default)]
        accept: Vec<String>,
        /// Asks for a folder rather than a file; `accept` is then ignored.
        #[serde(default)]
        folder: bool,
        /// The path filled in at first.
        #[serde(default)]
        default: Option<String>,
        #[serde(default)]
        placeholder: Option<String>,
    },
    /// Several lines of text: a note, a commit message, a prompt.
    TextArea {
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default)]
        default: Option<String>,
    },
    /// A number, which the Extension gets as an integer when it is whole and
    /// as a float otherwise. A Front end may step it by `step`, or by 1
    /// without one.
    Number {
        /// The least it may be.
        #[serde(default)]
        min: Option<f64>,
        /// The most it may be.
        #[serde(default)]
        max: Option<f64>,
        /// The value must be `min`, or 0 without a `min`, plus a whole number
        /// of steps; any number goes without one.
        #[serde(default)]
        step: Option<f64>,
        #[serde(default)]
        default: Option<f64>,
    },
    /// A date, or with `time` a date and a time of day, submitted as ISO 8601
    /// in local time with no offset: `2026-10-30`, or `2026-10-30T09:30`
    /// (seconds may follow, `09:30:15`).
    Date {
        #[serde(default)]
        time: bool,
        /// The date filled in at first, written as it is submitted.
        #[serde(default)]
        default: Option<String>,
    },
    /// Any number of a list of options, submitted as the list of the chosen
    /// options' `value`s, in the order of `options`.
    MultiSelect {
        options: Vec<SelectOption>,
        /// The `value`s of the options chosen at first.
        #[serde(default)]
        default: Vec<String>,
    },
    /// Text between the fields, to read; it has no value, and the submitted
    /// values leave it out.
    Description { text: String },
}

/// One option of a [`FormInput::Select`] or a [`FormInput::MultiSelect`]:
/// the `value` submitted and the `label` shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// One row of a detail View's table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub label: String,
    pub value: String,
}

/// A long-running task's state. The Extension sends the whole state on every
/// change; the Daemon keeps the latest, so the job outlives the menu window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Unique among the Extension's jobs. Reusing a finished job's id starts a
    /// new job.
    pub id: String,
    pub title: String,
    pub state: JobState,
    /// How far the job has got; `None` when the Extension can't tell.
    #[serde(default)]
    pub progress: Option<Fraction>,
    /// What the job is doing at the moment, or why it failed.
    #[serde(default)]
    pub message: Option<String>,
}

/// Where a [`Job`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
}
