use crate::search::SearchResult;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct QueryCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,
    max_entries: usize,
}

struct CacheEntry {
    results: Vec<SearchResult>,
    created_at: Instant,
}

impl QueryCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(10),
            max_entries: 256,
        }
    }

    fn make_key(
        query_text: &str,
        top_k: usize,
        file: Option<&str>,
        type_f: Option<&str>,
        sources: Option<&[String]>,
        agent_id: Option<&str>,
    ) -> String {
        let mut key = query_text.to_string();
        key.push('|');
        key.push_str(&top_k.to_string());
        if let Some(f) = file {
            key.push_str("|file:");
            key.push_str(f);
        }
        if let Some(t) = type_f {
            key.push_str("|type:");
            key.push_str(t);
        }
        if let Some(s) = sources {
            key.push_str("|src:");
            key.push_str(&s.join(","));
        }
        if let Some(a) = agent_id {
            key.push_str("|agent:");
            key.push_str(a);
        }
        key
    }

    pub fn get(&self, key: &str) -> Option<&Vec<SearchResult>> {
        let entry = self.entries.get(key)?;
        if entry.created_at.elapsed() > self.ttl {
            return None;
        }
        Some(&entry.results)
    }

    pub fn insert(&mut self, key: String, results: Vec<SearchResult>) {
        if self.entries.len() >= self.max_entries {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.created_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.entries.remove(&k);
            }
        }
        self.entries.insert(
            key,
            CacheEntry {
                results,
                created_at: Instant::now(),
            },
        );
    }

    pub fn lookup(
        &self,
        query_text: &str,
        top_k: usize,
        file: Option<&str>,
        type_f: Option<&str>,
        sources: Option<&[String]>,
        agent_id: Option<&str>,
    ) -> Option<Vec<SearchResult>> {
        let key = Self::make_key(query_text, top_k, file, type_f, sources, agent_id);
        self.get(&key).cloned()
    }

    pub fn store(
        &mut self,
        query_text: &str,
        top_k: usize,
        file: Option<&str>,
        type_f: Option<&str>,
        sources: Option<&[String]>,
        agent_id: Option<&str>,
        results: Vec<SearchResult>,
    ) {
        let key = Self::make_key(query_text, top_k, file, type_f, sources, agent_id);
        self.insert(key, results);
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(id: i64) -> SearchResult {
        SearchResult {
            chunk_id: id,
            file_path: "test.rs".to_string(),
            name: None,
            signature: None,
            line_start: 0,
            line_end: 0,
            chunk_type: "function".to_string(),
            source_type: "code".to_string(),
            score: 0.5,
            vec_distance: None,
            fts_rank: None,
            snippet: None,
            decay_rate: 0.0,
            last_accessed: None,
            importance: 0.5,
        }
    }

    #[test]
    fn test_cache_hit() {
        let mut cache = QueryCache::new();
        let results = vec![make_result(1), make_result(2)];
        cache.store("hello", 10, None, None, None, None, results.clone());
        let cached = cache.lookup("hello", 10, None, None, None, None).unwrap();
        assert_eq!(cached.len(), 2);
    }

    #[test]
    fn test_cache_miss_different_params() {
        let mut cache = QueryCache::new();
        cache.store("hello", 10, None, None, None, None, vec![make_result(1)]);
        let cached = cache.lookup("hello", 5, None, None, None, None);
        assert!(cached.is_none());
    }
}
