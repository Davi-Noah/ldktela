//! Keyset pagination envelope (`docs/api/rest-api.md` §4).
//!
//! There is no `total`: an exact count over 100k messages is a scan per page and
//! nobody uses the number.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub has_more: bool,
}

impl<T> Page<T> {
    pub fn new(data: Vec<T>, has_more: bool) -> Self {
        Self { data, has_more }
    }
}

/// The cursor trio. `before`, `after` and `around` are mutually exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageQuery {
    pub before: Option<Uuid>,
    pub after: Option<Uuid>,
    pub around: Option<Uuid>,
    pub limit: Option<u32>,
}

/// Which direction a validated cursor walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// Newest first, strictly older than the given id.
    Before(Uuid),
    /// Oldest first, strictly newer than the given id.
    After(Uuid),
    /// `limit / 2` on each side of the given id.
    Around(Uuid),
    /// Newest first, from the end of the channel.
    Latest,
}

impl PageQuery {
    /// Rejects more than one cursor. The caller turns `None` into a validation
    /// error; this crate holds no logic beyond the shape check.
    pub fn cursor(&self) -> Option<Cursor> {
        match (self.before, self.after, self.around) {
            (None, None, None) => Some(Cursor::Latest),
            (Some(id), None, None) => Some(Cursor::Before(id)),
            (None, Some(id), None) => Some(Cursor::After(id)),
            (None, None, Some(id)) => Some(Cursor::Around(id)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(before: bool, after: bool, around: bool) -> PageQuery {
        let id = Uuid::now_v7();
        PageQuery {
            before: before.then_some(id),
            after: after.then_some(id),
            around: around.then_some(id),
            limit: None,
        }
    }

    #[test]
    fn cursors_are_mutually_exclusive() {
        assert_eq!(query(false, false, false).cursor(), Some(Cursor::Latest));
        assert!(matches!(
            query(true, false, false).cursor(),
            Some(Cursor::Before(_))
        ));
        assert!(matches!(
            query(false, true, false).cursor(),
            Some(Cursor::After(_))
        ));
        assert!(matches!(
            query(false, false, true).cursor(),
            Some(Cursor::Around(_))
        ));
        assert_eq!(query(true, true, false).cursor(), None);
        assert_eq!(query(true, false, true).cursor(), None);
        assert_eq!(query(true, true, true).cursor(), None);
    }

    #[test]
    fn page_carries_no_total() {
        let page = Page::new(vec![1, 2, 3], true);
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(
            json.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["data", "has_more"]
        );
    }
}
