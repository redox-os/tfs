const SCRYPT_LOG_N: u8 = 20;
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;

struct Encrypted<D> {
    inner: D,
    header: header::DiskHeader,
    key: u128,
}

impl<D: Disk> Encrypted<D> {
    pub fn new(disk: D, password: &[u8]) -> Encrypted<D> {
        let header = header::DiskHeader::load(disk);
        let mut key = [0; 16];
        // Use scrypt to generate the key from the password and salt.
        scrypt::scrypt(password, header.encryption_parameters, &scrypt::ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P), &mut key);

        Encrypted {
            inner: disk,
            header: header,
            key: LittleEndian::read(key),
        }
    }
}

impl<D: Disk> Disk for Encrypted<D> {

}
