/// Integration test: knowledge ingest → unified search
use sqmd_core::index::{KnowledgeChunk, KnowledgeIngestor};
use sqmd_core::schema;
use sqmd_core::search::{fts_search, SearchQuery};

#[test]
fn test_knowledge_ingest_and_unified_search() {
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    schema::init(&mut db).unwrap();

    // Verify schema v6
    let version: i64 = db
        .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 7);

    // Insert a code chunk
    db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/auth.ts', 'typescript', 100, 0.0, 'abc')", []).unwrap();
    db.execute(
        "INSERT INTO chunks (file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance, source_type)
         VALUES ('src/auth.ts', 'typescript', 'function', 'verifyToken', 'verifyToken(token: string)', 0, 10,
                 'async function verifyToken(token) { return jwt.verify(token); }', 'code1', 0.9, 'code')", []).unwrap();

    // Ingest knowledge via KnowledgeIngestor
    let ingestor = KnowledgeIngestor::new(&db);

    let fact = ingestor
        .ingest(&KnowledgeChunk {
            content: "Auth module rewritten for compliance. JWT now uses RS256.".into(),
            chunk_type: "fact".into(),
            source_type: "memory".into(),
            name: Some("Auth compliance rewrite".into()),
            importance: Some(0.8),
            agent_id: Some("agent-001".into()),
            tags: Some(vec!["auth".into()]),
            decay_rate: None,
            created_by: Some("extraction-worker".into()),
            metadata: None,
            relationships: None,
        })
        .unwrap();
    assert!(!fact.was_duplicate);

    let pref = ingestor
        .ingest(&KnowledgeChunk {
            content: "User prefers RS256 for JWT signing.".into(),
            chunk_type: "preference".into(),
            source_type: "memory".into(),
            name: Some("JWT preference".into()),
            importance: Some(0.75),
            agent_id: Some("agent-001".into()),
            tags: None,
            decay_rate: None,
            created_by: None,
            metadata: None,
            relationships: None,
        })
        .unwrap();
    assert!(!pref.was_duplicate);

    let summary = ingestor
        .ingest(&KnowledgeChunk {
            content: "Session: migrated auth from HS256 to RS256. Updated verifyToken and tests."
                .into(),
            chunk_type: "summary".into(),
            source_type: "transcript".into(),
            name: Some("Auth migration session".into()),
            importance: Some(0.6),
            agent_id: None,
            tags: None,
            decay_rate: None,
            created_by: Some("summary-worker".into()),
            metadata: None,
            relationships: None,
        })
        .unwrap();
    assert!(!summary.was_duplicate);

    // Dedup test
    let dup = ingestor
        .ingest(&KnowledgeChunk {
            content: "Auth module rewritten for compliance. JWT now uses RS256.".into(),
            chunk_type: "fact".into(),
            source_type: "memory".into(),
            name: None,
            importance: None,
            agent_id: None,
            tags: None,
            decay_rate: None,
            created_by: None,
            metadata: None,
            relationships: None,
        })
        .unwrap();
    assert!(dup.was_duplicate);
    assert_eq!(dup.chunk_id, fact.chunk_id);

    // Unified FTS search
    let results = fts_search(
        &db,
        &SearchQuery {
            text: "JWT token auth verify".into(),
            top_k: 10,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!results.is_empty());

    let types: Vec<&str> = results.iter().map(|r| r.source_type.as_str()).collect();
    println!("Results ({}):", results.len());
    for r in &results {
        println!(
            "  [{:.2}] [{}] {} — {}",
            r.score,
            r.source_type,
            r.chunk_type,
            r.name.as_deref().unwrap_or("(unnamed)")
        );
    }
    assert!(
        types.contains(&"code") || types.contains(&"memory"),
        "Should find code or memory results"
    );

    // Source type filter
    let mem_results = fts_search(
        &db,
        &SearchQuery {
            text: "auth compliance".into(),
            top_k: 10,
            source_type_filter: Some(vec!["memory".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    for r in &mem_results {
        assert_eq!(r.source_type, "memory");
    }

    // Batch ingest
    let batch = ingestor
        .ingest_batch(&[KnowledgeChunk {
            content: "DB pool size increased to 20 for production.".into(),
            chunk_type: "decision".into(),
            source_type: "memory".into(),
            name: Some("DB pool decision".into()),
            importance: Some(0.85),
            agent_id: None,
            tags: None,
            decay_rate: None,
            created_by: None,
            metadata: None,
            relationships: None,
        }])
        .unwrap();
    assert_eq!(batch.ingested, 1);

    // Totals
    let total: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(total, 5); // 1 code + 3 memory + 1 transcript (dup not counted)

    // Forget
    assert!(ingestor.forget(fact.chunk_id).unwrap());
    let deleted: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE id = ?1 AND is_deleted = 1",
            rusqlite::params![fact.chunk_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1);

    // Extended relationship types
    db.execute(
        "INSERT INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, 'contradicts')",
        rusqlite::params![pref.chunk_id, batch.results[0].chunk_id],
    )
    .unwrap();
    let rels: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM relationships WHERE rel_type = 'contradicts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rels, 1);

    println!("\n✓ All integration tests passed!");
}
