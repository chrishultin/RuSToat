//! Message search: what the header's search box may send to Stoat.

/// Longest query Stoat accepts (`DataMessageSearch.query` is 1 to 64
/// characters).
pub const MAX_QUERY_CHARS: usize = 64;

/// How many results are asked for (the most Stoat sends by default). There's
/// no "load more": a search that matches more than this shows the first page.
pub const RESULT_LIMIT: i64 = 50;

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
}
