//! Views an Extension pushes on top of the search, and the jobs a progress View
//! follows. The Daemon passes them on to the Front end unchanged.

use serde::{Deserialize, Serialize};

use crate::{Fraction, Item};

/// A screen pushed on top of the search. Esc pops it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    /// Unique among the Extension's open Views; updating or popping a View names
    /// it by this id.
    pub id: String,
    pub title: String,
    pub content: ViewContent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewContent {
    /// Items to pick from; their Actions run like a search Result's, and
    /// Contributed actions are offered on them by Item kind (ADR 0002). Typing
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
    Detail {
        markdown: String,
        #[serde(default)]
        metadata: Vec<MetadataEntry>,
    },
    /// Follows the job with this id as the Extension reports its progress.
    Progress { job: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormField {
    /// The key this field's value has in the submitted values.
    pub id: String,
    pub label: String,
    pub input: FormInput,
}

/// A form field's control, and so the type of its submitted value: a string for
/// text and a select, a boolean for a checkbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormInput {
    Text {
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default)]
        default: Option<String>,
        /// Hides what is typed, for passwords and tokens.
        #[serde(default)]
        secret: bool,
    },
    Checkbox {
        #[serde(default)]
        default: bool,
    },
    Select {
        options: Vec<SelectOption>,
        /// The `value` of the option selected at first.
        #[serde(default)]
        default: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

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
    /// What the job is doing now, or why it failed.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
}
