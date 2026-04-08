use crate::search::SearchResult;

/// Post-fusion dampening: diversity only.
/// Preserves absolute scores — only penalizes source clustering.
/// Applied after score combination, before truncation.
pub fn dampen(results: &mut [SearchResult], _spread_factor: f64) {
    if results.len() < 2 {
        return;
    }

    // Only apply diversity to results with distinct source keys
    // Skip if all results share the same source (e.g. all from signet://memory)
    let keys: std::collections::HashSet<String> = results.iter().map(extract_source_key).collect();
    if keys.len() > 1 {
        diversity(results);
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Pull outlier scores toward the mean to reduce bunching.
#[allow(dead_code)]
fn gravity(results: &mut [SearchResult], spread_factor: f64) {
    let mean = results.iter().map(|r| r.score).sum::<f64>() / results.len() as f64;
    for r in results.iter_mut() {
        r.score = mean + (r.score - mean) * spread_factor;
    }
}

/// Penalize results from the same source (file_path or session) when they cluster.
fn diversity(results: &mut [SearchResult]) {
    let mut source_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    // Determine source key: for memory chunks use session from name (locomo:conv:session:...),
    // for code use file_path
    for r in results.iter_mut() {
        let source_key = extract_source_key(r);
        let count = source_counts.entry(source_key).or_insert(0);
        *count += 1;

        if *count > 1 {
            // Each additional result from same source gets progressively penalized
            r.score *= 0.85f64.powi(*count as i32 - 1);
        }
    }
}

/// Boost results with higher importance field.
/// importance=0.0 → 0.95x, importance=1.0 → 1.0x
pub fn importance_boost(results: &mut [SearchResult]) {
    for r in results.iter_mut() {
        let importance = r.importance;
        r.score *= 0.95 + 0.05 * importance;
    }
}

fn extract_source_key(r: &SearchResult) -> String {
    // Memory chunks from locomo have names like "locomo:conv-26:session_1:..."
    // or "locomo-fact:conv-26:session_1:..."
    // Group by conversation+session
    if let Some(ref name) = r.name {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 3 && (parts[0] == "locomo" || parts[0] == "locomo-fact") {
            return format!("{}:{}", parts[1], parts[2]);
        }
    }
    // Fall back to file_path for code chunks
    r.file_path.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(score: f64, file_path: &str, name: Option<&str>) -> SearchResult {
        SearchResult {
            chunk_id: 0,
            file_path: file_path.to_string(),
            name: name.map(String::from),
            signature: None,
            line_start: 0,
            line_end: 0,
            chunk_type: "function".to_string(),
            source_type: "code".to_string(),
            score,
            vec_distance: None,
            fts_rank: None,
            snippet: None,
            decay_rate: 0.0,
            last_accessed: None,
            importance: 0.5,
        }
    }

    #[test]
    fn test_gravity_reduces_spread() {
        let mut results = vec![
            make_result(0.9, "a.rs", None),
            make_result(0.5, "b.rs", None),
            make_result(0.1, "c.rs", None),
        ];
        gravity(&mut results, 0.8);

        // Mean is 0.5, spread should be reduced
        assert!(results[0].score < 0.9);
        assert!(results[2].score > 0.1);
        // Mean should be preserved
        let new_mean = results.iter().map(|r| r.score).sum::<f64>() / 3.0;
        assert!((new_mean - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_diversity_penalizes_same_source() {
        let mut results = vec![
            make_result(0.8, "a.rs", None),
            make_result(0.7, "a.rs", None),
            make_result(0.6, "a.rs", None),
            make_result(0.5, "b.rs", None),
        ];
        diversity(&mut results);

        // First from a.rs: unchanged
        assert!((results[0].score - 0.8).abs() < 0.001);
        // Second from a.rs: penalized
        assert!(results[1].score < 0.7);
        // Third from a.rs: penalized more
        assert!(results[2].score < results[1].score);
        // b.rs: unchanged
        assert!((results[3].score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_diversity_groups_by_session_for_memory() {
        let mut results = vec![
            make_result(0.8, "mem", Some("locomo:conv-26:session_1:fact1")),
            make_result(0.7, "mem", Some("locomo:conv-26:session_1:fact2")),
            make_result(0.6, "mem", Some("locomo:conv-26:session_2:fact1")),
        ];
        diversity(&mut results);

        // Second from session_1 gets penalized
        assert!(results[1].score < 0.7);
        // First from session_2 does not
        assert!((results[2].score - 0.6).abs() < 0.001);
    }

    #[test]
    fn test_dampen_full_pipeline() {
        let mut results = vec![
            make_result(0.9, "a.rs", None),
            make_result(0.85, "a.rs", None),
            make_result(0.4, "b.rs", None),
        ];
        dampen(&mut results, 0.8);

        // Should still be sorted descending
        assert!(results[0].score >= results[1].score);
        assert!(results[1].score >= results[2].score);
    }
}
