//! 差分検証で使い回す完成手の一覧。
//!
//! 役・役満の判定は decomposition ごとに走るので、狙った役形と固定面子・槓の有無を並べた
//! 決め打ちの手牌に、固定 seed の擬似乱数で作った手牌を足して面を広げる。判定規則そのものは
//! 持たず、[`analyze_completed_hand`] に渡す牌姿を組み立てるだけ。

use crate::completed_hand::{CompletedHandAnalysis, analyze_completed_hand};
use crate::meld::{Meld, MeldKind};
use crate::tile::{TileId, TileType};
use crate::winning_context::{RiichiStatus, WinMethod, WinningContext};

/// ロン・ツモ、状況役、点数計算に必要な事実の欠落を含む差分検証用の入力。
pub(crate) fn winning_contexts() -> impl Iterator<Item = WinningContext> {
    [WinMethod::Ron, WinMethod::Tsumo]
        .into_iter()
        .flat_map(|method| {
            let exact = WinningContext::new(method)
                .with_round_wind(Some(TileType::from_mjai_type_str("E").unwrap()))
                .with_seat_wind(Some(TileType::from_mjai_type_str("S").unwrap()))
                .with_riichi(RiichiStatus::NotDeclared)
                .with_ippatsu(Some(false))
                .with_rinshan(Some(false))
                .with_chankan(Some(false))
                .with_remaining_live_tiles(Some(12));
            [
                WinningContext::new(method),
                exact,
                exact.with_riichi(RiichiStatus::Riichi),
                exact
                    .with_riichi(RiichiStatus::DoubleRiichi)
                    .with_ippatsu(Some(true)),
                exact.with_remaining_live_tiles(Some(0)),
                exact.with_rinshan(Some(true)).with_chankan(Some(true)),
                exact.with_round_wind(None),
                exact.with_seat_wind(None),
                exact.with_riichi(RiichiStatus::Unknown),
                exact.with_riichi(RiichiStatus::Riichi).with_ippatsu(None),
                exact.with_rinshan(None).with_chankan(None),
                exact.with_remaining_live_tiles(None),
            ]
        })
}

/// 面子1つ分の牌姿。
#[derive(Debug, Clone, Copy)]
enum Group {
    Pair(u8),
    Triplet(u8),
    Sequence(u8),
    Kan(u8),
}

impl Group {
    fn tile_types(self) -> Vec<u8> {
        match self {
            Self::Pair(tile) => vec![tile, tile],
            Self::Triplet(tile) => vec![tile, tile, tile],
            Self::Kan(tile) => vec![tile, tile, tile, tile],
            Self::Sequence(start) => vec![start, start + 1, start + 2],
        }
    }
}

/// 同じ牌種を5枚使わないように配る物理牌の割り当て。
struct TileSource {
    used: [u8; TileType::COUNT],
}

impl TileSource {
    fn new() -> Self {
        Self {
            used: [0; TileType::COUNT],
        }
    }

    fn take(&mut self, raw: u8) -> Option<TileId> {
        let copy = &mut self.used[usize::from(raw)];
        if *copy >= 4 {
            return None;
        }
        let tile = TileId::new(raw * 4 + *copy)?;
        *copy += 1;
        Some(tile)
    }

    fn group(&mut self, group: Group) -> Option<Vec<TileId>> {
        group
            .tile_types()
            .into_iter()
            .map(|tile| self.take(tile))
            .collect()
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }

    fn below(&mut self, bound: u64) -> u8 {
        (self.next() % bound) as u8
    }
}

fn analysis(concealed: &[TileId], fixed_melds: &[Meld]) -> Option<CompletedHandAnalysis> {
    analyze_completed_hand(concealed, fixed_melds).ok()
}

fn from_groups(
    groups: &[Group],
    fixed_kinds: &[Option<MeldKind>],
) -> Option<CompletedHandAnalysis> {
    let mut source = TileSource::new();
    let mut concealed = Vec::new();
    let mut fixed_melds = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let tiles = source.group(*group)?;
        match fixed_kinds.get(index).copied().flatten() {
            Some(kind) => {
                let called_tile = kind.is_open().then(|| tiles[0]);
                fixed_melds.push(Meld::new(kind, tiles, called_tile));
            }
            None => concealed.extend(tiles),
        }
    }
    analysis(&concealed, &fixed_melds)
}

fn from_tile_strings(tiles: &[&str]) -> Option<CompletedHandAnalysis> {
    let mut source = TileSource::new();
    let concealed = tiles
        .iter()
        .map(|tile| {
            let tile_type = TileType::from_mjai_type_str(tile).expect("牌姿は正当な mjai 表記");
            source.take(tile_type.raw())
        })
        .collect::<Option<Vec<_>>>()?;
    analysis(&concealed, &[])
}

