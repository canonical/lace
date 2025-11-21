// SPDX-License-Identifier: GPL-2.0-only
// Copyright (C) 2025, Canonical Ltd.
// Authors: Mate Kukri <mate.kukri@canonical.com>

const SHA1_STATE_CNT: usize = 5;
const SHA1_BLOCK_SIZE: usize = 64;
const SHA1_DIGEST_SIZE: usize = 20;
const SHA1_ITERS: usize = 80;

/// One shot SHA1 hash computation, for non-security purposes only.
pub fn sha1(data: &[u8]) -> [u8; SHA1_DIGEST_SIZE] {
    let mut ctx = Sha1Ctx::new();
    ctx.update(data);
    ctx.digest()
}

/// This is a SHA1 implementation for non-security purposes only.
pub struct Sha1Ctx {
    state: [u32; SHA1_STATE_CNT],
    buffer: [u8; SHA1_BLOCK_SIZE],
    buffer_size: usize,
    total_size: usize,
}

impl Sha1Ctx {
    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0],
            buffer: [0u8; SHA1_BLOCK_SIZE],
            buffer_size: 0,
            total_size: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        while (self.buffer_size + data.len()) >= SHA1_BLOCK_SIZE {
            self.buffer[self.buffer_size..]
                .copy_from_slice(&data[..SHA1_BLOCK_SIZE - self.buffer_size]);
            data = &data[SHA1_BLOCK_SIZE - self.buffer_size..];

            sha1_transform(&mut self.state, &self.buffer);
            self.buffer_size = 0;
            self.total_size += SHA1_BLOCK_SIZE;
        }

        if data.len() > 0 {
            self.buffer[self.buffer_size..self.buffer_size + data.len()].copy_from_slice(data);
            self.buffer_size += data.len();
        }
    }

    pub fn digest(mut self) -> [u8; SHA1_DIGEST_SIZE] {
        assert!(self.buffer_size < SHA1_BLOCK_SIZE);

        self.buffer[self.buffer_size..].fill(0);
        self.buffer[self.buffer_size] = 0x80;

        if self.buffer_size >= SHA1_BLOCK_SIZE - 8 {
            sha1_transform(&mut self.state, &self.buffer);
            self.buffer.fill(0);
        }

        self.total_size += self.buffer_size;
        let bit_count = self.total_size * 8;
        self.buffer[SHA1_BLOCK_SIZE - 8..SHA1_BLOCK_SIZE]
            .copy_from_slice(bit_count.to_be_bytes().as_ref());
        sha1_transform(&mut self.state, &self.buffer);

        let mut digest = [0u8; SHA1_DIGEST_SIZE];
        for (i, chunk) in digest.chunks_mut(4).enumerate() {
            chunk.copy_from_slice(&self.state[i].to_be_bytes());
        }
        digest
    }
}

fn sha1_transform(state: &mut [u32; SHA1_STATE_CNT], block: &[u8; SHA1_BLOCK_SIZE]) {
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];

    let mut w: [u32; SHA1_ITERS] = [0u32; SHA1_ITERS];

    for i in 0..SHA1_ITERS {
        if i < 16 {
            w[i] = u32::from_be_bytes(block[i * 4..(i + 1) * 4].try_into().unwrap());
        } else {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (f, k) = if i < 20 {
            ((b & c) ^ ((!b) & d), 0x5a827999u32)
        } else if i < 40 {
            (b ^ c ^ d, 0x6ed9eba1u32)
        } else if i < 60 {
            ((b & c) ^ (b & d) ^ (c & d), 0x8f1bbcdcu32)
        } else {
            (b ^ c ^ d, 0xca62c1d6u32)
        };

        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(w[i]);

        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sha1_empty() {
        let digest = sha1(b"");
        let expected = [
            0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
            0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_sha1_abc() {
        let digest = sha1(b"abc");
        let expected = [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
            0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_sha1_long_pattern() {
        let data = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let digest = sha1(data);
        let expected = [
            0x84, 0x98, 0x3e, 0x44, 0x1c, 0x3b, 0xd2, 0x6e, 0xba, 0xae, 0x4a, 0xa1, 0xf9, 0x51,
            0x29, 0xe5, 0xe5, 0x46, 0x70, 0xf1,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_sha1_quick_cog() {
        let digest = sha1(b"The quick brown fox jumps over the lazy cog");
        let expected = [
            0xde, 0x9f, 0x2c, 0x7f, 0xd2, 0x5e, 0x1b, 0x3a, 0xfa, 0xd3, 0xe8, 0x5a, 0x0b, 0xd1,
            0x7d, 0x9b, 0x10, 0x0d, 0xb4, 0xb3,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_sha1_one_million_a() {
        let data = vec![b'a'; 1_000_000];
        let digest = sha1(&data);
        let expected = [
            0x34, 0xaa, 0x97, 0x3c, 0xd4, 0xc4, 0xda, 0xa4, 0xf6, 0x1e, 0xeb, 0x2b, 0xdb, 0xad,
            0x27, 0x31, 0x65, 0x34, 0x01, 0x6f,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_sha1_incremental_update() {
        let mut ctx = Sha1Ctx::new();
        ctx.update(b"The quick brown ");
        ctx.update(b"fox jumps over ");
        ctx.update(b"the lazy dog");
        let digest = ctx.digest();
        let expected = [
            0x2f, 0xd4, 0xe1, 0xc6, 0x7a, 0x2d, 0x28, 0xfc, 0xed, 0x84, 0x9e, 0xe1, 0xbb, 0x76,
            0xe7, 0x39, 0x1b, 0x93, 0xeb, 0x12,
        ];
        assert_eq!(digest, expected);
    }
}
