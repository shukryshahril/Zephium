//! The lab's stand-in for the browser's stores: a little history, a few
//! bookmarks and closed tabs, kept in memory, answering as the shell does and raising the
//! changes it would.

use std::cell::{Cell, RefCell};

use zephium_core::bookmarks::{
    BookmarkChange, BookmarkEntry, BookmarkFailure, BookmarkReply, BookmarkRequest,
};
use zephium_core::extension_data::{
    ClosedTab, ExtensionDataCall, ExtensionDataChange, ExtensionDataInterest, ExtensionDataReply,
    HistoryEntry, HistoryReply, HistoryRequest, TopSite, VisitEntry,
};

pub struct LabData {
    pub interest: Cell<ExtensionDataInterest>,
    history: RefCell<Vec<HistoryEntry>>,
    /// In sibling order; places are counted when answering.
    bookmarks: RefCell<Vec<BookmarkEntry>>,
    /// Newest first.
    closed: RefCell<Vec<ClosedTab>>,
    next: Cell<i64>,
}

impl Default for LabData {
    fn default() -> Self {
        let entry = |id: i64, url: &str, title: &str, visits: u32| HistoryEntry {
            id,
            url: url.into(),
            title: title.into(),
            last_visit: now() - id,
            visits,
        };
        Self {
            interest: Cell::default(),
            history: RefCell::new(vec![
                entry(1, "https://docs.example/guide", "Guide", 5),
                entry(2, "https://news.example/", "News", 2),
                entry(3, "https://shop.example/cart", "Cart", 1),
            ]),
            bookmarks: RefCell::new(vec![
                mark(1, None, "Reading", None),
                mark(2, Some(1), "Guide", Some("https://docs.example/guide")),
                mark(3, None, "News", Some("https://news.example/")),
            ]),
            closed: RefCell::new(
                [
                    ("s2", "https://closed.example/two", "Two"),
                    ("s1", "https://closed.example/one", "One"),
                ]
                .into_iter()
                .map(|(session, url, title)| ClosedTab {
                    session: session.into(),
                    url: url.into(),
                    title: title.into(),
                    closed_at_ms: Some(1_700_000_000_000),
                })
                .collect(),
            ),
            next: Cell::new(100),
        }
    }
}

