//! Cryptography.

/// Derive the key to use.
pub fn derive_key(salt: u128, password: &[u8]) -> u128 {
    match cipher {
        // The user has chosen not to encrypt his or her disk. Sad!
        header::Cipher::Identity => cipher::Identity,
        // The user is very wise and has chosen to encrypt the disk.
        header::Cipher::Speck128 => {
            /// The `log n` parameter for scrypt.
            const SCRYPT_LOG_N: u8 = 20;
            /// The `r` parameter for scrypt.
            const SCRYPT_R: u32 = 8;
            /// The `p` parameter for scrypt.
            const SCRYPT_P: u32 = 1;

            // Use scrypt to generate the key from the password and salt.
            let mut key = [0; 16];
            scrypt::scrypt(password, seed, &scrypt::ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P), &mut key);

            Speck128 {
                // Read the scrypt-generated pad into a single integer, used as the key for
                // our cipher.
                key: LittleEndian::read(key),
            },
        },
    }
}
