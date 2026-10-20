use crate::buffer::{RopeBuffer, TextBuffer};

/// Find all non-overlapping occurrences of `needle` in `haystack`.
/// Returns a vector of `(start_byte, end_byte)` tuples.
pub fn find_matches_in_str(
    haystack: &str,
    needle: &str,
    case_sensitive: bool,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || haystack.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    if case_sensitive {
        let mut matches = Vec::new();
        let needle_len = needle.len();
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            let match_start = start + pos;
            let match_end = match_start + needle_len;
            matches.push((match_start, match_end));
            start = match_end;
        }
        return matches;
    }

    // Case-insensitive matching
    if haystack.is_ascii() && needle.is_ascii() {
        let lower_haystack = haystack.to_ascii_lowercase();
        let lower_needle = needle.to_ascii_lowercase();
        let needle_len = lower_needle.len();
        let mut matches = Vec::new();
        let mut start = 0;
        while let Some(pos) = lower_haystack[start..].find(&lower_needle) {
            let match_start = start + pos;
            let match_end = match_start + needle_len;
            matches.push((match_start, match_end));
            start = match_end;
        }
        return matches;
    }

    // General Unicode case-insensitive matching
    let mut matches = Vec::new();
    let needle_chars: Vec<char> = needle.chars().collect();
    let needle_char_count = needle_chars.len();

    let char_indices: Vec<(usize, char)> = haystack.char_indices().collect();
    let total_chars = char_indices.len();
    let mut i = 0;

    while i + needle_char_count <= total_chars {
        let mut matched = true;
        for (offset, &nc) in needle_chars.iter().enumerate() {
            let hc = char_indices[i + offset].1;
            let mut h_lower = hc.to_lowercase();
            let mut n_lower = nc.to_lowercase();
            if h_lower.next() != n_lower.next()
                || h_lower.next().is_some()
                || n_lower.next().is_some()
            {
                matched = false;
                break;
            }
        }

        if matched {
            let start_byte = char_indices[i].0;
            let end_byte = if i + needle_char_count < total_chars {
                char_indices[i + needle_char_count].0
            } else {
                haystack.len()
            };
            matches.push((start_byte, end_byte));
            i += needle_char_count;
        } else {
            i += 1;
        }
    }

    matches
}

/// Find all non-overlapping occurrences within a specified byte range of `buffer`.
pub fn find_matches_in_range(
    buffer: &RopeBuffer,
    needle: &str,
    start_byte: usize,
    end_byte: usize,
    case_sensitive: bool,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || start_byte >= end_byte {
        return Vec::new();
    }
    let slice = buffer.slice_to_cow(start_byte, end_byte);
    let slice_matches = find_matches_in_str(&slice, needle, case_sensitive);
    slice_matches
        .into_iter()
        .map(|(s, e)| (start_byte + s, start_byte + e))
        .collect()
}

fn line_matches(
    buffer: &RopeBuffer,
    line_idx: usize,
    needle: &str,
    case_sensitive: bool,
) -> Vec<(usize, usize)> {
    if let Some(line) = buffer.line(line_idx) {
        let line_start_byte = buffer.line_to_byte(line_idx);
        let s = if let Some(as_str) = line.as_str() {
            std::borrow::Cow::Borrowed(as_str)
        } else {
            std::borrow::Cow::Owned(line.to_string())
        };
        find_matches_in_str(&s, needle, case_sensitive)
            .into_iter()
            .map(|(s, e)| (line_start_byte + s, line_start_byte + e))
            .collect()
    } else {
        Vec::new()
    }
}

/// Find the next occurrence of `needle` at or after `from_byte`.
/// Wraps around to the start of the buffer if reaching EOF.
pub fn find_next(
    buffer: &RopeBuffer,
    needle: &str,
    from_byte: usize,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    if needle.is_empty() || buffer.len_bytes() == 0 {
        return None;
    }

    if needle.contains('\n') {
        let text = buffer.slice_to_cow(0, buffer.len_bytes());
        let matches = find_matches_in_str(&text, needle, case_sensitive);
        if matches.is_empty() {
            return None;
        }
        for (s, e) in &matches {
            if *s >= from_byte {
                return Some((*s, *e));
            }
        }
        return matches.first().copied();
    }

    let total_lines = buffer.len_lines();
    let start_line = buffer.byte_to_line(from_byte);

    // 1. Search forward from start_line to end of file
    for line in start_line..total_lines {
        let matches = line_matches(buffer, line, needle, case_sensitive);
        for (s, e) in matches {
            if s >= from_byte {
                return Some((s, e));
            }
        }
    }

    // 2. Wrap around from line 0 to start_line
    for line in 0..=start_line {
        let matches = line_matches(buffer, line, needle, case_sensitive);
        for (s, e) in matches {
            if s < from_byte {
                return Some((s, e));
            }
        }
    }

    None
}

