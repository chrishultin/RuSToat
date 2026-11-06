//! Message search: what the header's search box may send to Stoat.

use crate::api::models::MessageSort;

/// Longest query Stoat accepts (`DataMessageSearch.query` is 1 to 64
/// characters).
pub const MAX_QUERY_CHARS: usize = 64;

/// How many results a page of a Latest or Oldest search asks for (the most
/// Stoat sends by default). A full page may have more after it.
pub const RESULT_LIMIT: i64 = 50;

/// How many results a Relevance search asks for (the most Stoat allows).
/// Relevance has no next page: its order is by text score, which a message
/// ID cursor can't continue, and Stoat has no offset.
pub const RELEVANCE_LIMIT: i64 = 100;

/// How many pinned messages are asked for (the most Stoat allows). A channel
/// with more shows its newest.
pub const PINS_LIMIT: i64 = 100;

/// What a search box's text amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryCheck {
    /// Nothing but whitespace: there's nothing to search for.
    Empty,
    /// More than [`MAX_QUERY_CHARS`] characters (this many): Stoat would
    /// refuse it, so it isn't sent.
    TooLong { chars: usize },
    /// Fine to send, without the whitespace around it.
    Ok(String),
}

/// Decide what to do with `text` typed in the search box.
pub fn check_query(text: &str) -> QueryCheck {
    let query = text.trim();
    let chars = query.chars().count();
    if chars == 0 {
        QueryCheck::Empty
    } else if chars > MAX_QUERY_CHARS {
        QueryCheck::TooLong { chars }
    } else {
        QueryCheck::Ok(query.to_string())
    }
}

/// Where the next page of a search starts. Stoat's `before` and `after` are
/// exclusive message ID filters, applied before sorting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchCursor {
    /// Matches older than this message (a Latest search).
    Before(String),
    /// Matches newer than this message (an Oldest search).
    After(String),
}

/// How many results one request for a `sort` search asks for.
pub fn page_limit(sort: &MessageSort) -> i64 {
    match sort {
        MessageSort::Relevance => RELEVANCE_LIMIT,
        MessageSort::Latest | MessageSort::Oldest => RESULT_LIMIT,
    }
}

/// Where the page after one with these message IDs (in the order Stoat sent
/// them) starts, if there may be one: only a full page of a Latest or Oldest
/// search may have more after it.
pub fn next_cursor(sort: &MessageSort, ids: &[&str]) -> Option<SearchCursor> {
    if (ids.len() as i64) < page_limit(sort) {
        return None;
    }
    let last = ids.last()?.to_string();
    match sort {
        MessageSort::Latest => Some(SearchCursor::Before(last)),
        MessageSort::Oldest => Some(SearchCursor::After(last)),
        MessageSort::Relevance => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_text_is_not_a_query() {
        assert_eq!(check_query(""), QueryCheck::Empty);
        assert_eq!(check_query("  \n\t "), QueryCheck::Empty);
    }

    #[test]
    fn a_query_is_trimmed() {
        assert_eq!(
            check_query("  hello world "),
            QueryCheck::Ok("hello world".into())
        );
    }

    #[test]
    fn sixty_four_characters_is_the_most() {
        let longest = "a".repeat(64);
        assert_eq!(check_query(&longest), QueryCheck::Ok(longest.clone()));
        assert_eq!(
            check_query(&format!("{longest}a")),
            QueryCheck::TooLong { chars: 65 }
        );
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        // 64 two-byte characters are 128 bytes, and fine.
        let accented = "é".repeat(64);
        assert_eq!(check_query(&accented), QueryCheck::Ok(accented.clone()));
        assert_eq!(
            check_query(&"é".repeat(65)),
            QueryCheck::TooLong { chars: 65 }
        );
    }

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("m{i:03}")).collect()
    }

    #[test]
    fn page_limits() {
        assert_eq!(page_limit(&MessageSort::Latest), 50);
        assert_eq!(page_limit(&MessageSort::Oldest), 50);
        assert_eq!(page_limit(&MessageSort::Relevance), 100);
    }

    #[test]
    fn a_full_page_continues_from_its_last_message() {
        let page = ids(50);
        let page: Vec<&str> = page.iter().map(String::as_str).collect();
        assert_eq!(
            next_cursor(&MessageSort::Latest, &page),
            Some(SearchCursor::Before("m049".into()))
        );
        assert_eq!(
            next_cursor(&MessageSort::Oldest, &page),
            Some(SearchCursor::After("m049".into()))
        );
    }

    #[test]
    fn relevance_never_continues() {
        let page = ids(100);
        let page: Vec<&str> = page.iter().map(String::as_str).collect();
        assert_eq!(next_cursor(&MessageSort::Relevance, &page), None);
    }

    #[test]
    fn a_short_page_is_the_last() {
        let page = ids(49);
        let page: Vec<&str> = page.iter().map(String::as_str).collect();
        assert_eq!(next_cursor(&MessageSort::Latest, &page), None);
        assert_eq!(next_cursor(&MessageSort::Oldest, &page), None);
        assert_eq!(next_cursor(&MessageSort::Latest, &[]), None);
    }
}
