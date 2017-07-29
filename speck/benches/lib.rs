#![feature(test, i128_type)]

extern crate test;
use test::Bencher;

extern crate rand;
extern crate speck;

use rand::Rng;

#[bench]
fn encrypt(mut bencher: &mut Bencher) {
    let mut rng = rand::OsRng::new().unwrap();

    let key = speck::Key::new(rng.gen());

    let block: u128 = rng.gen();

    bencher.iter(|| test::black_box(key.encrypt_block(block)));
}

#[bench]
fn decrypt(mut bencher: &mut Bencher) {
    let mut rng = rand::OsRng::new().unwrap();

    let key = speck::Key::new(rng.gen());

    let block: u128 = rng.gen();

    bencher.iter(|| test::black_box(key.decrypt_block(block)));
}
