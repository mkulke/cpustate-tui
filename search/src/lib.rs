//! Search functionality for CPU debug tool
//!
//! This crate is `no_std` compatible but can use `std` for testing.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Search state for navigating through matches
#[derive(Default)]
pub struct SearchState {
    pub matches: Vec<u16>,
    pub current_match: usize,
    pub last_query: String,
}

impl SearchState {
    /// Move to next match, wrapping around. Returns new offset if matches exist.
    pub fn next(&mut self) -> Option<u16> {
        if self.matches.is_empty() {
            return None;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
        Some(self.matches[self.current_match])
    }

    /// Move to previous match, wrapping around. Returns new offset if matches exist.
    pub fn prev(&mut self) -> Option<u16> {
        if self.matches.is_empty() {
            return None;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len() - 1;
        } else {
            self.current_match -= 1;
        }
        Some(self.matches[self.current_match])
    }

    /// Clear all matches and reset state
    pub fn clear(&mut self) {
        self.matches.clear();
        self.current_match = 0;
        self.last_query.clear();
    }
}

/// Check if query contains any uppercase characters (for smart-case)
pub fn has_uppercase(s: &str) -> bool {
    s.chars().any(|c| c.is_uppercase())
}

/// Smart-case string matching: case-insensitive if query is all lowercase,
/// case-sensitive if query contains any uppercase characters
pub fn smart_contains(haystack: &str, needle: &str) -> bool {
    if has_uppercase(needle) {
        // Case-sensitive search
        haystack.contains(needle)
    } else {
        // Case-insensitive search
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

/// Search through feature names and return matching line offsets
pub fn find_matches(query: &str, features: &[(&str, bool)], start_line: u16) -> Vec<u16> {
    let mut matches = Vec::new();
    let mut line = start_line;

    for (name, _) in features {
        if smart_contains(name, query) {
            matches.push(line);
        }
        line += 1;
    }

    matches
}

/// Search through string names and return matching line offsets
pub fn find_matches_strs(query: &str, names: &[&str], start_line: u16) -> Vec<u16> {
    let mut matches = Vec::new();
    let mut line = start_line;

    for name in names {
        if smart_contains(name, query) {
            matches.push(line);
        }
        line += 1;
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matches_basic() {
        let features = vec![
            ("avx", true),
            ("avx2", true),
            ("sse", true),
            ("sse2", true),
            ("avx512", false),
        ];

        let matches = find_matches("avx", &features, 0);
        assert_eq!(matches, vec![0, 1, 4]); // avx, avx2, avx512
    }

    #[test]
    fn test_find_matches_case_insensitive() {
        let features = vec![("AVX", true), ("avx2", true), ("SSE", true)];

        let matches = find_matches("avx", &features, 0);
        assert_eq!(matches, vec![0, 1]);
    }

    #[test]
    fn test_find_matches_with_offset() {
        let features = vec![("fpu", true), ("sse", true)];

        let matches = find_matches("sse", &features, 10);
        assert_eq!(matches, vec![11]); // starts at line 10, sse is second
    }

    #[test]
    fn test_find_matches_no_match() {
        let features = vec![("avx", true), ("sse", true)];

        let matches = find_matches("xyz", &features, 0);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_search_state_next() {
        let mut state = SearchState {
            matches: vec![5, 10, 15],
            current_match: 0,
            ..Default::default()
        };

        assert_eq!(state.next(), Some(10)); // 0 -> 1
        assert_eq!(state.current_match, 1);
        assert_eq!(state.next(), Some(15)); // 1 -> 2
        assert_eq!(state.current_match, 2);
        assert_eq!(state.next(), Some(5)); // 2 -> 0 (wrap)
        assert_eq!(state.current_match, 0);
    }

    #[test]
    fn test_search_state_prev() {
        let mut state = SearchState {
            matches: vec![5, 10, 15],
            current_match: 0,
            ..Default::default()
        };

        assert_eq!(state.prev(), Some(15)); // 0 -> 2 (wrap)
        assert_eq!(state.current_match, 2);
        assert_eq!(state.prev(), Some(10)); // 2 -> 1
        assert_eq!(state.current_match, 1);
        assert_eq!(state.prev(), Some(5)); // 1 -> 0
        assert_eq!(state.current_match, 0);
    }

    #[test]
    fn test_search_state_empty() {
        let mut state = SearchState::default();

        assert_eq!(state.next(), None);
        assert_eq!(state.prev(), None);
    }

    #[test]
    fn test_search_state_clear() {
        let mut state = SearchState {
            matches: vec![1, 2, 3],
            current_match: 2,
            last_query: String::from("test"),
        };

        state.clear();

        assert!(state.matches.is_empty());
        assert_eq!(state.current_match, 0);
        assert!(state.last_query.is_empty());
    }

    #[test]
    fn test_smart_contains_lowercase_query() {
        // Lowercase query matches both cases
        assert!(smart_contains("IA32_EFER", "efer"));
        assert!(smart_contains("ia32_efer", "efer"));
        assert!(smart_contains("EFER", "ef"));
    }

    #[test]
    fn test_smart_contains_uppercase_query() {
        // Uppercase in query triggers case-sensitive
        assert!(smart_contains("IA32_EFER", "EFER"));
        assert!(!smart_contains("ia32_efer", "EFER")); // No match - case sensitive
        assert!(smart_contains("IA32_EFER", "IA32"));
        assert!(!smart_contains("ia32_efer", "IA32")); // No match
    }

    #[test]
    fn test_smart_contains_mixed_query() {
        // Mixed case query is case-sensitive
        assert!(smart_contains("IA32_TSC_AUX", "TSC_A"));
        assert!(!smart_contains("ia32_tsc_aux", "TSC_A"));
    }

    #[test]
    fn test_has_uppercase() {
        assert!(!has_uppercase("abc"));
        assert!(!has_uppercase("123"));
        assert!(!has_uppercase("abc_123"));
        assert!(has_uppercase("Abc"));
        assert!(has_uppercase("ABC"));
        assert!(has_uppercase("abC"));
    }
}