fn named_hands(corpus: &mut Vec<CompletedHandAnalysis>) {
    let hands: &[&[&str]] = &[
        // 九蓮宝燈と、そこから1枚崩した形。
        &[
            "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m", "1m",
        ],
        &[
            "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m", "5m",
        ],
        &[
            "1p", "1p", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p", "9p", "9p", "3p",
        ],
        &[
            "1s", "1s", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s", "9s", "9s", "9s",
        ],
        &[
            "1m", "1m", "2m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m", "9m",
        ],
        // 七対子・国士無双。
        &[
            "1m", "1m", "3m", "3m", "5m", "5m", "7m", "7m", "9m", "9m", "1p", "1p", "3p", "3p",
        ],
        &[
            "2m", "2m", "3m", "3m", "5m", "5m", "7m", "7m", "8m", "8m", "2p", "2p", "3p", "3p",
        ],
        &[
            "E", "E", "S", "S", "W", "W", "N", "N", "P", "P", "F", "F", "C", "C",
        ],
        &[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "C",
        ],
        &[
            "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C", "1m",
        ],
        // 牌種構成で決まる役・役満。
        &[
            "1m", "1m", "1m", "9m", "9m", "9m", "1p", "1p", "1p", "E", "E", "E", "C", "C",
        ],
        &[
            "1m", "1m", "1m", "9m", "9m", "9m", "1p", "1p", "1p", "9s", "9s", "9s", "1s", "1s",
        ],
        &[
            "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "P", "P",
        ],
        &[
            "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "6s", "6s", "F", "F",
        ],
        &[
            "2s", "3s", "4s", "2s", "3s", "4s", "6s", "6s", "6s", "8s", "8s", "8s", "F", "F",
        ],
        &[
            "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "E", "E", "E", "C", "C",
        ],
        &[
            "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "7m", "8m", "9m", "5m", "5m",
        ],
        &[
            "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5p", "6p", "7p", "8s", "8s",
        ],
        // 三元牌・風牌の集まり。
        &[
            "P", "P", "P", "F", "F", "F", "C", "C", "C", "2m", "3m", "4m", "5m", "5m",
        ],
        &[
            "P", "P", "P", "F", "F", "F", "C", "C", "2m", "3m", "4m", "5m", "6m", "7m",
        ],
        &[
            "P", "P", "P", "F", "F", "F", "C", "C", "C", "2m", "2m", "2m", "5m", "5m",
        ],
        &[
            "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "2m", "3m", "4m",
        ],
        &[
            "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "5m", "5m",
        ],
        &[
            "E", "E", "E", "S", "S", "S", "W", "W", "W", "N", "N", "N", "N", "N",
        ],
        // 複数の分解を持つ形。
        &[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "4m", "4m", "4m", "5m", "5m",
        ],
        &[
            "1m", "1m", "1m", "2m", "2m", "2m", "3m", "3m", "3m", "E", "E", "E", "5m", "5m",
        ],
        &[
            "2m", "2m", "3m", "3m", "4m", "4m", "5m", "5m", "6m", "6m", "7m", "7m", "8m", "8m",
        ],
        &[
            "1p", "1p", "1p", "2p", "2p", "2p", "3p", "3p", "3p", "7p", "8p", "9p", "5p", "5p",
        ],
        &[
            "P", "P", "P", "F", "F", "F", "C", "C", "C", "2s", "2s", "2s", "3s", "3s",
        ],
        &[
            "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "6s", "6s", "8s", "8s",
        ],
        &[
            "1m", "1m", "1m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "9m", "9m",
        ],
        &[
            "2m", "3m", "4m", "2m", "3m", "4m", "2m", "3m", "4m", "2m", "3m", "4m", "5m", "5m",
        ],
        &[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "2p", "3p", "4p", "5s", "5s",
        ],
        &[
            "1m", "2m", "3m", "7m", "8m", "9m", "1p", "2p", "3p", "9s", "9s", "9s", "1s", "1s",
        ],
        &[
            "1m", "2m", "3m", "7m", "8m", "9m", "1p", "2p", "3p", "E", "E", "E", "1s", "1s",
        ],
        // 完成していない牌姿。
        &[
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
        ],
    ];
    corpus.extend(hands.iter().filter_map(|tiles| from_tile_strings(tiles)));
}

