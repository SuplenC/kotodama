//! Items' payload: everything a Result shows of an Item but a search never
//! reads, kept encoded (ADR 0017). A Shelf holds the payloads of one
//! Extension's Items, in a file the kernel pages in when a row is shown, or
//! in memory when there is no file to use.
//!
//! A Shelf is only appended to: the versions of a Segment built one after
//! another share it, and each reads only what was there when it was built.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use kotodama_protocol::{Item, WireFormat};

/// Where one Item's payload lies on its Shelf.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub at: u64,
    pub len: u32,
}

impl Place {
    pub fn end(self) -> u64 {
        self.at + u64::from(self.len)
    }
}

pub struct Shelf {
    /// Unique to the Shelf, so a payload read from it can be told apart from
    /// one at the same place on another.
    id: u64,
    store: Store,
}

enum Store {
    /// Unlinked as soon as it is made: it goes when the last Shelf does.
    File(File),
    Memory(RwLock<Vec<u8>>),
}

static SHELVES: AtomicU64 = AtomicU64::new(0);

impl Shelf {
    /// A Shelf in a file in `dir`, or in memory without one or when the file
    /// can't be made.
    pub fn new(dir: Option<&Path>) -> Arc<Shelf> {
        let id = SHELVES.fetch_add(1, Ordering::Relaxed);
        let store = match dir.map(|dir| file_in(dir, id)) {
            Some(Ok(file)) => Store::File(file),
            Some(Err(err)) => {
                log::warn!("keeping Items in memory: can't make a file for them: {err}");
                Store::Memory(RwLock::default())
            }
            None => Store::Memory(RwLock::default()),
        };
        Arc::new(Shelf { id, store })
    }

    pub fn in_memory() -> Arc<Shelf> {
        Shelf::new(None)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Writes `bytes` at `at`.
    pub fn put(&self, at: u64, bytes: &[u8]) -> io::Result<()> {
        match &self.store {
            Store::File(file) => std::os::unix::fs::FileExt::write_all_at(file, bytes, at),
            Store::Memory(memory) => {
                let mut memory = memory.write().unwrap_or_else(|err| err.into_inner());
                let at = at as usize;
                if memory.len() < at + bytes.len() {
                    memory.resize(at + bytes.len(), 0);
                }
                memory[at..at + bytes.len()].copy_from_slice(bytes);
                Ok(())
            }
        }
    }

    /// Reads the bytes at `place` into `buf`.
    pub fn read(&self, place: Place, buf: &mut Vec<u8>) -> io::Result<()> {
        self.read_up_to(place.at, place.end(), buf)
    }

    /// Reads the bytes from `start` up to `end` into `buf`: a whole Shelf
    /// may be longer than one payload can.
    pub fn read_up_to(&self, start: u64, end: u64, buf: &mut Vec<u8>) -> io::Result<()> {
        let len = usize::try_from(end - start).map_err(|_| io::ErrorKind::OutOfMemory)?;
        buf.clear();
        buf.resize(len, 0);
        match &self.store {
            Store::File(file) => std::os::unix::fs::FileExt::read_exact_at(file, buf, start),
            Store::Memory(memory) => {
                let memory = memory.read().unwrap_or_else(|err| err.into_inner());
                let at = start as usize;
                let bytes = memory
                    .get(at..at + len)
                    .ok_or(io::ErrorKind::UnexpectedEof)?;
                buf.copy_from_slice(bytes);
                Ok(())
            }
        }
    }

    /// The Item at `place`.
    pub fn get(&self, place: Place) -> Option<Item> {
        self.get_with(place, &mut Vec::new())
    }

    /// The Item at `place`, read through `buf`, which a caller reading one
    /// Item after another keeps rather than allocate one per Item.
    pub fn get_with(&self, place: Place, buf: &mut Vec<u8>) -> Option<Item> {
        let item = self
            .read(place, buf)
            .map_err(|err| err.to_string())
            .and_then(|()| decode(buf).map_err(|err| err.to_string()));
        match item {
            Ok(item) => Some(item),
            Err(err) => {
                log::error!("can't read an Item back: {err}");
                None
            }
        }
    }
}

pub fn encode(item: &Item, buf: &mut Vec<u8>) {
    buf.clear();
    // Encoding into memory fails only on a type serde can't encode, which
    // an Item never is.
    if let Err(err) = WireFormat::Postcard.encode_into(item, buf) {
        log::error!("can't encode an Item: {err}");
    }
}

fn decode(bytes: &[u8]) -> Result<Item, kotodama_protocol::Error> {
    WireFormat::Postcard.decode(bytes)
}

/// A new file in `dir`, already unlinked. Only its user may read it: it
/// holds what the Extensions sent, clipboard entries among them.
fn file_in(dir: &Path, id: u64) -> io::Result<File> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let path = dir.join(format!("items-{}-{id}", std::process::id()));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    fs::remove_file(&path)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kotodama_protocol::{Action, ItemKind};

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            kind: ItemKind::Url {
                url: format!("https://example.org/{id}"),
            },
            identity: Some(format!("url:{id}")),
            title: format!("Title of {id}"),
            subtitle: Some("A subtitle".into()),
            icon: Some("web-browser".into()),
            keyword: None,
            keywords: vec!["more".into()],
            hidden: false,
            actions: vec![Action {
                id: "open".into(),
                title: "Open".into(),
                shortcut: None,
                arguments: vec![],
            }],
        }
    }

    fn shelve(shelf: &Shelf, items: &[Item]) -> Vec<Place> {
        let mut buf = Vec::new();
        let mut at = 0;
        items
            .iter()
            .map(|item| {
                encode(item, &mut buf);
                shelf.put(at, &buf).unwrap();
                let place = Place {
                    at,
                    len: buf.len() as u32,
                };
                at = place.end();
                place
            })
            .collect()
    }

    #[test]
    fn an_item_comes_back_off_a_shelf_in_memory_as_it_went_on() {
        let shelf = Shelf::in_memory();
        let items = [item("a"), item("b")];
        let places = shelve(&shelf, &items);
        assert_eq!(shelf.get(places[1]), Some(items[1].clone()));
        assert_eq!(shelf.get(places[0]), Some(items[0].clone()));
    }

    #[test]
    fn an_item_comes_back_off_a_shelf_in_a_file_that_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = Shelf::new(Some(dir.path()));
        let items = [item("a"), item("b")];
        let places = shelve(&shelf, &items);
        assert_eq!(shelf.get(places[1]), Some(items[1].clone()));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
