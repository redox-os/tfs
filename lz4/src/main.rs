extern crate lz4_compress as lz4;

use std::env;
use std::io::{self, Write, Read};

fn main() {
    // Get and lock stdout.
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    match &*env::args().nth(1).unwrap_or(String::new()) {
        "-c" => {
            // Read stream from stdin.
            let mut vec = Vec::new();
            io::stdin().read_to_end(&mut vec).expect("Failed to read stdin");

            // Compress it and write the result to stdout.
            stdout.write(&lz4::compress(&vec)).expect("Failed to write to stdout");
        },
        "-d" => {
            // Read stream from stdin.
            let mut vec = Vec::new();
            io::stdin().read_to_end(&mut vec).expect("Failed to read stdin");

            // Decompress the input.
            let decompressed = lz4::decompress(&vec).expect("Compressed data contains errors");

            // Write the decompressed buffer to stdout.
            stdout.write(&decompressed).expect("Failed to write to stdout");
        },
        // If no valid arguments are given, we print the help page.
        _ => {
            stdout.write(b"\
            Introduction:\n\
                lz4 - an utility to decompress or compress a raw, headerless LZ4 stream.\n\
            Usage:\n\
                lz4 [option]\n\
            Options:\n\
                -c : Compress stdin and write the result to stdout.\n\
                -d : Decompress stdin and write the result to stdout.\n\
                -h : Write this manpage to stderr.\n\
            ").expect("Failed to write to stdout");
        },
    }
}
