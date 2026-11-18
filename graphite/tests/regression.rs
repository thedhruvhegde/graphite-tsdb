//! Regression tests for bugs that only appear once data leaves the MemTable.

use graphite::{DB, LsmConfig};

fn small_cfg() -> LsmConfig {
    LsmConfig {
        auto_compact: false,
        memtable_flush_threshold: 100,
        ..Default::default()
    }
}

const BASE_NS: i64 = 1_700_000_000_000_000_000;

fn fill(db: &DB, n: i64) {
    for i in 0..n {
        db.insert("AAPL", BASE_NS + i * 1_000, 1.0, 2.0, 0.5, 100.0 + i as f64 * 0.01, 10)
            .unwrap();
    }
}

#[test]
fn insert_past_flush_threshold_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let db = DB::open_with_config(dir.path(), small_cfg()).unwrap();
    fill(&db, 450); // crosses the threshold four times
    assert_eq!(db.stats().total_rows, 450);
}

#[test]
fn flushed_sstables_are_readable() {
    let dir = tempfile::tempdir().unwrap();
    let db = DB::open_with_config(dir.path(), small_cfg()).unwrap();
    fill(&db, 450);
    let rows = db.query_range("AAPL", BASE_NS, BASE_NS + 1_000_000_000).unwrap().rows;
    assert_eq!(rows.len(), 450);
    let t = db.get("AAPL", BASE_NS + 10 * 1_000).unwrap().unwrap();
    assert!((t.close - 100.10).abs() < 1e-9);
}

#[test]
fn compaction_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    let db = DB::open_with_config(dir.path(), small_cfg()).unwrap();
    fill(&db, 450);
    db.compact().unwrap();
    let rows = db.query_range("AAPL", BASE_NS, BASE_NS + 1_000_000_000).unwrap().rows;
    assert_eq!(rows.len(), 450);
    assert_eq!(db.stats().level_sizes[0], 0, "L0 should be drained into L1");
}

#[test]
fn reopen_restores_symbols_and_data() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = DB::open_with_config(dir.path(), small_cfg()).unwrap();
        fill(&db, 450);
    }
    let db = DB::open_with_config(dir.path(), small_cfg()).unwrap();
    let rows = db.query_range("AAPL", BASE_NS, BASE_NS + 1_000_000_000).unwrap().rows;
    assert_eq!(rows.len(), 450, "flushed SSTables + replayed WAL");
    assert!(db.get("AAPL", BASE_NS + 449 * 1_000).unwrap().is_some());
}

#[test]
fn gql_accepts_nanosecond_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let db = DB::open_with_config(dir.path(), small_cfg()).unwrap();
    fill(&db, 10);
    let q = format!(
        "SELECT * FROM AAPL WHERE timestamp BETWEEN {} AND {}",
        BASE_NS,
        BASE_NS + 5_000
    );
    assert_eq!(db.query(&q).unwrap().rows.len(), 6);
}

#[test]
fn bloom_false_positive_rate_near_target() {
    let mut bf = graphite_core::BloomFilter::new(10_000);
    for i in 0..10_000u32 {
        bf.insert(&i.to_be_bytes());
    }
    for i in 0..10_000u32 {
        assert!(bf.contains(&i.to_be_bytes()), "no false negatives");
    }
    let fp = (10_000..110_000u32).filter(|i| bf.contains(&i.to_be_bytes())).count();
    let fpr = fp as f64 / 100_000.0;
    assert!(fpr < 0.03, "fpr was {fpr}");
}
