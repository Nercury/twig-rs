//! `hash('xxh128', $data)`: the XXH3 128-bit hash in xxHash's canonical (big-endian) form, as
//! PHPSRC/ext/hash/hash_xxhash.c produces it (`XXH128_canonicalFromHash(XXH3_128bits_digest())`,
//! seed 0, bundled xxHash 0.8.1). Twig names `template_from_string` templates with it
//! (`__string_template__<hash>`, TWIG/src/Environment.php).

use crate::string::PhpString;

/// `hash('xxh128', $data, true)`: the 16 digest bytes, high 64 bits first, each half big-endian.
pub fn hash_xxh128_binary(data: &[u8]) -> [u8; 16] {
    xxhash_rust::xxh3::xxh3_128(data).to_be_bytes()
}

/// `hash('xxh128', $data)`: 32 lowercase hexadecimal digits.
pub fn hash_xxh128(data: &[u8]) -> PhpString {
    PhpString::from(format!("{:032x}", xxhash_rust::xxh3::xxh3_128(data)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdlib_vectors() {
        assert_eq!(hash_xxh128(b""), "99aa06d3014798d86001c324468d497f");
        assert_eq!(hash_xxh128(b"abc"), "06b05ab6733a618578af5f94892f3950");
        assert_eq!(
            hash_xxh128(b"Hi {{ x }"),
            "bc3540ba8d8676228047b5a35f275ff8"
        );
        assert_eq!(hash_xxh128_binary(b"")[..2], [0x99, 0xaa]);
    }
}
