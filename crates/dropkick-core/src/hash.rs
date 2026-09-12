//! SHA-256, SHA-1 and MD5, written out longhand.
//!
//! `dropkick-core` has no dependencies and keeps none, so these are here
//! rather than pulled in. That is affordable precisely because hashes are the
//! best-specified code in the world: every one of them has official test
//! vectors, so "did I transcribe the constants correctly" is a question the
//! test suite answers rather than a thing to worry about.
//!
//! **Two of these three are broken.** SHA-1 has practical collisions and MD5
//! has trivial ones; neither belongs anywhere near a security decision. They
//! are here because the common reason to hash something from a clipboard is
//! checking it against a checksum somebody published, and publishers still
//! use both. [`Algo::caution`] is what puts that on screen.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Sha256,
    Sha1,
    Md5,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Sha256 => "sha-256",
            Algo::Sha1 => "sha-1",
            Algo::Md5 => "md5",
        }
    }

    /// The warning shown under a broken algorithm, or `None` for one that is
    /// still sound.
    ///
    /// Both halves matter. "Broken" alone would suggest the answer is wrong,
    /// which it is not — a SHA-1 digest still matches the SHA-1 a publisher
    /// printed. What is broken is the guarantee that *only* the same input
    /// produces it, and that only matters when someone might be trying.
    pub fn caution(self) -> Option<&'static str> {
        match self {
            Algo::Sha256 => None,
            Algo::Sha1 => {
                Some("sha-1 is broken: fine for checking a download, never for security")
            }
            Algo::Md5 => {
                Some("md5 is badly broken: fine for checking a download, never for security")
            }
        }
    }

    /// The order `h` walks. SHA-256 is first because it is the one that is
    /// still trustworthy, and the first press of a key should not hand anyone
    /// a broken algorithm.
    pub fn next(self) -> Algo {
        match self {
            Algo::Sha256 => Algo::Sha1,
            Algo::Sha1 => Algo::Md5,
            Algo::Md5 => Algo::Sha256,
        }
    }

    pub fn hex_of(self, msg: &[u8]) -> String {
        match self {
            Algo::Sha256 => hex(&sha256(msg)),
            Algo::Sha1 => hex(&sha1(msg)),
            Algo::Md5 => hex(&md5(msg)),
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0xf) as usize] as char);
    }
    s
}

/// Pad to a multiple of 64 bytes: one `0x80`, then zeroes, then the message
/// length in bits.
///
/// `big_endian` is the whole difference between the SHA family and MD5 here,
/// and getting it backwards produces a plausible-looking wrong answer — which
/// is why the vectors below are not optional.
fn padded(msg: &[u8], big_endian: bool) -> Vec<u8> {
    let mut v = Vec::with_capacity(msg.len() + 72);
    v.extend_from_slice(msg);
    v.push(0x80);
    while v.len() % 64 != 56 {
        v.push(0);
    }
    let bits = (msg.len() as u64).wrapping_mul(8);
    if big_endian {
        v.extend_from_slice(&bits.to_be_bytes());
    } else {
        v.extend_from_slice(&bits.to_le_bytes());
    }
    v
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

pub fn sha256(msg: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    for block in padded(msg, true).chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let b = &block[i * 4..i * 4 + 4];
            *word = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn sha1(msg: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    for block in padded(msg, true).chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let b = &block[i * 4..i * 4 + 4];
            *word = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[rustfmt::skip]
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `floor(2^32 * abs(sin(i + 1)))`, tabled rather than computed: core has no
/// float maths worth trusting for this and the values are fixed forever.
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
    0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
    0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
    0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
    0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
    0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
    0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
    0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
    0xeb86d391,
];

pub fn md5(msg: &[u8]) -> [u8; 16] {
    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    // Little-endian, unlike the two above. This is the classic way to get a
    // wrong answer that looks entirely reasonable.
    for block in padded(msg, false).chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            let b = &block[i * 4..i * 4 + 4];
            *word = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f
                .wrapping_add(a)
                .wrapping_add(MD5_K[i])
                .wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(MD5_S[i]));
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 16];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published vectors. These are why hand-rolling this is reasonable:
    /// a transcription error in any of the constants fails here rather than
    /// producing a confident wrong answer somebody pastes into a form.
    #[test]
    fn sha256_matches_the_published_vectors() {
        for (input, want) in [
            ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
        ] {
            assert_eq!(hex(&sha256(input.as_bytes())), want, "sha256({input:?})");
        }
    }

    #[test]
    fn sha1_matches_the_published_vectors() {
        for (input, want) in [
            ("", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            ("abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
            ),
        ] {
            assert_eq!(hex(&sha1(input.as_bytes())), want, "sha1({input:?})");
        }
    }

    #[test]
    fn md5_matches_the_published_vectors() {
        // From RFC 1321's own test suite.
        for (input, want) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            ("abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(hex(&md5(input.as_bytes())), want, "md5({input:?})");
        }
    }

    /// A message long enough to need many blocks, which is where a padding or
    /// length-encoding mistake shows up and a short one does not.
    #[test]
    fn a_long_message_hashes_correctly() {
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha256(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        assert_eq!(hex(&sha1(&million)), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
        assert_eq!(hex(&md5(&million)), "7707d6ae4e027c70eea2a935c2296f21");
    }

    /// Exactly 55, 56 and 64 bytes: the lengths where padding either just
    /// fits, just spills into a second block, or fills one exactly.
    #[test]
    fn the_awkward_lengths_around_a_block_boundary_are_right() {
        for n in [55usize, 56, 63, 64, 65] {
            let msg = vec![b'x'; n];
            assert_eq!(hex(&sha256(&msg)).len(), 64, "sha256 at {n}");
            assert_eq!(hex(&sha1(&msg)).len(), 40, "sha1 at {n}");
            assert_eq!(hex(&md5(&msg)).len(), 32, "md5 at {n}");
        }
        // 56 is the nastiest of them -- the length field is the first thing
        // pushed into the second block -- so it is pinned to real answers.
        // These came from coreutils (`md5sum`, `sha1sum`, `sha256sum`) rather
        // than from this code, so they check it against something that was
        // written by somebody else.
        let x56 = vec![b'x'; 56];
        assert_eq!(hex(&md5(&x56)), "668a72d5ba17f08e62dabcafad6db14b");
        assert_eq!(hex(&sha1(&x56)), "901305367c259952f4e7af8323f480d59f81335b");
        assert_eq!(
            hex(&sha256(&x56)),
            "04c26261370ee7541549d16dee320c723e3fd14671e66a099afe0a377c16888e"
        );
    }

    #[test]
    fn the_broken_ones_say_so_and_the_sound_one_does_not() {
        assert!(Algo::Sha256.caution().is_none());
        assert!(Algo::Sha1.caution().unwrap().contains("broken"));
        assert!(Algo::Md5.caution().unwrap().contains("broken"));
    }

    #[test]
    fn cycling_starts_and_returns_to_the_trustworthy_one() {
        assert_eq!(Algo::Sha256.next(), Algo::Sha1);
        assert_eq!(Algo::Sha1.next(), Algo::Md5);
        assert_eq!(Algo::Md5.next(), Algo::Sha256);
    }
}
