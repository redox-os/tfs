#![feature(test, i128_type)]

extern crate test;
use test::Bencher;

extern crate rand;
extern crate speck;

use rand::Rng;
use rand::OsRng;

use speck::Key;

#[bench]
fn generate_key(mut bencher: &mut Bencher) {
    let mut rng = OsRng::new().unwrap();

    let key_input = rng.gen();

    bencher.iter(|| test::black_box(Key::new(key_input)));
}

#[bench]
fn encrypt(mut bencher: &mut Bencher) {
    let (key, block) = gen_test();

    bencher.iter(|| test::black_box(key.encrypt_block(block)));
}

#[bench]
fn decrypt(mut bencher: &mut Bencher) {
    let (key, block) = gen_test();

    bencher.iter(|| test::black_box(key.decrypt_block(block)));
}

fn gen_test() -> (Key, u128) {
    let mut rng = OsRng::new().unwrap();

    (Key::new(rng.gen()), rng.gen())
}
