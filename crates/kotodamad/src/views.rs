//! The Views Extensions have open, in the order the menus stack them, and
//! filtering a list View's Items as the user types into it. A list View is
//! filtered here, with the search's matcher and Tiers, so typing never waits
//! on the Extension unless it asked to filter the list itself.

use kotodama_protocol::{Item, ListMatch, View, ViewContent};
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};

use crate::daemon::ExtensionId;
use crate::narrowing::Narrowing;
use crate::rank::{self, Haystack, Tiers};

/// The matches past which none has highlights: they cost as much as the
/// matching, and rows that far down are seldom seen.
const HIGHLIGHTED: usize = 256;

/// One open View.
pub struct Open {
    pub extension: ExtensionId,
    pub view: View,
    /// A list View's Items as the matcher reads them, made the first time it
    /// is filtered: most lists never are.
    haystacks: Option<Vec<Haystack>>,
    /// The positions of the Items the last filter matched, so typing on
    /// searches only those.
    narrowing: Narrowing<u32>,
}

impl Open {
    fn new(extension: ExtensionId, view: View) -> Self {
        Open {
            extension,
            view,
            haystacks: None,
            narrowing: Narrowing::default(),
        }
    }

    pub fn is(&self, extension: ExtensionId, id: &str) -> bool {
        self.extension == extension && self.view.id == id
    }

    /// A list View's Items; none for other Views.
    pub fn items(&self) -> &[Item] {
        match &self.view.content {
            ViewContent::List { items, .. } => items,
            _ => &[],
        }
    }

    /// Whether this is a list View the Daemon filters, rather than its
    /// Extension.
    pub fn filtered_here(&self) -> bool {
        matches!(
            self.view.content,
            ViewContent::List {
                filters_itself: false,
                ..
            }
        )
    }

    /// Whether this is a list View its Extension filters itself.
    pub fn filters_itself(&self) -> bool {
        matches!(
            self.view.content,
            ViewContent::List {
                filters_itself: true,
                ..
            }
        )
    }
}

/// Every View Extensions have open, the top one last, as each menu stacks
/// them.
#[derive(Default)]
pub struct Views {
    stack: Vec<Open>,
    /// The positions of the Items a filter matches as it runs, before its
    /// View's [`Narrowing`] keeps them; kept for its capacity.
    scored: Vec<u32>,
}

impl Views {
    /// Bottom first.
    pub fn iter(&self) -> impl Iterator<Item = &Open> {
        self.stack.iter()
    }

    pub fn get(&self, extension: ExtensionId, id: &str) -> Option<&Open> {
        self.stack.iter().find(|open| open.is(extension, id))
    }

    /// Puts a View on top. One already open with the same id moves to the
    /// top in its new form, as it does in the menus.
    pub fn push(&mut self, extension: ExtensionId, view: View) {
        self.pop(extension, &view.id);
        self.stack.push(Open::new(extension, view));
    }

    /// Replaces the open View with the same id where it is. False if there
    /// is none.
    pub fn update(&mut self, extension: ExtensionId, view: View) -> bool {
        match self
            .stack
            .iter_mut()
            .find(|open| open.is(extension, &view.id))
        {
            Some(open) => {
                *open = Open::new(extension, view);
                true
            }
            None => false,
        }
    }

    pub fn pop(&mut self, extension: ExtensionId, id: &str) {
        self.stack.retain(|open| !open.is(extension, id));
    }

    /// Pops every View of an Extension: their ids, bottom first.
    pub fn pop_all(&mut self, extension: ExtensionId) -> Vec<String> {
        let mut popped = Vec::new();
        self.stack.retain_mut(|open| {
            let mine = open.extension == extension;
            if mine {
                popped.push(std::mem::take(&mut open.view.id));
            }
            !mine
        });
        popped
    }

    /// The Item `id` of one of the Extension's list Views.
    pub fn item(&self, extension: ExtensionId, id: &str) -> Option<&Item> {
        self.stack
            .iter()
            .filter(|open| open.extension == extension)
            .flat_map(Open::items)
            .find(|item| item.id == id)
    }

    /// The Items of the list View that match `text`, as the search matches
    /// Items: the best Tier first, and within a Tier in the Extension's
    /// order, which is the list's meaning, such as most recent first. The
    /// first [`HIGHLIGHTED`] have highlights. Empty if no such View is open.
    pub fn filter(
        &mut self,
        extension: ExtensionId,
        id: &str,
        text: &str,
        matcher: &mut Matcher,
        tiers: &mut Tiers,
    ) -> Vec<ListMatch> {
        let Some(open) = self.stack.iter_mut().find(|open| open.is(extension, id)) else {
            return Vec::new();
        };
        let items = match &open.view.content {
            ViewContent::List { items, .. } => items,
            _ => return Vec::new(),
        };
        let haystacks = open
            .haystacks
            .get_or_insert_with(|| items.iter().map(Haystack::new).collect());
        // Folded like the haystacks, so neither case nor accents matter.
        let pattern = Pattern::parse(
            &rank::fold_text(text),
            CaseMatching::Ignore,
            Normalization::Never,
        );
        let query = rank::Query::new(text);
        let filter = query.filter();
        let scored = &mut self.scored;
        scored.clear();
        let mut found: Vec<(u8, rank::Tier, u32)> = Vec::new();
        let mut consider = |n: u32| {
            let (item, haystack) = (&items[n as usize], &haystacks[n as usize]);
            if let Some((_, matched)) = haystack.score(&pattern, matcher, filter) {
                scored.push(n);
                if let Some(tier) = tiers.item(haystack, item, &query, matched) {
                    found.push((tier.coarse(), tier, n));
                }
            }
        };
        match open.narrowing.within(text, &query) {
            Some(within) => within.iter().copied().for_each(&mut consider),
            None => (0..items.len() as u32).for_each(&mut consider),
        }
        open.narrowing.found(text, &query, scored.drain(..));
        // Stable, so a Tier keeps the Extension's order.
        found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        found
            .into_iter()
            .enumerate()
            .map(|(rank, (_, _, n))| {
                let mut highlights = Vec::new();
                let title = haystacks[n as usize].title.slice(..);
                if rank < HIGHLIGHTED && pattern.indices(title, matcher, &mut highlights).is_some()
                {
                    highlights.sort_unstable();
                    highlights.dedup();
                } else {
                    // Matched by a keyword or the subtitle.
                    highlights.clear();
                }
                ListMatch {
                    item: n,
                    highlights,
                }
            })
            .collect()
    }
}
