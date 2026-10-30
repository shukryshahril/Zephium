//! The lab's stand-in for the browser's stores: a little history, kept in
//! memory, answering as the shell does and raising the changes it would.

use std::cell::{Cell, RefCell};

use zephium_core::extension_data::{
    ExtensionDataCall, ExtensionDataChange, ExtensionDataInterest, ExtensionDataReply,
    HistoryEntry, HistoryReply, HistoryRequest, VisitEntry,
};

pub struct LabData {
    pub interest: Cell<ExtensionDataInterest>,
    history: RefCell<Vec<HistoryEntry>>,
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
            next: Cell::new(100),
        }
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

    /// Whether the shell would report `change`: only what someone hears.
    pub fn heard(&self, change: &ExtensionDataChange) -> bool {
        let interest = self.interest.get();
        match change {
            ExtensionDataChange::Visited(_) | ExtensionDataChange::VisitsRemoved { .. } => {
                interest.history
            }
        }
    }
}
