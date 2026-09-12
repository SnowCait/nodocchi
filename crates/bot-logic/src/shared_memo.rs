//! thread をまたいで純関数の結果を共有する memo の土台。
//!
//! 載せるのは「同じ key なら必ず同じ値になる純関数の結果」だけで、entry を捨てない。どの
//! thread が先に値を入れても、何 thread から引いても同じ値が返り、thread の終了順にも
//! worker 数にも依らない。
//!
//! key の同一性は `HashMap` の [`Eq`] そのもので、hash 値が一致しただけの別 key を同じ entry
//! として扱わない。hash を使うのは shard を選ぶところと `HashMap` 内部の bucket だけ。
//!
//! 共有するのは worker ごとの local memo が外した lookup だけという前提で、shard 単位の
//! [`RwLock`] を持つ。hot path をそのまま lock へ流す用途には向かない。

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};
use std::sync::RwLock;

use crate::count_hasher::CountHasherBuilder;

/// shard 数。2 の冪にして shard 選択を mask だけで行う。
const SHARDS: usize = 32;

pub struct SharedMemo<K, V> {
    shards: Box<[RwLock<HashMap<K, V, CountHasherBuilder>>]>,
    hasher: CountHasherBuilder,
}

impl<K, V> Default for SharedMemo<K, V> {
    fn default() -> Self {
        Self {
            shards: (0..SHARDS).map(|_| RwLock::default()).collect(),
            hasher: CountHasherBuilder::default(),
        }
    }
}

impl<K: Eq + Hash, V: Clone> SharedMemo<K, V> {
    /// 共有済みの値。無ければ `None` で、呼び出し側がその場で評価する。
    pub fn get(&self, key: &K) -> Option<V> {
        self.shard(key)
            .read()
            .expect("共有 memo の shard は poison しない")
            .get(key)
            .cloned()
    }

    /// 評価済みの値を共有する。同じ key に別 thread が先に入れていても値は同じなので、
    /// どちらが残っても結果は変わらない。
    pub fn insert(&self, key: K, value: V) {
        self.shard(&key)
            .write()
            .expect("共有 memo の shard は poison しない")
            .insert(key, value);
    }

    /// 共有している entry 数。memory footprint の観測用で、値も選択も変えない。
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| {
                shard
                    .read()
                    .expect("共有 memo の shard は poison しない")
                    .len()
            })
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // shard は hash の上位側から選ぶ。`HashMap` が bucket に使う側と重ならないようにする。
    fn shard(&self, key: &K) -> &RwLock<HashMap<K, V, CountHasherBuilder>> {
        let hash = self.hasher.hash_one(key);
        &self.shards[(hash >> 32) as usize % SHARDS]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_entry_comes_back_unchanged() {
        let memo: SharedMemo<u32, u64> = SharedMemo::default();

        assert_eq!(memo.get(&7), None);
        memo.insert(7, 11);
        assert_eq!(memo.get(&7), Some(11));
        assert_eq!(memo.get(&8), None);
        assert_eq!(memo.len(), 1);
    }

    // 同じ値を別 thread が同時に入れても、読み出す値は入れた順に依らない。
    #[test]
    fn concurrent_writers_agree_on_the_value() {
        let memo: SharedMemo<u32, u64> = SharedMemo::default();

        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for key in 0..1024u32 {
                        if memo.get(&key).is_none() {
                            memo.insert(key, u64::from(key) * 3);
                        }
                    }
                });
            }
        });

        for key in 0..1024u32 {
            assert_eq!(memo.get(&key), Some(u64::from(key) * 3));
        }
        assert_eq!(memo.len(), 1024);
    }
}
