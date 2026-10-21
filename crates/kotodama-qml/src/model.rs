//! The search as Qt's list model: the roles a delegate reads, the overrides
//! that answer a view, and the signals Qt requires around each change to the
//! rows.
//!
//! # Owns
//!
//! - The roles, by number and by the name QML reads them as.
//! - `data`, `roleNames` and `rowCount`, and `QtModel`, the [`Model`] over Qt's begin/end signals.
//!
//! # Not here
//!
//! - The rows, and how a new list replaces the old: `rows`.
//! - Which list is shown, the Results or a list View's Items: `updates`.

use std::ops::Range;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QByteArray;
use cxx_qt_lib::QHash;
use cxx_qt_lib::QHashPair_i32_QByteArray;
use cxx_qt_lib::QModelIndex;
use cxx_qt_lib::QString;
use cxx_qt_lib::QVariant;

use crate::bridge::qobject::KotodamaSearch;
use crate::bridge::qobject::QVector_i32;
use crate::rows;
use crate::rows::Model;
use crate::rows::Row;
use crate::rows::Rows;

// Roles start at `Qt::UserRole`.
const TITLE: i32 = 0x100;
const SUBTITLE: i32 = 0x101;
const ICON: i32 = 0x102;
/// The title as `StyledText`, its matched characters in `highlightColor`.
pub const MARKUP: i32 = 0x103;
const EXTENSION: i32 = 0x104;
/// The Alias the query named the Result by, or empty.
const ALIAS: i32 = 0x105;
/// Why the Result can't run, or empty when it can.
const UNAVAILABLE: i32 = 0x106;

impl KotodamaSearch {
    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = usize::try_from(index.row()).ok().and_then(|row| self.rows.get(row)) else {
            return QVariant::default();
        };
        let text = match role {
            TITLE => QString::from(&row.title),
            SUBTITLE => QString::from(&row.subtitle),
            ICON => QString::from(&row.icon),
            MARKUP => QString::from(&row.markup(&String::from(&self.highlight_color))),
            EXTENSION => QString::from(&row.result.extension),
            ALIAS => QString::from(row.alias.as_deref().unwrap_or_default()),
            UNAVAILABLE => QString::from(row.unavailable.as_deref().unwrap_or_default()),
            _ => return QVariant::default(),
        };
        QVariant::from(&text)
    }

    #[expect(
        clippy::unused_self,
        reason = "it overrides `QAbstractListModel::roleNames`, a const member function"
    )]
    pub(crate) fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in [
            (TITLE, "title"),
            (SUBTITLE, "subtitle"),
            (ICON, "icon"),
            (MARKUP, "markup"),
            (EXTENSION, "extension"),
            (ALIAS, "alias"),
            (UNAVAILABLE, "unavailable"),
        ] {
            roles.insert(role, QByteArray::from(name));
        }
        roles
    }

    pub(crate) fn row_count(&self, parent: &QModelIndex) -> i32 {
        // A list has rows only at the top.
        if parent.is_valid() { 0 } else { self.rows.len() as i32 }
    }

    /// Puts the Results in place of the current rows, by position: see
    /// [`rows::replace`].
    pub(crate) fn replace(self: Pin<&mut Self>, rows: Vec<Row>) {
        rows::replace(&mut QtModel(self), rows);
    }

    /// Tells views the rows in `range` changed, in `roles` or, if empty, in all.
    pub(crate) fn rows_changed(mut self: Pin<&mut Self>, range: Range<usize>, roles: &[i32]) {
        if range.is_empty() {
            return;
        }
        let root = QModelIndex::default();
        let first = self.index(range.start as i32, 0, &root);
        let last = self.index(range.end as i32 - 1, 0, &root);
        let mut changed = QVector_i32::default();
        for &role in roles {
            changed.append(role);
        }
        self.as_mut().data_changed(&first, &last, &changed);
    }
}

/// The model's rows, changed between the signals Qt requires around each
/// change.
pub struct QtModel<'a>(pub Pin<&'a mut KotodamaSearch>);

impl Model for QtModel<'_> {
    fn rows(&mut self) -> &mut Rows {
        &mut self.0.as_mut().rust_mut().get_mut().rows
    }

    fn begin_remove(&mut self, rows: Range<usize>) {
        // SAFETY: `rows::diff` and `rows::replace` remove the rows before
        // `end_remove`, as Qt requires.
        unsafe {
            self.0
                .as_mut()
                .begin_remove_rows(&QModelIndex::default(), rows.start as i32, rows.end as i32 - 1);
        }
    }

    fn end_remove(&mut self) {
        // SAFETY: after a `begin_remove`.
        unsafe { self.0.as_mut().end_remove_rows() }
    }

    fn begin_insert(&mut self, rows: Range<usize>) {
        // SAFETY: the rows are inserted before `end_insert`, as Qt requires.
        unsafe {
            self.0
                .as_mut()
                .begin_insert_rows(&QModelIndex::default(), rows.start as i32, rows.end as i32 - 1);
        }
    }

    fn end_insert(&mut self) {
        // SAFETY: after a `begin_insert`.
        unsafe { self.0.as_mut().end_insert_rows() }
    }

    fn begin_move(&mut self, from: usize, before: usize) {
        let root = QModelIndex::default();
        // SAFETY: the row moves before `end_move`, as Qt requires; `before`
        // is never `from` or `from + 1`, the moves Qt refuses.
        let moving = unsafe {
            self.0
                .as_mut()
                .begin_move_rows(&root, from as i32, from as i32, &root, before as i32)
        };
        debug_assert!(moving, "Qt refused to move row {from} before {before}");
    }

    fn end_move(&mut self) {
        // SAFETY: after a `begin_move`.
        unsafe { self.0.as_mut().end_move_rows() }
    }

    fn changed(&mut self, rows: Range<usize>) {
        self.0.as_mut().rows_changed(rows, &[]);
    }
}
