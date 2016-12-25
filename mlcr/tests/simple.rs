extern crate mlcr;

#[test]
fn simple() {
    let mut cache = mlcr::Cache::new();

    cache.insert(1);
    cache.insert(2);
    cache.insert(3);
    cache.insert(4);
    cache.insert(100);
    cache.insert(200);

    cache.touch(100);
    cache.touch(100);
    cache.touch(1);
    cache.touch(2);
    cache.touch(2);
    cache.touch(2);
    cache.touch(2);
    cache.touch(2);
    cache.touch(100);
    cache.touch(2);
    cache.touch(2);
    cache.touch(2);
    cache.touch(100);
    cache.touch(100);
    cache.touch(100);
    cache.touch(1);
    cache.touch(2);

    assert_eq!(cache.cold().next(), Some(200));
    assert_eq!(cache.cold().next(), Some(100));
    assert_eq!(cache.cold().next(), Some(1));
}