fn melded_hands(corpus: &mut Vec<CompletedHandAnalysis>) {
    let cases: &[(&[Group], &[Option<MeldKind>])] = &[
        (
            &[
                Group::Pair(4),
                Group::Triplet(31),
                Group::Triplet(32),
                Group::Triplet(33),
                Group::Sequence(0),
            ],
            &[None, Some(MeldKind::Pon), Some(MeldKind::Pon), None, None],
        ),
        (
            &[
                Group::Pair(4),
                Group::Kan(0),
                Group::Kan(9),
                Group::Kan(18),
                Group::Kan(27),
            ],
            &[
                None,
                Some(MeldKind::Ankan),
                Some(MeldKind::Daiminkan),
                Some(MeldKind::Kakan),
                Some(MeldKind::Ankan),
            ],
        ),
        (
            &[
                Group::Pair(4),
                Group::Kan(0),
                Group::Kan(9),
                Group::Kan(18),
                Group::Sequence(20),
            ],
            &[
                None,
                Some(MeldKind::Ankan),
                Some(MeldKind::Daiminkan),
                Some(MeldKind::Kakan),
                None,
            ],
        ),
        (
            &[
                Group::Pair(31),
                Group::Triplet(0),
                Group::Sequence(1),
                Group::Sequence(4),
                Group::Triplet(27),
            ],
            &[
                None,
                Some(MeldKind::Pon),
                Some(MeldKind::Chi),
                None,
                Some(MeldKind::Pon),
            ],
        ),
        (
            &[
                Group::Pair(0),
                Group::Kan(8),
                Group::Triplet(9),
                Group::Triplet(17),
                Group::Triplet(26),
            ],
            &[None, Some(MeldKind::Ankan), Some(MeldKind::Pon), None, None],
        ),
        (
            &[
                Group::Pair(33),
                Group::Kan(27),
                Group::Triplet(28),
                Group::Triplet(29),
                Group::Triplet(30),
            ],
            &[None, Some(MeldKind::Ankan), Some(MeldKind::Pon), None, None],
        ),
        (
            &[
                Group::Pair(30),
                Group::Triplet(27),
                Group::Triplet(28),
                Group::Triplet(29),
                Group::Sequence(0),
            ],
            &[
                None,
                Some(MeldKind::Pon),
                None,
                Some(MeldKind::Pon),
                Some(MeldKind::Chi),
            ],
        ),
        (
            &[
                Group::Pair(0),
                Group::Triplet(0),
                Group::Sequence(1),
                Group::Sequence(4),
                Group::Triplet(8),
            ],
            &[None, Some(MeldKind::Pon), None, None, None],
        ),
    ];
    corpus.extend(
        cases
            .iter()
            .filter_map(|(groups, kinds)| from_groups(groups, kinds)),
    );
}

fn random_hands(corpus: &mut Vec<CompletedHandAnalysis>, seed: u64, pool: &[u8], cases: usize) {
    let mut rng = Rng(seed);
    let sequence_starts: Vec<u8> = (0..3u8)
        .flat_map(|suit| (0..7u8).map(move |number| suit * 9 + number))
        .collect();
    for _ in 0..cases {
        let mut groups = vec![Group::Pair(pool[usize::from(rng.below(pool.len() as u64))])];
        let mut fixed_kinds = vec![None];
        for _ in 0..4 {
            let group = match rng.below(10) {
                0..=3 => Group::Sequence(
                    sequence_starts[usize::from(rng.below(sequence_starts.len() as u64))],
                ),
                4..=7 => Group::Triplet(pool[usize::from(rng.below(pool.len() as u64))]),
                _ => Group::Kan(pool[usize::from(rng.below(pool.len() as u64))]),
            };
            let kind = match (group, rng.below(3)) {
                (Group::Kan(_), 0) => Some(MeldKind::Ankan),
                (Group::Kan(_), 1) => Some(MeldKind::Kakan),
                (Group::Sequence(_), 0) => Some(MeldKind::Chi),
                (_, 0) => Some(MeldKind::Pon),
                _ => None,
            };
            groups.push(group);
            fixed_kinds.push(kind);
        }
        corpus.extend(from_groups(&groups, &fixed_kinds));
    }
}

fn chiitoitsu_hands(corpus: &mut Vec<CompletedHandAnalysis>, cases: usize) {
    let mut rng = Rng(0xFEED_FACE_1234_5678);
    for _ in 0..cases {
        let mut used = [false; TileType::COUNT];
        let mut source = TileSource::new();
        let mut concealed = Vec::new();
        while concealed.len() < 14 {
            let tile = rng.below(TileType::COUNT as u64);
            if used[usize::from(tile)] {
                continue;
            }
            used[usize::from(tile)] = true;
            concealed.extend(source.take(tile));
            concealed.extend(source.take(tile));
        }
        corpus.extend(analysis(&concealed, &[]));
    }
}

/// 差分検証で比べる完成手の一覧。
pub(crate) fn analyses() -> Vec<CompletedHandAnalysis> {
    const ALL_TILES: [u8; 34] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, 33,
    ];
    const HONORS_AND_TERMINALS: [u8; 13] = [0, 8, 9, 17, 18, 26, 27, 28, 29, 30, 31, 32, 33];
    const GREEN: [u8; 6] = [19, 20, 21, 23, 25, 32];
    const MAN: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];

    let mut corpus = Vec::new();
    named_hands(&mut corpus);
    melded_hands(&mut corpus);
    random_hands(&mut corpus, 0x9E37_79B9_7F4A_7C15, &ALL_TILES, 600);
    random_hands(
        &mut corpus,
        0xDEAD_BEEF_CAFE_BABE,
        &HONORS_AND_TERMINALS,
        400,
    );
    random_hands(&mut corpus, 0x1234_5678_90AB_CDEF, &GREEN, 200);
    random_hands(&mut corpus, 0x0F1E_2D3C_4B5A_6978, &MAN, 200);
    chiitoitsu_hands(&mut corpus, 200);
    corpus
}