fn mark(id: i64, parent: Option<i64>, title: &str, url: Option<&str>) -> BookmarkEntry {
    BookmarkEntry {
        id,
        parent,
        index: 0,
        url: url.map(str::to_owned),
        title: title.into(),
        added_at: 1_700_000_000 + id,
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

impl LabData {
    /// The answer, and the change the shell would report once it is made.
    pub fn answer(
        &self,
        call: ExtensionDataCall,
    ) -> (ExtensionDataReply, Option<ExtensionDataChange>) {
        println!("DATA {call:?}");
        match call {
            ExtensionDataCall::History(request) => self.history(request),
            ExtensionDataCall::RecentlyClosed { limit } => {
                let closed = self
                    .closed
                    .borrow()
                    .iter()
                    .take(limit as usize)
                    .cloned()
                    .collect();
                (ExtensionDataReply::Closed(closed), None)
            }
            ExtensionDataCall::Restore { session } => {
                let mut closed = self.closed.borrow_mut();
                match closed
                    .iter()
                    .position(|tab| session.as_ref().is_none_or(|id| &tab.session == id))
                {
                    Some(at) => (
                        ExtensionDataReply::Restored(closed.remove(at)),
                        Some(ExtensionDataChange::Sessions),
                    ),
                    None => (ExtensionDataReply::Unavailable, None),
                }
            }
            ExtensionDataCall::Bookmarks(request) => {
                let (reply, change) = self.bookmarks(request);
                (
                    ExtensionDataReply::Bookmarks(reply),
                    change.map(ExtensionDataChange::Bookmark),
                )
            }
        }
    }

    fn history(
        &self,
        request: HistoryRequest,
    ) -> (ExtensionDataReply, Option<ExtensionDataChange>) {
        let mut history = self.history.borrow_mut();
        let removed = |urls: Vec<String>, all: bool| {
            let change = (all || !urls.is_empty()).then(|| ExtensionDataChange::VisitsRemoved {
                urls: urls.clone(),
                all,
            });
            (
                ExtensionDataReply::History(HistoryReply::Removed {
                    visits: urls.len() as u32,
                    urls,
                    all,
                }),
                change,
            )
        };
        match request {
            HistoryRequest::Search {
                text,
                start,
                end,
                limit,
            } => {
                let found = history
                    .iter()
                    .filter(|entry| {
                        entry.last_visit >= start
                            && entry.last_visit < end
                            && text
                                .split_whitespace()
                                .all(|word| entry.url.contains(word) || entry.title.contains(word))
                    })
                    .take(limit as usize)
                    .cloned()
                    .collect();
                (
                    ExtensionDataReply::History(HistoryReply::Entries(found)),
                    None,
                )
            }
            HistoryRequest::Visits { url } => {
                let visits = history
                    .iter()
                    .find(|entry| entry.url == url)
                    .map(|entry| {
                        (0..entry.visits as i64)
                            .map(|n| VisitEntry {
                                id: entry.id * 10 + n,
                                entry: entry.id,
                                visited_at: entry.last_visit - n * 60,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    ExtensionDataReply::History(HistoryReply::Visits(visits)),
                    None,
                )
            }
            HistoryRequest::Address { .. } => (ExtensionDataReply::Unavailable, None),
            HistoryRequest::MostVisited { limit } => {
                let mut sites: Vec<_> = history.iter().collect();
                sites.sort_by_key(|entry| std::cmp::Reverse(entry.visits));
                let sites = sites
                    .into_iter()
                    .take(limit as usize)
                    .map(|entry| TopSite {
                        url: entry.url.clone(),
                        title: entry.title.clone(),
                    })
                    .collect();
                (
                    ExtensionDataReply::History(HistoryReply::Sites(sites)),
                    None,
                )
            }
            HistoryRequest::Add { url } => {
                let id = self.next.replace(self.next.get() + 1);
                let entry = HistoryEntry {
                    id,
                    url,
                    title: String::new(),
                    last_visit: now(),
                    visits: 1,
                };
                history.insert(0, entry.clone());
                (
                    ExtensionDataReply::History(HistoryReply::Entries(Vec::new())),
                    Some(ExtensionDataChange::Visited(entry)),
                )
            }
            HistoryRequest::RemoveUrl { url } => {
                let before = history.len();
                history.retain(|entry| entry.url != url);
                let urls = if history.len() < before {
                    vec![url]
                } else {
                    Vec::new()
                };
                removed(urls, false)
            }
            HistoryRequest::RemoveRange { start, end } => {
                let mut urls = Vec::new();
                history.retain(|entry| {
                    let inside = entry.last_visit >= start && entry.last_visit < end;
                    if inside {
                        urls.push(entry.url.clone());
                    }
                    !inside
                });
                removed(urls, false)
            }
            HistoryRequest::RemoveAll => {
                history.clear();
                removed(Vec::new(), true)
            }
        }
    }

    fn numbered(&self) -> Vec<BookmarkEntry> {
        let mut next = std::collections::HashMap::new();
        self.bookmarks
            .borrow()
            .iter()
            .map(|entry| {
                let index = next.entry(entry.parent).or_insert(0u32);
                let entry = BookmarkEntry {
                    index: *index,
                    ..entry.clone()
                };
                *index += 1;
                entry
            })
            .collect()
    }

    fn entry(&self, id: i64) -> Option<BookmarkEntry> {
        self.numbered().into_iter().find(|entry| entry.id == id)
    }

    fn below(&self, root: Option<i64>) -> Vec<BookmarkEntry> {
        let all = self.numbered();
        let mut found: Vec<BookmarkEntry> = Vec::new();
        let mut frontier = vec![root];
        while let Some(parent) = frontier.pop() {
            for entry in all.iter().filter(|entry| entry.parent == parent) {
                frontier.push(Some(entry.id));
                found.push(entry.clone());
            }
        }
        found
    }

    /// Puts `entry` at `index` among its siblings, or last.
    fn place(&self, entry: BookmarkEntry, index: Option<u32>) {
        let mut marks = self.bookmarks.borrow_mut();
        let siblings: Vec<usize> = marks
            .iter()
            .enumerate()
            .filter(|(_, other)| other.parent == entry.parent)
            .map(|(at, _)| at)
            .collect();
        let at = index
            .and_then(|index| siblings.get(index as usize).copied())
            .unwrap_or(marks.len());
        marks.insert(at, entry);
    }

    fn bookmarks(&self, request: BookmarkRequest) -> (BookmarkReply, Option<BookmarkChange>) {
        let missing = (BookmarkReply::Failed(BookmarkFailure::Missing), None);
        match request {
            BookmarkRequest::Subtree { root } => {
                let mut entries: Vec<_> = root.and_then(|id| self.entry(id)).into_iter().collect();
                entries.extend(self.below(root));
                (BookmarkReply::Entries(entries), None)
            }
            BookmarkRequest::ChildEntries { parent } => {
                let children = self
                    .numbered()
                    .into_iter()
                    .filter(|entry| entry.parent == parent)
                    .collect();
                (BookmarkReply::Entries(children), None)
            }
            BookmarkRequest::Entries { ids } => {
                match ids.iter().map(|id| self.entry(*id)).collect() {
                    Some(entries) => (BookmarkReply::Entries(entries), None),
                    None => missing,
                }
            }
            BookmarkRequest::Recent { limit } => {
                let mut links: Vec<_> = self
                    .numbered()
                    .into_iter()
                    .filter(|entry| entry.url.is_some())
                    .collect();
                links.sort_by_key(|entry| std::cmp::Reverse(entry.added_at));
                links.truncate(limit as usize);
                (BookmarkReply::Entries(links), None)
            }
            BookmarkRequest::Find { query, url, title } => {
                let found = self
                    .numbered()
                    .into_iter()
                    .filter(|entry| {
                        query.as_deref().is_none_or(|query| {
                            entry.url.is_some()
                                && query.split_whitespace().all(|word| {
                                    entry.title.contains(word)
                                        || entry
                                            .url
                                            .as_deref()
                                            .is_some_and(|url| url.contains(word))
                                })
                        }) && url
                            .as_ref()
                            .is_none_or(|url| entry.url.as_ref() == Some(url))
                            && title.as_ref().is_none_or(|title| &entry.title == title)
                    })
                    .collect();
                (BookmarkReply::Entries(found), None)
            }
            BookmarkRequest::Create {
                parent,
                index,
                title,
                url,
            } => {
                if parent.is_some_and(|parent| {
                    self.entry(parent).is_none_or(|entry| entry.url.is_some())
                }) {
                    return missing;
                }
                let id = self.next.replace(self.next.get() + 1);
                let entry = BookmarkEntry {
                    id,
                    parent,
                    index: 0,
                    url,
                    title,
                    added_at: now(),
                };
                self.place(entry, index);
                let made = self.entry(id).expect("just placed");
                (
                    BookmarkReply::Entries(vec![made.clone()]),
                    Some(BookmarkChange::Created(made)),
                )
            }
            BookmarkRequest::Update { id, title, url } => {
                let mut marks = self.bookmarks.borrow_mut();
                let Some(entry) = marks.iter_mut().find(|entry| entry.id == id) else {
                    return missing;
                };
                if url.is_some() && entry.url.is_none() {
                    return (BookmarkReply::Failed(BookmarkFailure::Invalid), None);
                }
                if let Some(title) = title {
                    entry.title = title;
                }
                if url.is_some() {
                    entry.url = url;
                }
                let (title, url) = (entry.title.clone(), entry.url.clone());
                drop(marks);
                (
                    BookmarkReply::Entries(self.entry(id).into_iter().collect()),
                    Some(BookmarkChange::Changed { id, title, url }),
                )
            }
            BookmarkRequest::Relocate { id, parent, index } => {
                let Some(before) = self.entry(id) else {
                    return missing;
                };
                let parent = parent.unwrap_or(before.parent);
                let index = match index {
                    Some(index) if parent == before.parent && index > before.index => {
                        Some(index - 1)
                    }
                    index => index,
                };
                self.bookmarks.borrow_mut().retain(|entry| entry.id != id);
                self.place(
                    BookmarkEntry {
                        parent,
                        ..before.clone()
                    },
                    index,
                );
                let now = self.entry(id).expect("just placed");
                let change = BookmarkChange::Moved {
                    id,
                    parent: now.parent,
                    index: now.index,
                    old_parent: before.parent,
                    old_index: before.index,
                };
                (BookmarkReply::Entries(vec![now]), Some(change))
            }
            BookmarkRequest::Delete { id, recursive } => {
                let Some(before) = self.entry(id) else {
                    return missing;
                };
                let below: Vec<i64> = self.below(Some(id)).iter().map(|entry| entry.id).collect();
                if !below.is_empty() && !recursive {
                    return (BookmarkReply::Failed(BookmarkFailure::Invalid), None);
                }
                self.bookmarks
                    .borrow_mut()
                    .retain(|entry| entry.id != id && !below.contains(&entry.id));
                (BookmarkReply::Done, Some(BookmarkChange::Removed(before)))
            }
            _ => (BookmarkReply::Failed(BookmarkFailure::Invalid), None),
        }
    }

    /// Whether the shell would report `change`: only what someone hears.
    pub fn heard(&self, change: &ExtensionDataChange) -> bool {
        let interest = self.interest.get();
        match change {
            ExtensionDataChange::Visited(_) | ExtensionDataChange::VisitsRemoved { .. } => {
                interest.history
            }
            ExtensionDataChange::Bookmark(_) => interest.bookmarks,
            ExtensionDataChange::Sessions => interest.sessions,
        }
    }
}
