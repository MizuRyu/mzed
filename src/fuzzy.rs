//! Subsequence fuzzy matching for the command palette and file search.
//!
//! `fuzzy_match` returns `None` when `query` is not a (case-insensitive)
//! subsequence of `candidate`, otherwise `Some(score)` where a higher score is
//! a better match. Scoring rewards contiguous runs and matches at word
//! boundaries (start of string, or after a separator), so that typing the
//! initials or a leading prefix ranks above scattered matches.

/// Match `query` against `candidate` as a case-insensitive subsequence.
///
/// Empty queries match everything with a score of 0. Returns `None` if any
/// query character cannot be found in order.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let q: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let c: Vec<char> = candidate.chars().collect();
    let c_lower: Vec<char> = candidate.chars().flat_map(|ch| ch.to_lowercase()).collect();
    // `to_lowercase` can change length; fall back to a simpler path if so to
    // keep index alignment between `c` and `c_lower`.
    let aligned = c.len() == c_lower.len();

    let mut score = 0;
    let mut qi = 0;
    let mut prev_match: Option<usize> = None;

    for (ci, &lc) in c_lower.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if lc == q[qi] {
            // Base reward for a match.
            score += 1;
            // Contiguous with the previous match: reward a run.
            if let Some(p) = prev_match {
                if p + 1 == ci {
                    score += 5;
                }
            }
            // Word-boundary bonus: start of string or after a separator.
            let at_boundary = ci == 0
                || matches!(
                    c.get(ci.wrapping_sub(1)),
                    Some(' ') | Some('/') | Some('_') | Some('-') | Some('.')
                );
            if at_boundary {
                score += 8;
            }
            // Uppercase camel-hump boundary (only when indices align).
            if aligned && ci > 0 && c[ci].is_uppercase() {
                score += 4;
            }
            prev_match = Some(ci);
            qi += 1;
        }
    }

    if qi == q.len() {
        // Shorter candidates with the same matches rank slightly higher.
        Some(score - (c.len() as i32) / 50)
    } else {
        None
    }
}

/// Filter and rank `items` by `query`, returning matching items paired with
/// their score, sorted best-first. The `key` closure extracts the text to match
/// for each item. Ties keep the original order (stable sort).
pub fn rank<'a, T, F>(query: &str, items: &'a [T], key: F) -> Vec<(&'a T, i32)>
where
    F: Fn(&T) -> &str,
{
    let mut scored: Vec<(&T, i32)> = items
        .iter()
        .filter_map(|it| fuzzy_match(query, key(it)).map(|s| (it, s)))
        .collect();
    scored.sort_by_key(|x| std::cmp::Reverse(x.1));
    scored
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn 部分列がマッチする() {
        // "tt" は "Toggle Theme" の部分列。
        assert!(fuzzy_match("tt", "Toggle Theme").is_some());
        assert!(fuzzy_match("theme", "Toggle Theme").is_some());
    }

    #[test]
    fn 大文字小文字を無視する() {
        assert!(fuzzy_match("THEME", "toggle theme").is_some());
        assert!(fuzzy_match("theme", "TOGGLE THEME").is_some());
    }

    #[test]
    fn 順序が違うとマッチしない() {
        // "emht" は "theme" の部分列ではない。
        assert!(fuzzy_match("emht", "theme").is_none());
    }

    #[test]
    fn 含まれない文字はマッチしない() {
        assert!(fuzzy_match("xyz", "Toggle Theme").is_none());
    }

    #[test]
    fn 空クエリは常にマッチする() {
        assert_eq!(fuzzy_match("", "anything"), Some(0));
    }

    #[test]
    fn 連続一致は飛び石より高スコア() {
        // "ab" 連続 vs a...b 飛び石(境界ボーナスが無い同条件で比較)。
        let contiguous = fuzzy_match("ab", "xabc").unwrap();
        let scattered = fuzzy_match("ab", "xaxbc").unwrap();
        assert!(contiguous > scattered);
    }

    #[test]
    fn 単語境界一致は高スコア() {
        // "tt" は両単語の先頭にマッチ -> 境界ボーナス。
        let boundary = fuzzy_match("tt", "Toggle Theme").unwrap();
        let inner = fuzzy_match("og", "Toggle Theme").unwrap();
        assert!(boundary > inner);
    }

    #[test]
    fn rankはスコア順に並ぶ() {
        let items = vec!["Toggle Theme", "Open Tab", "Theme picker"];
        let ranked = rank("theme", &items, |s| s);
        // "Theme picker"(先頭境界一致) が "Toggle Theme" より上。
        assert_eq!(ranked.len(), 2); // "Open Tab" はマッチしない。
        assert_eq!(*ranked[0].0, "Theme picker");
    }

    #[test]
    fn rankはマッチしない項目を除外する() {
        let items = vec!["alpha", "beta", "gamma"];
        let ranked = rank("zzz", &items, |s| s);
        assert!(ranked.is_empty());
    }
}
