use graphene_node::cache::local::LocalDiskCache;
use graphene_node::cache::DependencyCache;
use rand::RngCore;
use std::fs;
use tokio::runtime::Builder;

#[test]
fn calculate_hash_is_order_insensitive() {
    let cache = LocalDiskCache::new("/tmp/cache-hash-test");
    let h1 = cache.calculate_hash(&["b".into(), "a".into(), "c".into()]);
    let h2 = cache.calculate_hash(&["c".into(), "b".into(), "a".into()]);
    assert_eq!(h1, h2, "hash should be deterministic ignoring order");
}

#[test]
fn put_and_get_roundtrip() {
    // unique temp dir
    let tmpdir = tempfile::tempdir().expect("tmpdir");
    let cache = LocalDiskCache::new(tmpdir.path().to_str().unwrap());

    // make a dummy artifact
    let mut data = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut data);
    let src = tmpdir.path().join("artifact.ext4");
    fs::write(&src, &data).expect("write artifact");

    let hash = "deadbeef";

    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");

    let dest_path = rt.block_on(cache.put(hash, src.clone())).expect("put");
    assert!(dest_path.exists(), "dest file should exist");

    // src should have been moved
    assert!(!src.exists(), "source should be moved (rename)");

    let got = rt.block_on(cache.get(hash)).expect("get");
    let got_path = got.expect("cache hit");
    assert_eq!(
        dest_path, got_path,
        "get should return the same cached path"
    );

    let read_back = fs::read(&got_path).expect("read back");
    assert_eq!(read_back, data, "cached data should match original");
}