/// Find the previous occurrence of `needle` strictly before `from_byte`.
/// Wraps around to the end of the buffer if reaching BOF.
pub fn find_prev(
    buffer: &RopeBuffer,
    needle: &str,
    from_byte: usize,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    if needle.is_empty() || buffer.len_bytes() == 0 {
        return None;
    }

    if needle.contains('\n') {
        let text = buffer.slice_to_cow(0, buffer.len_bytes());
        let matches = find_matches_in_str(&text, needle, case_sensitive);
        if matches.is_empty() {
            return None;
        }
        for (s, e) in matches.iter().rev() {
            if *s < from_byte {
                return Some((*s, *e));
            }
        }
        return matches.last().copied();
    }

    let total_lines = buffer.len_lines();
    let start_line = buffer
        .byte_to_line(from_byte)
        .min(total_lines.saturating_sub(1));

    // 1. Search backward from start_line to line 0
    for line in (0..=start_line).rev() {
        let matches = line_matches(buffer, line, needle, case_sensitive);
        for (s, e) in matches.into_iter().rev() {
            if s < from_byte {
                return Some((s, e));
            }
        }
    }

    // 2. Wrap around from EOF to start_line
    for line in (start_line..total_lines).rev() {
        let matches = line_matches(buffer, line, needle, case_sensitive);
        for (s, e) in matches.into_iter().rev() {
            if s >= from_byte {
                return Some((s, e));
            }
        }
    }

    None
}

/// Count total non-overlapping occurrences of `needle` in `haystack` without extra allocations.
pub fn count_matches_in_str(haystack: &str, needle: &str, case_sensitive: bool) -> usize {
    if needle.is_empty() || haystack.is_empty() || needle.len() > haystack.len() {
        return 0;
    }

    if case_sensitive {
        let mut count = 0;
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            count += 1;
            start += pos + needle.len();
        }
        return count;
    }

    if haystack.is_ascii() && needle.is_ascii() {
        let lower_haystack = haystack.to_ascii_lowercase();
        let lower_needle = needle.to_ascii_lowercase();
        let mut count = 0;
        let mut start = 0;
        while let Some(pos) = lower_haystack[start..].find(&lower_needle) {
            count += 1;
            start += pos + lower_needle.len();
        }
        return count;
    }

    find_matches_in_str(haystack, needle, case_sensitive).len()
}

/// Count total non-overlapping occurrences of `needle` in `buffer`.
pub fn count_matches(buffer: &RopeBuffer, needle: &str, case_sensitive: bool) -> usize {
    if needle.is_empty() || buffer.len_bytes() == 0 {
        return 0;
    }

    if needle.contains('\n') {
        let text = buffer.slice_to_cow(0, buffer.len_bytes());
        return count_matches_in_str(&text, needle, case_sensitive);
    }

    let mut count = 0;
    for line_idx in 0..buffer.len_lines() {
        if let Some(line) = buffer.line(line_idx) {
            if let Some(as_str) = line.as_str() {
                count += count_matches_in_str(as_str, needle, case_sensitive);
            } else {
                let s = line.to_string();
                count += count_matches_in_str(&s, needle, case_sensitive);
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::RopeBuffer;

    #[test]
    fn test_find_in_str_basic_and_case() {
        let text = "Hello world, hello Rust!";
        let matches = find_matches_in_str(text, "hello", false);
        assert_eq!(matches, vec![(0, 5), (13, 18)]);

        let exact = find_matches_in_str(text, "Hello", true);
        assert_eq!(exact, vec![(0, 5)]);
    }

    #[test]
    fn test_find_multibyte_utf8() {
        let text = "Função de busca com emoji 🚀 e acentuação!";
        let matches = find_matches_in_str(text, "busca", false);
        assert_eq!(matches.len(), 1);
        let (s, e) = matches[0];
        assert_eq!(&text[s..e], "busca");

        let emoji_matches = find_matches_in_str(text, "🚀", false);
        assert_eq!(emoji_matches.len(), 1);
        let (es, ee) = emoji_matches[0];
        assert_eq!(&text[es..ee], "🚀");
    }

    #[test]
    fn test_find_next_and_prev_with_wraparound() {
        let buf = RopeBuffer::from_str("apple banana apple cherry apple");
        // Occurrences of "apple" at: 0..5, 13..18, 26..31

        assert_eq!(find_next(&buf, "apple", 0, true), Some((0, 5)));
        assert_eq!(find_next(&buf, "apple", 5, true), Some((13, 18)));
        assert_eq!(find_next(&buf, "apple", 18, true), Some((26, 31)));
        // Wrap around at EOF
        assert_eq!(find_next(&buf, "apple", 31, true), Some((0, 5)));

        // Backward search
        assert_eq!(find_prev(&buf, "apple", 31, true), Some((26, 31)));
        assert_eq!(find_prev(&buf, "apple", 26, true), Some((13, 18)));
        assert_eq!(find_prev(&buf, "apple", 13, true), Some((0, 5)));
        // Wrap around at BOF
        assert_eq!(find_prev(&buf, "apple", 0, true), Some((26, 31)));
    }

    #[test]
    fn test_find_empty_query() {
        let buf = RopeBuffer::from_str("sample text");
        assert_eq!(find_next(&buf, "", 0, false), None);
        assert_eq!(find_prev(&buf, "", 0, false), None);
        assert_eq!(count_matches(&buf, "", false), 0);
    }
}
