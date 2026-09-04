//! 牌種ごとの枚数を key にした memo 用の [`Hasher`]。
//!
//! 向聴数探索や受け入れの骨格は 34 byte の枚数配列を key にした memo を探索 node ごとに引く。
//! 既定の hasher は攻撃者が key を選べる場面を想定した強度を持つ代わりに1回あたりの費用が高く、
//! 探索そのものより hashing が重くなる。ここでは 8 byte ずつ乗算で畳む安価な hasher を使う。
//!
//! memo が共有するのはどれも純関数の結果なので、hash 値が変わっても向聴数も受け入れも変わらない。

use std::hash::{BuildHasherDefault, Hasher};

// 64 bit 用の奇数乗数。
const MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;

#[derive(Default)]
pub(crate) struct CountHasher {
    hash: u64,
}

impl CountHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(MULTIPLIER);
    }
}

impl Hasher for CountHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, remainder) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.add(u64::from_le_bytes(*chunk));
        }

        if !remainder.is_empty() {
            let mut last = [0u8; 8];
            last[..remainder.len()].copy_from_slice(remainder);
            self.add(u64::from_le_bytes(last));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub(crate) type CountHasherBuilder = BuildHasherDefault<CountHasher>;
