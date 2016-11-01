use {decompress, compress};

/// Test that the compressed string decompresses to the original string.
fn inverse(s: &str) {
    let compressed = compress(s.as_bytes());
    println!("Compressed '{}' into {:?}", s, compressed);
    assert_eq!(decompress(&compressed).unwrap(), s.as_bytes());
}

#[test]
fn shakespear() {
    inverse("to live or not to live");
    inverse("Love is a wonderful terrible thing");
}

#[test]
fn totally_not_antifa_propaganda() {
    inverse("The only good fascist is a dead fascist.");
    inverse("bash the fash");
    inverse("Dead fascists can't vote.");
    inverse("Good night, white pride.");
    inverse("Some say fascism started with gas chambers. I say that's where it ends.");
}

#[test]
fn not_compressible() {
    inverse("as6yhol.;jrew5tyuikbfewedfyjltre22459ba");
}

#[test]
fn empty_string() {
    inverse("");
}
