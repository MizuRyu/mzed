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

/// Match quality, best first. An exact name beats a prefix, which beats a
/// contiguous substring, which beats a scattered subsequence hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Exact,
    Prefix,
    Substring,
    Fuzzy,
}

/// Best tier (and its fuzzy score) `query` reaches across `keys`. `query` is
/// already lowercased and non-empty. `None` when no key matches at all.
fn best_tier(query: &str, keys: &[String]) -> Option<(Tier, i32)> {
    let mut best: Option<(Tier, i32)> = None;
    for key in keys {
        let lower = key.to_lowercase();
        let hit = if lower == query {
            Some((Tier::Exact, 0))
        } else if lower.starts_with(query) {
            Some((Tier::Prefix, 0))
        } else if lower.contains(query) {
            Some((Tier::Substring, 0))
        } else {
            fuzzy_match(query, key).map(|s| (Tier::Fuzzy, s))
        };
        best = match (best, hit) {
            (Some((bt, bs)), Some((t, s))) if t < bt || (t == bt && s > bs) => Some((t, s)),
            (None, hit) => hit,
            (best, _) => best,
        };
    }
    best
}

/// Filter and rank `items` by `query` into tiers: exact → prefix → substring →
/// fuzzy score, with the most recent item first inside a tier. `keys` returns
/// every string an item may be matched by (a file offers its relative path, its
/// name and its extension-less name); `recency` is any "bigger is newer" stamp.
/// An empty query returns every item, most recent first.
pub fn rank_tiered<'a, T, K, R>(query: &str, items: &'a [T], keys: K, recency: R) -> Vec<&'a T>
where
    K: Fn(&'a T) -> Vec<String>,
    R: Fn(&'a T) -> u64,
{
    let q = query.trim().to_lowercase();
    let mut scored: Vec<(&T, Tier, i32, u64)> = if q.is_empty() {
        items
            .iter()
            .map(|it| (it, Tier::Exact, 0, recency(it)))
            .collect()
    } else {
        items
            .iter()
            .filter_map(|it| best_tier(&q, &keys(it)).map(|(t, s)| (it, t, s, recency(it))))
            .collect()
    };
    scored.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.cmp(&a.3))
    });
    scored.into_iter().map(|(it, _, _, _)| it).collect()
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

    /// (keys, recency) rows for the tiered tests.
    fn row(keys: &[&str], recency: u64) -> (Vec<String>, u64) {
        (keys.iter().map(|s| s.to_string()).collect(), recency)
    }

    fn ranked_keys<'a>(query: &str, items: &'a [(Vec<String>, u64)]) -> Vec<&'a str> {
        rank_tiered(query, items, |it| it.0.clone(), |it| it.1)
            .into_iter()
            .map(|it| it.0[0].as_str())
            .collect()
    }

    #[test]
    fn rank_tieredはtier順に並ぶ() {
        let items = vec![
            row(&["replan.md"], 0),         // substring
            row(&["planning-notes.md"], 0), // prefix
            row(&["plan.md", "plan"], 0),   // exact (extension-less key)
            row(&["p-l-a-n-x.md"], 0),      // fuzzy only
        ];
        assert_eq!(
            ranked_keys("plan", &items),
            vec!["plan.md", "planning-notes.md", "replan.md", "p-l-a-n-x.md"]
        );
    }

    #[test]
    fn rank_tieredは拡張子なしでも完全一致() {
        let items = vec![row(&["docs/plan.md", "plan.md", "plan"], 0)];
        let ranked = rank_tiered("plan", &items, |it| it.0.clone(), |it| it.1);
        assert_eq!(ranked.len(), 1);
        // 完全一致なので、前方一致だけの候補より上に来る。
        let mixed = vec![
            row(&["plan-b.md", "plan-b"], 9),
            row(&["docs/plan.md", "plan.md", "plan"], 1),
        ];
        assert_eq!(
            ranked_keys("plan", &mixed),
            vec!["docs/plan.md", "plan-b.md"]
        );
    }

    #[test]
    fn 同tierでは新しい方が上() {
        let items = vec![
            row(&["plan-old.md"], 100),
            row(&["plan-new.md"], 500),
            row(&["plan-mid.md"], 300),
        ];
        assert_eq!(
            ranked_keys("plan", &items),
            vec!["plan-new.md", "plan-mid.md", "plan-old.md"]
        );
    }

    #[test]
    fn ファジー同士はスコアが優先されrecencyはタイブレークのみ() {
        // どちらも substring/prefix/exact ではない（scattered な subsequence のみ）
        // ので、両方 Tier::Fuzzy。連続一致+単語境界を多く持つ方が高スコア。
        let items = vec![
            row(&["xpxlxaxnx.md"], 999), // 低スコア・新しい
            row(&["p-l-a-n-x.md"], 1),   // 高スコア・古い
        ];
        assert_eq!(
            ranked_keys("plan", &items),
            vec!["p-l-a-n-x.md", "xpxlxaxnx.md"]
        );

        // 同スコア同士（末尾の飾り文字だけが違う）は recency で決まる。
        let tie = vec![row(&["p-l-a-n-x.md"], 1), row(&["p-l-a-n-y.md"], 2)];
        assert_eq!(
            ranked_keys("plan", &tie),
            vec!["p-l-a-n-y.md", "p-l-a-n-x.md"]
        );
    }

    #[test]
    fn 空クエリは全件が最近順() {
        let items = vec![row(&["a.md"], 1), row(&["b.md"], 3), row(&["c.md"], 2)];
        assert_eq!(ranked_keys("", &items), vec!["b.md", "c.md", "a.md"]);
        assert_eq!(ranked_keys("   ", &items).len(), 3);
    }

    #[test]
    fn rank_tieredはフォルダ名を含むクエリで当たる() {
        let items = vec![
            row(&["docs/specs/05-zed.md", "05-zed.md", "05-zed"], 0),
            row(&["README.md", "README"], 0),
        ];
        assert_eq!(
            ranked_keys("specs/05", &items),
            vec!["docs/specs/05-zed.md"]
        );
    }

    #[test]
    fn rank_tieredはマッチしない項目を除外する() {
        let items = vec![row(&["alpha.md"], 0), row(&["beta.md"], 0)];
        assert!(ranked_keys("zzzq", &items).is_empty());
    }
}
