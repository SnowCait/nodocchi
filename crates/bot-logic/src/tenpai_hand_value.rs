//! テンパイ手の待ちごとの手牌価値を列挙する pure layer。
//!
//! テンパイ形の待ち1つ1つについて「その牌でアガった完成手」を組み立て、既存の
//! [`evaluate_hand_value`] による評価を待ち単位で並べる。リーチ / ダマの判断、押し引き、
//! EV はここでは扱わず、待ちごとの評価結果を診断可能な形で列挙するところまでを責務にする。
//!
//! # 待ちの source of truth
//!
//! 待ち牌種と待ち全体の残枚数は既存の受け入れ ([`Acceptance`]) の値をそのまま使い、専用の待ち
//! 計算を持たない。ロン可否も既存のフリテン診断 ([`TenpaiWaitAvailability`]) の結論を持ち回る
//! だけで、ここで判定し直さない。フリテンは点数計算の入力ではないため、フリテンの待ちでも
//! HandValue を書き換えない。フリテンで変わるのはロン可否だけである。
//!
//! # 残枚数の内訳
//!
//! 待ち全体の残枚数は [`Acceptance`] のままにしつつ、その内訳を和了牌の物理牌 (variant) ごとに
//! 持つ。内訳はまだ見えていない物理牌を数えたもので、待ち全体の残枚数を見え牌から計算し直して
//! 置き換えることはしない。
//!
//! 2つは別の入力から求まるため矛盾し得る。そこで variant ごとの残枚数の合計が
//! [`Acceptance`] の残枚数と一致することを牌種ごとに検証し、一致しなければ
//! [`TenpaiHandValueError::InconsistentRemaining`] にする。どちらかへ silent に寄せて
//! 辻褄を合わせない。赤5のある牌種に限らず、全ての待ちで同じ検証を通す。
//!
//! # WinningContext
//!
//! 和了状況は caller が渡した [`WinningContext`] をそのまま全ての待ちへ渡す。ロン / ツモ・
//! 一発・槍槓・嶺上・残り山のような「アガった時点の事実」をこの layer で推測しない。
//! したがって context が exact scoring に足りなければ既存どおりエラーになり、裏ドラ未確定は
//! [`HandValueOutcome::IndeterminateBonusHan`]、役なしは [`HandValueOutcome::NoCandidate`] の
//! まま待ちごとに保持する。未来の事実を `false` や現在値で補ってまで確定値を作らない。
//!
//! # 赤5
//!
//! 赤5と黒5は同じ牌種で点数だけが違うため、待ちが 5m / 5p / 5s の場合は物理牌単位へ分けて
//! 評価する。まだ見えていない物理牌に赤5と黒5の両方が残っていれば両方を variant として
//! 評価し、「黒5として評価」のような推測をしない。赤5と黒5それぞれの残枚数も variant ごとに
//! 持つため、後段は「5s の残り3枚のうち赤が1枚・黒が2枚」を扱える。赤ドラの数え方自体は既存の
//! bonus 翻 ([`TileId::is_red`]) に任せ、ここでは物理牌の候補と枚数を分けるだけである。

use thiserror::Error;

use crate::acceptance::Acceptance;
use crate::completed_hand::{CompletedHandAnalysis, CompletedHandError, analyze_completed_hand};
use crate::furiten::{TENPAI_SHANTEN, TenpaiWaitAvailability};
use crate::hand_value::{HandValue, HandValueError, HandValueOutcome, evaluate_hand_value};
use crate::meld::Meld;
use crate::shanten::MinShanten;
use crate::tile::{TileId, TileType};
use crate::winning_context::WinningContext;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TenpaiHandValueError {
    #[error("the hand is not tenpai: min shanten {0}")]
    NotTenpai(i8),

    /// 待ちの残枚数と、まだ見えていない物理牌の枚数が食い違っている。
    ///
    /// 受け入れと見え牌が別の時点の局面を指している場合に起きる。どちらが正しいかはこの layer
    /// では決められないため、silent に補正せずそのまま報告する。
    #[error(
        "the remaining of {} disagrees with the unseen physical tiles: acceptance {acceptance}, physical {physical}",
        winning_tile.to_mjai_string()
    )]
    InconsistentRemaining {
        winning_tile: TileType,
        acceptance: u8,
        physical: u8,
    },

    #[error(transparent)]
    CompletedHand(#[from] CompletedHandError),
}

/// 待ち1牌種について、和了牌の物理牌ごとに組み立てた完成手。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningTileCompletedHand {
    winning_tile: TileId,
    remaining: u8,
    analysis: CompletedHandAnalysis,
}

impl WinningTileCompletedHand {
    pub fn winning_tile(&self) -> TileId {
        self.winning_tile
    }

    pub fn is_red(&self) -> bool {
        self.winning_tile.is_red()
    }

    /// この variant の残枚数。まだ見えていない同じ赤 / 黒の物理牌の枚数。
    pub fn remaining(&self) -> u8 {
        self.remaining
    }

    pub fn analysis(&self) -> &CompletedHandAnalysis {
        &self.analysis
    }
}

/// 待ち1牌種分の完成手。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiWaitCompletedHand {
    winning_tile: TileType,
    remaining: u8,
    winning_tiles: Vec<WinningTileCompletedHand>,
}

impl TenpaiWaitCompletedHand {
    pub fn winning_tile(&self) -> TileType {
        self.winning_tile
    }

    /// この待ち全体の残枚数。既存 [`Acceptance`] の値そのもの。
    ///
    /// [`winning_tiles`](Self::winning_tiles) の残枚数の合計と一致することは組み立て時に
    /// 検証済み。
    pub fn remaining(&self) -> u8 {
        self.remaining
    }

    /// 和了牌の物理牌ごとの完成手。赤5と黒5のどちらもあり得る場合は両方を含む。
    pub fn winning_tiles(&self) -> &[WinningTileCompletedHand] {
        &self.winning_tiles
    }
}

/// テンパイ手の待ちごとの完成手一式。
///
/// [`evaluate_tenpai_hand_value`] が借用して評価するため、完成手そのものはこの型が所有する。
/// 和了状況やドラ表示牌に依存しない部分だけを持ち、点数計算の入力は評価時に渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiCompletedHands {
    waits: Vec<TenpaiWaitCompletedHand>,
    wait_availability: Option<TenpaiWaitAvailability>,
}

impl TenpaiCompletedHands {
    pub fn waits(&self) -> &[TenpaiWaitCompletedHand] {
        &self.waits
    }

    /// 組み立てに使った既存のフリテン診断。渡されなかった場合は `None`。
    pub fn wait_availability(&self) -> Option<&TenpaiWaitAvailability> {
        self.wait_availability.as_ref()
    }
}

/// 和了牌の物理牌1つ分の評価結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinningTileHandValue<'a> {
    winning_tile: TileId,
    remaining: u8,
    outcome: Result<HandValueOutcome<'a>, HandValueError>,
}

impl<'a> WinningTileHandValue<'a> {
    pub fn winning_tile(&self) -> TileId {
        self.winning_tile
    }

    pub fn is_red(&self) -> bool {
        self.winning_tile.is_red()
    }

    /// この variant の残枚数。まだ見えていない同じ赤 / 黒の物理牌の枚数。
    ///
    /// 5m / 5p / 5s の待ちでは「残り3枚のうち赤が1枚・黒が2枚」のように、赤 / 黒別の枚数に
    /// なる。同じ待ちの variant の合計は [`TenpaiWaitHandValue::remaining`] と一致する。
    pub fn remaining(&self) -> u8 {
        self.remaining
    }

    /// 既存 [`evaluate_hand_value`] の結果そのもの。
    ///
    /// context 不足のエラー、裏ドラ未確定、役なしを畳まずにそのまま保持する。
    pub fn outcome(&self) -> Result<&HandValueOutcome<'a>, HandValueError> {
        self.outcome.as_ref().map_err(|error| *error)
    }

    /// 点数まで確定した手牌価値。確定しない場合は `None`。
    ///
    /// 確定しない理由 (context 不足 / 裏ドラ未確定 / 役なし) を区別するには
    /// [`outcome`](Self::outcome) を使う。
    pub fn known(&self) -> Option<&HandValue<'a>> {
        self.outcome.as_ref().ok().and_then(HandValueOutcome::known)
    }
}

/// 待ち1牌種分の評価結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiWaitHandValue<'a> {
    winning_tile: TileType,
    remaining: u8,
    can_ron: Option<bool>,
    winning_tiles: Vec<WinningTileHandValue<'a>>,
}

impl<'a> TenpaiWaitHandValue<'a> {
    pub fn winning_tile(&self) -> TileType {
        self.winning_tile
    }

    /// この待ち全体の残枚数。既存 [`Acceptance`] の値そのもの。
    ///
    /// 赤 / 黒別の内訳は [`winning_tiles`](Self::winning_tiles) が持ち、その合計がこの値と
    /// 一致することは組み立て時に検証済み。
    pub fn remaining(&self) -> u8 {
        self.remaining
    }

    /// 既存のフリテン診断による総合ロン可否。診断が渡されていない場合と判断できない場合は
    /// どちらも `None`。
    ///
    /// 恒常フリテンは待ち全体に効くため、全ての待ちで同じ値になる。ロン可否は点数計算の
    /// 入力ではないので、この値によって [`winning_tiles`](Self::winning_tiles) の評価は
    /// 変わらない。
    pub fn can_ron(&self) -> Option<bool> {
        self.can_ron
    }

    /// 和了牌の物理牌ごとの評価結果。赤5と黒5のどちらもあり得る場合は両方を含む。
    pub fn winning_tiles(&self) -> &[WinningTileHandValue<'a>] {
        &self.winning_tiles
    }
}

/// テンパイ手の待ちごとの手牌価値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenpaiHandValueProfile<'a> {
    waits: Vec<TenpaiWaitHandValue<'a>>,
    wait_availability: Option<&'a TenpaiWaitAvailability>,
}

impl<'a> TenpaiHandValueProfile<'a> {
    pub fn waits(&self) -> &[TenpaiWaitHandValue<'a>] {
        &self.waits
    }

    pub fn wait(&self, winning_tile: TileType) -> Option<&TenpaiWaitHandValue<'a>> {
        self.waits
            .iter()
            .find(|wait| wait.winning_tile == winning_tile)
    }

    /// 評価に使った既存のフリテン診断。渡されなかった場合は `None`。
    pub fn wait_availability(&self) -> Option<&'a TenpaiWaitAvailability> {
        self.wait_availability
    }
}

/// テンパイ手の待ちごとに、その牌でアガった完成手を組み立てる。
///
/// `acceptance` は待ち牌種と残枚数の source of truth で、テンパイ判定もその最小向聴数で行う。
/// 門前形・副露形のどちらの受け入れでも同じ helper を使う。テンパイ形でなければ待ちが定まらない
/// ため [`TenpaiHandValueError::NotTenpai`] になる。
///
/// `wait_availability` は同じ受け入れから求めた既存のフリテン診断。ロン可否を持ち回るためだけに
/// 使い、フリテンをここで判定し直さない。渡さない場合、ロン可否は unknown のままになる。
///
/// `visible_tiles` は手牌以外に物理牌が判明している牌 (自分の河・副露・ドラ表示牌など)。和了牌の
/// 物理牌と、その赤 / 黒別の残枚数を求めるために使う。待ち全体の残枚数はあくまで `acceptance` の
/// 値で、見え牌から計算し直して置き換えない。自分の手牌が含まれていてもよい。
///
/// そのため `acceptance` を求めたときと同じ見え牌を渡すこと。牌種ごとに variant の残枚数の合計と
/// `acceptance` の残枚数を突き合わせ、食い違えば
/// [`TenpaiHandValueError::InconsistentRemaining`] になる。
pub fn tenpai_completed_hands<S: MinShanten>(
    concealed_tiles: &[TileId],
    fixed_melds: &[Meld],
    acceptance: &Acceptance<S>,
    wait_availability: Option<&TenpaiWaitAvailability>,
    visible_tiles: &[TileId],
) -> Result<TenpaiCompletedHands, TenpaiHandValueError> {
    let min_shanten = acceptance.current_min_shanten();
    if min_shanten != TENPAI_SHANTEN {
        return Err(TenpaiHandValueError::NotTenpai(min_shanten));
    }

    let seen = seen_tiles(concealed_tiles, fixed_melds, visible_tiles);
    let waits = acceptance
        .tiles
        .iter()
        .map(|wait| {
            let variants: Vec<WinningTileVariant> =
                winning_tile_variants(wait.tile, &seen).collect();
            let physical: u8 = variants.iter().map(|variant| variant.remaining).sum();
            if physical != wait.remaining {
                return Err(TenpaiHandValueError::InconsistentRemaining {
                    winning_tile: wait.tile,
                    acceptance: wait.remaining,
                    physical,
                });
            }

            let winning_tiles = variants
                .into_iter()
                .map(|variant| {
                    completed_hand(concealed_tiles, fixed_melds, variant.winning_tile).map(
                        |analysis| WinningTileCompletedHand {
                            winning_tile: variant.winning_tile,
                            remaining: variant.remaining,
                            analysis,
                        },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok(TenpaiWaitCompletedHand {
                winning_tile: wait.tile,
                remaining: wait.remaining,
                winning_tiles,
            })
        })
        .collect::<Result<Vec<_>, TenpaiHandValueError>>()?;

    Ok(TenpaiCompletedHands {
        waits,
        wait_availability: wait_availability.cloned(),
    })
}

/// 待ちごとの完成手を、caller が渡した和了状況とドラでそのまま評価する。
///
/// 和了状況の推測も、待ちごとの context の作り分けもしない。全ての待ちへ同じ `context` を渡す。
pub fn evaluate_tenpai_hand_value<'a>(
    hands: &'a TenpaiCompletedHands,
    context: WinningContext,
    dora_indicators: &[TileId],
    ura_dora_indicators: Option<&[TileId]>,
) -> TenpaiHandValueProfile<'a> {
    let can_ron = hands
        .wait_availability
        .as_ref()
        .and_then(TenpaiWaitAvailability::can_ron);

    TenpaiHandValueProfile {
        waits: hands
            .waits
            .iter()
            .map(|wait| TenpaiWaitHandValue {
                winning_tile: wait.winning_tile,
                remaining: wait.remaining,
                can_ron,
                winning_tiles: wait
                    .winning_tiles
                    .iter()
                    .map(|completed| WinningTileHandValue {
                        winning_tile: completed.winning_tile,
                        remaining: completed.remaining,
                        outcome: evaluate_hand_value(
                            &completed.analysis,
                            context,
                            completed.winning_tile.tile_type(),
                            dora_indicators,
                            ura_dora_indicators,
                        ),
                    })
                    .collect(),
            })
            .collect(),
        wait_availability: hands.wait_availability.as_ref(),
    }
}

fn completed_hand(
    concealed_tiles: &[TileId],
    fixed_melds: &[Meld],
    winning_tile: TileId,
) -> Result<CompletedHandAnalysis, CompletedHandError> {
    let mut tiles = Vec::with_capacity(concealed_tiles.len() + 1);
    tiles.extend_from_slice(concealed_tiles);
    tiles.push(winning_tile);
    analyze_completed_hand(&tiles, fixed_melds)
}

/// 和了牌として評価すべき物理牌と、その残枚数。
struct WinningTileVariant {
    winning_tile: TileId,
    remaining: u8,
}

/// 和了牌として評価すべき物理牌を、まだ見えていない同種牌から求める。
///
/// 点数が変わるのは赤5かどうかだけなので、赤5と黒5をそれぞれ1つの variant にまとめ、代表牌と
/// 残枚数を返す。同じ赤 / 黒の物理牌はどれを選んでも評価が同じため、代表牌の copy は評価に
/// 影響しない。赤5と黒5の両方があり得る場合は両方を返し、どちらか一方だと推測しない。
///
/// 赤5の無い牌種では黒の variant 1つだけになり、その残枚数がその牌種のまだ見えていない枚数に
/// なる。したがって赤5の有無にかかわらず、同じ数え方で受け入れとの整合性を確認できる。
fn winning_tile_variants(
    winning_tile: TileType,
    seen: &[bool; TileId::COUNT],
) -> impl Iterator<Item = WinningTileVariant> {
    let variant = |red: bool| {
        let mut unseen =
            TileId::copies(winning_tile).filter(|tile| tile.is_red() == red && !seen[tile.index()]);
        let winning_tile = unseen.next()?;
        Some(WinningTileVariant {
            winning_tile,
            remaining: 1 + unseen.count() as u8,
        })
    };

    [variant(true), variant(false)].into_iter().flatten()
}

fn seen_tiles(
    concealed_tiles: &[TileId],
    fixed_melds: &[Meld],
    visible_tiles: &[TileId],
) -> [bool; TileId::COUNT] {
    let mut seen = [false; TileId::COUNT];
    for tile in concealed_tiles
        .iter()
        .chain(fixed_melds.iter().flat_map(|meld| meld.tiles()))
        .chain(visible_tiles)
    {
        seen[tile.index()] = true;
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::acceptance::{
        calculate_acceptance_with_fixed_melds,
        calculate_acceptance_with_fixed_melds_and_visible_tiles,
        calculate_acceptance_with_visible_tiles, structural_acceptance_tile_types_with_fixed_melds,
    };
    use crate::completed_hand::CompletedHandDecomposition;
    use crate::furiten::{
        HistoryFuritenFacts, OwnDiscards, PermanentFuriten, tenpai_wait_availability,
    };
    use crate::meld::MeldKind;
    use crate::normal_hand_scoring::{MissingScoringFact, NormalScoringError};
    use crate::payment::Payment;
    use crate::shanten::FixedMeldCount;
    use crate::tile_counts::TileCounts;
    use crate::winning_context::{RiichiStatus, WinMethod};
    use crate::winning_tile::WaitType;
    use crate::yakuman::Yakuman;

    struct TileIdSource {
        used: [bool; TileId::COUNT],
    }

    impl TileIdSource {
        fn new() -> Self {
            Self {
                used: [false; TileId::COUNT],
            }
        }

        fn tiles(&mut self, strings: &[&str]) -> Vec<TileId> {
            strings.iter().map(|s| self.tile(s)).collect()
        }

        fn meld(&mut self, kind: MeldKind, strings: &[&str]) -> Meld {
            let tiles = self.tiles(strings);
            let called_tile = kind.is_open().then(|| tiles[0]);
            Meld::new(kind, tiles, called_tile)
        }

        fn tile(&mut self, s: &str) -> TileId {
            let tile_type = tile_type(s);
            let red = s.ends_with('r');
            let id = TileId::copies(tile_type)
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .unwrap();
            self.used[id.index()] = true;
            id
        }
    }

    fn tile_type(s: &str) -> TileType {
        TileType::from_mjai_type_str(s.trim_end_matches('r')).unwrap()
    }

    struct Tenpai {
        concealed_tiles: Vec<TileId>,
        fixed_melds: Vec<Meld>,
        source: TileIdSource,
        other_visible_tiles: Vec<TileId>,
        acceptance_sees_them: bool,
    }

    impl Tenpai {
        fn new(concealed: &[&str], fixed: &[(MeldKind, &[&str])]) -> Self {
            let mut source = TileIdSource::new();
            let fixed_melds: Vec<Meld> = fixed
                .iter()
                .map(|(kind, tiles)| source.meld(*kind, tiles))
                .collect();
            let concealed_tiles = source.tiles(concealed);
            Self {
                concealed_tiles,
                fixed_melds,
                source,
                other_visible_tiles: Vec::new(),
                acceptance_sees_them: true,
            }
        }

        /// 手牌以外の見え牌を、受け入れと layer の両方へ渡す。
        fn with_visible(mut self, visible: &[&str]) -> Self {
            self.other_visible_tiles = self.source.tiles(visible);
            self
        }

        /// 手牌以外の見え牌を layer だけへ渡し、受け入れは見え牌を知らないままにする。
        fn with_visible_unknown_to_the_acceptance(mut self, visible: &[&str]) -> Self {
            self.acceptance_sees_them = false;
            self.with_visible(visible)
        }

        /// 自分の手牌を含む見え牌。受け入れの見え牌は手牌を含む前提のため、同じ形で渡す。
        fn visible_tiles(&self) -> Vec<TileId> {
            self.concealed_tiles
                .iter()
                .chain(self.fixed_melds.iter().flat_map(|meld| meld.tiles()))
                .chain(&self.other_visible_tiles)
                .copied()
                .collect()
        }

        fn indicators(&mut self, strings: &[&str]) -> Vec<TileId> {
            self.source.tiles(strings)
        }

        fn counts(&self) -> TileCounts {
            TileCounts::from_tiles(self.concealed_tiles.iter().copied())
        }

        /// 門前形で layer へ渡すのと同じ受け入れ。
        fn acceptance(&self) -> Acceptance {
            calculate_acceptance_with_visible_tiles(&self.counts(), &self.visible_tiles())
        }

        fn fixed_meld_count(&self) -> FixedMeldCount {
            FixedMeldCount::new(self.fixed_melds.len() as u8).unwrap()
        }

        fn hands(&self) -> TenpaiCompletedHands {
            self.completed_hands(None).unwrap()
        }

        fn completed_hands(
            &self,
            wait_availability: Option<&TenpaiWaitAvailability>,
        ) -> Result<TenpaiCompletedHands, TenpaiHandValueError> {
            let counts = self.counts();
            let visible_tiles = self.visible_tiles();
            let acceptance_visible_tiles: &[TileId] = if self.acceptance_sees_them {
                &visible_tiles
            } else {
                &self.concealed_tiles
            };

            // 門前形は Shanten、副露形は EffectiveShanten の受け入れを渡す。
            if self.fixed_melds.is_empty() {
                tenpai_completed_hands(
                    &self.concealed_tiles,
                    &self.fixed_melds,
                    &calculate_acceptance_with_visible_tiles(&counts, acceptance_visible_tiles),
                    wait_availability,
                    &visible_tiles,
                )
            } else {
                tenpai_completed_hands(
                    &self.concealed_tiles,
                    &self.fixed_melds,
                    &calculate_acceptance_with_fixed_melds_and_visible_tiles(
                        &counts,
                        self.fixed_meld_count(),
                        acceptance_visible_tiles,
                    ),
                    wait_availability,
                    &visible_tiles,
                )
            }
        }

        fn availability(&self, own_discards: &OwnDiscards) -> TenpaiWaitAvailability {
            let counts = self.counts();
            let acceptance =
                calculate_acceptance_with_fixed_melds(&counts, self.fixed_meld_count());
            tenpai_wait_availability(
                &acceptance,
                &structural_acceptance_tile_types_with_fixed_melds(
                    &counts,
                    self.fixed_meld_count(),
                ),
                own_discards,
                HistoryFuritenFacts {
                    same_turn: Some(false),
                    riichi_missed_win: Some(false),
                },
            )
            .unwrap()
        }
    }

    fn profile<'a>(
        hands: &'a TenpaiCompletedHands,
        context: WinningContext,
    ) -> TenpaiHandValueProfile<'a> {
        evaluate_tenpai_hand_value(hands, context, &[], None)
    }

    fn known_context(win_method: WinMethod) -> WinningContext {
        WinningContext::new(win_method)
            .with_round_wind(Some(tile_type("E")))
            .with_seat_wind(Some(tile_type("S")))
            .with_riichi(RiichiStatus::NotDeclared)
            .with_chankan(Some(false))
            .with_rinshan(Some(false))
            .with_remaining_live_tiles(Some(1))
    }

    fn ron() -> WinningContext {
        known_context(WinMethod::Ron)
    }

    fn tsumo() -> WinningContext {
        known_context(WinMethod::Tsumo)
    }

    fn riichi(context: WinningContext) -> WinningContext {
        context
            .with_riichi(RiichiStatus::Riichi)
            .with_ippatsu(Some(false))
    }

    // 赤5 variant は専用の test で確認するため、ここでは黒牌の variant だけを見る。
    fn payment_of(wait: &TenpaiWaitHandValue<'_>) -> Option<u32> {
        black_variant(wait)
            .known()
            .and_then(HandValue::payment)
            .map(Payment::total)
    }

    fn black_variant<'a, 'h>(wait: &'a TenpaiWaitHandValue<'h>) -> &'a WinningTileHandValue<'h> {
        wait.winning_tiles()
            .iter()
            .find(|winning_tile| !winning_tile.is_red())
            .unwrap()
    }

    fn payments(profile: &TenpaiHandValueProfile<'_>) -> Vec<(TileType, Option<u32>)> {
        profile
            .waits()
            .iter()
            .map(|wait| (wait.winning_tile(), payment_of(wait)))
            .collect()
    }

    fn remaining_by_redness(wait: &TenpaiWaitHandValue<'_>) -> Vec<(bool, u8)> {
        wait.winning_tiles()
            .iter()
            .map(|winning_tile| (winning_tile.is_red(), winning_tile.remaining()))
            .collect()
    }

    fn payment_by_redness(wait: &TenpaiWaitHandValue<'_>) -> Vec<(bool, Option<u32>)> {
        wait.winning_tiles()
            .iter()
            .map(|winning_tile| {
                (
                    winning_tile.is_red(),
                    winning_tile
                        .known()
                        .and_then(HandValue::payment)
                        .map(Payment::total),
                )
            })
            .collect()
    }

    // 両面待ち。2s で三色同順が付き、5s では付かない。
    const RYANMEN_SANSHOKU: [&str; 13] = [
        "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "3s", "4s", "5s", "5s",
    ];
    // 延べ単騎。5s でも 8s でも単騎になる。
    const NOBETAN: [&str; 13] = [
        "1m", "1m", "1m", "2p", "3p", "4p", "6p", "7p", "8p", "5s", "6s", "7s", "8s",
    ];
    // 5s の単騎待ち。5s は手牌に1枚だけなので、残り3枚の内訳は赤1枚 + 黒2枚になる。
    const TANKI_FIVE: [&str; 13] = [
        "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "6p", "7p", "8p", "5s",
    ];
    // 国士無双十三面待ち。
    const KOKUSHI_THIRTEEN: [&str; 13] = [
        "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
    ];
    // 緑一色 + 四暗刻の単騎待ち。和了形が複数の decomposition を持つ。
    const GREEN_TANKI: [&str; 13] = [
        "2s", "2s", "2s", "3s", "3s", "3s", "4s", "4s", "4s", "6s", "8s", "8s", "8s",
    ];

    // ---- 待ちごとの列挙 ----

    #[test]
    fn every_wait_of_a_ryanmen_hand_is_listed_with_its_own_hand_value() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        // 2s: 断么九 + 平和 + 三色同順、5s: 断么九 + 平和。
        assert_eq!(
            payments(&profile),
            vec![(tile_type("2s"), Some(7700)), (tile_type("5s"), Some(2000)),]
        );
    }

    #[test]
    fn a_nobetan_hand_lists_both_tanki_waits() {
        let tenpai = Tenpai::new(&NOBETAN, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, tsumo());

        // 5s / 8s のどちらも門前清自摸和だけの 1 翻 40 符の単騎待ち。
        assert_eq!(
            payments(&profile),
            vec![(tile_type("5s"), Some(1500)), (tile_type("8s"), Some(1500))]
        );
        for wait in profile.waits() {
            assert_eq!(
                black_variant(wait)
                    .known()
                    .unwrap()
                    .normal()
                    .unwrap()
                    .interpretation()
                    .wait(),
                WaitType::Tanki
            );
        }
    }

    #[test]
    fn the_wait_tile_types_and_remaining_come_from_the_existing_acceptance() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let acceptance = tenpai.acceptance();
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        assert_eq!(
            profile
                .waits()
                .iter()
                .map(|wait| (wait.winning_tile(), wait.remaining()))
                .collect::<Vec<_>>(),
            acceptance
                .tiles
                .iter()
                .map(|tile| (tile.tile, tile.remaining))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            profile
                .waits()
                .iter()
                .map(|wait| u32::from(wait.remaining()))
                .sum::<u32>(),
            u32::from(acceptance.total_remaining())
        );
    }

    #[test]
    fn a_narrowed_wait_still_takes_its_remaining_from_the_acceptance() {
        // 見え牌で残枚数が減る場合も、待ち全体の残枚数は受け入れの値をそのまま使う。
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]).with_visible(&["2s", "2s"]);
        let acceptance = tenpai.acceptance();
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("2s")).unwrap();

        assert_eq!(
            wait.remaining(),
            acceptance
                .tiles
                .iter()
                .find(|tile| tile.tile == tile_type("2s"))
                .unwrap()
                .remaining
        );
        assert_eq!(remaining_by_redness(wait), vec![(false, 2)]);
    }

    #[test]
    fn an_open_hand_uses_the_existing_melded_acceptance() {
        let tenpai = Tenpai::new(
            &["2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5s"],
            &[(MeldKind::Chi, &["6s", "7s", "8s"])],
        );
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        // 副露形の単騎待ちで、断么九だけの 1 翻 30 符。
        assert_eq!(payments(&profile), vec![(tile_type("5s"), Some(1000))]);
    }

    #[test]
    fn a_hand_that_is_not_tenpai_has_no_waits_to_evaluate() {
        let tenpai = Tenpai::new(
            &[
                "1m", "2m", "3m", "5m", "7m", "9m", "1p", "4p", "7p", "2s", "5s", "8s", "E",
            ],
            &[],
        );

        assert!(matches!(
            tenpai.completed_hands(None),
            Err(TenpaiHandValueError::NotTenpai(shanten)) if shanten > TENPAI_SHANTEN
        ));
    }

    // ---- 役満と複数 decomposition ----

    #[test]
    fn every_wait_of_a_thirteen_wait_kokushi_is_a_named_yakuman() {
        let tenpai = Tenpai::new(&KOKUSHI_THIRTEEN, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        assert_eq!(profile.waits().len(), 13);
        for wait in profile.waits() {
            assert_eq!(wait.remaining(), 3);
            let [winning_tile] = wait.winning_tiles() else {
                panic!("unexpected variants");
            };
            let hand_value = winning_tile.known().unwrap();
            assert!(hand_value.is_yakuman());
            assert_eq!(
                hand_value
                    .yakuman()
                    .unwrap()
                    .multiplier_of(Yakuman::KokushiMusou),
                Some(2)
            );
            assert_eq!(
                hand_value.payment().map(|payment| payment.total()),
                Some(64000)
            );
        }
    }

    #[test]
    fn a_wait_whose_completed_hand_has_several_decompositions_keeps_the_existing_selection() {
        let tenpai = Tenpai::new(&GREEN_TANKI, &[]);
        let hands = tenpai.hands();
        let wait = hands
            .waits()
            .iter()
            .find(|wait| wait.winning_tile() == tile_type("6s"))
            .unwrap();
        let [completed] = wait.winning_tiles() else {
            panic!("unexpected variants");
        };

        // 222s 333s 444s の刻子解釈と 234s 234s 234s の順子解釈が両方ある。
        let has_sequence: Vec<bool> = completed
            .analysis()
            .decompositions()
            .iter()
            .filter_map(CompletedHandDecomposition::as_standard)
            .map(|decomposition| {
                decomposition
                    .concealed_melds()
                    .iter()
                    .any(|meld| meld.is_sequence())
            })
            .collect();
        assert!(has_sequence.contains(&true) && has_sequence.contains(&false));

        let profile = profile(&hands, ron());
        let hand_value = profile
            .wait(tile_type("6s"))
            .unwrap()
            .winning_tiles()
            .first()
            .unwrap()
            .known()
            .unwrap();

        // 既存の候補選択どおり、緑一色 + 四暗刻単騎の 3 倍役満が選ばれる。
        assert_eq!(hand_value.yakuman().unwrap().total_multiplier(), 3);
        assert_eq!(
            hand_value.payment().map(|payment| payment.total()),
            Some(96000)
        );
        assert_eq!(
            evaluate_hand_value(completed.analysis(), ron(), tile_type("6s"), &[], None),
            Ok(HandValueOutcome::Known(hand_value.clone()))
        );
    }

    #[test]
    fn waits_of_one_hand_can_mix_a_named_yakuman_and_a_normal_hand() {
        let tenpai = Tenpai::new(&GREEN_TANKI, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        // 6s は緑一色 + 四暗刻単騎、7s は 678s ができるので緑一色にも四暗刻にもならない。
        let green = payment_of(profile.wait(tile_type("6s")).unwrap());
        let not_green = black_variant(profile.wait(tile_type("7s")).unwrap())
            .known()
            .unwrap();

        assert_eq!(green, Some(96000));
        assert!(!not_green.is_yakuman());
        // 断么九 + 三暗刻 + 清一色の 9 翻。
        assert_eq!(not_green.payment().map(Payment::total), Some(16000));
    }

    // ---- 赤5 ----

    #[test]
    fn a_five_wait_evaluates_the_red_and_the_black_five_separately() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("5s")).unwrap();

        // 手牌の 5s は黒2枚なので、赤5s も黒5s もアガリ牌になり得る。どちらか一方だと推測しない。
        assert_eq!(
            payment_by_redness(wait),
            vec![(true, Some(3900)), (false, Some(2000))]
        );
        assert_eq!(remaining_by_redness(wait), vec![(true, 1), (false, 1)]);
    }

    #[test]
    fn a_five_wait_without_a_remaining_red_five_has_only_the_black_variant() {
        // 赤5s を手牌に持っていれば、残りの 5s は黒だけだと物理牌から確定できる。
        let concealed: Vec<&str> = RYANMEN_SANSHOKU
            .iter()
            .enumerate()
            .map(|(index, tile)| if index == 12 { "5sr" } else { *tile })
            .collect();
        let tenpai = Tenpai::new(&concealed, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("5s")).unwrap();

        // 手牌の赤5s の 1 翻が乗った黒5s 和了だけになる。
        assert_eq!(payment_by_redness(wait), vec![(false, Some(3900))]);
        assert_eq!(remaining_by_redness(wait), vec![(false, 2)]);
    }

    #[test]
    fn seen_tiles_can_rule_out_the_red_five_variant() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]).with_visible(&["5sr"]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("5s")).unwrap();

        assert_eq!(payment_by_redness(wait), vec![(false, Some(2000))]);
        assert_eq!(wait.remaining(), 1);
        assert_eq!(remaining_by_redness(wait), vec![(false, 1)]);
    }

    // ---- 残枚数の内訳と整合性 ----

    #[test]
    fn each_winning_tile_variant_keeps_its_own_remaining() {
        let tenpai = Tenpai::new(&TANKI_FIVE, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("5s")).unwrap();

        // 5s の残り3枚の内訳は赤5s が1枚・黒5s が2枚。
        assert_eq!(wait.remaining(), 3);
        assert_eq!(remaining_by_redness(wait), vec![(true, 1), (false, 2)]);
    }

    #[test]
    fn the_variant_remaining_adds_up_to_the_acceptance_remaining() {
        let tenpai = Tenpai::new(&TANKI_FIVE, &[]);
        let acceptance = tenpai.acceptance();
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        for wait in profile.waits() {
            let variants: u8 = wait
                .winning_tiles()
                .iter()
                .map(WinningTileHandValue::remaining)
                .sum();
            assert_eq!(variants, wait.remaining());
            assert_eq!(
                wait.remaining(),
                acceptance
                    .tiles
                    .iter()
                    .find(|tile| tile.tile == wait.winning_tile())
                    .unwrap()
                    .remaining
            );
        }
    }

    #[test]
    fn a_seen_red_five_leaves_only_the_black_variant_with_its_remaining() {
        let tenpai = Tenpai::new(&TANKI_FIVE, &[]).with_visible(&["5sr"]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("5s")).unwrap();

        assert_eq!(wait.remaining(), 2);
        assert_eq!(remaining_by_redness(wait), vec![(false, 2)]);
    }

    #[test]
    fn a_remaining_that_disagrees_with_the_unseen_physical_tiles_is_an_error() {
        // 受け入れは赤5s を見ていないのに、layer だけが見えている状態。silent に寄せない。
        let tenpai = Tenpai::new(&TANKI_FIVE, &[]).with_visible_unknown_to_the_acceptance(&["5sr"]);

        assert_eq!(
            tenpai.completed_hands(None),
            Err(TenpaiHandValueError::InconsistentRemaining {
                winning_tile: tile_type("5s"),
                acceptance: 3,
                physical: 2,
            })
        );
    }

    #[test]
    fn an_inconsistent_remaining_is_detected_for_a_tile_without_a_red_five() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[])
            .with_visible_unknown_to_the_acceptance(&["2s", "2s"]);

        assert_eq!(
            tenpai.completed_hands(None),
            Err(TenpaiHandValueError::InconsistentRemaining {
                winning_tile: tile_type("2s"),
                acceptance: 4,
                physical: 2,
            })
        );
    }

    #[test]
    fn consistent_visible_tiles_change_the_remaining_but_not_the_hand_value() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let narrowed = Tenpai::new(&RYANMEN_SANSHOKU, &[]).with_visible(&["2s"]);
        let hands = tenpai.hands();
        let narrowed_hands = narrowed.hands();
        let full = profile(&hands, ron());
        let narrowed_profile = profile(&narrowed_hands, ron());

        assert_eq!(full.wait(tile_type("2s")).unwrap().remaining(), 4);
        assert_eq!(
            narrowed_profile.wait(tile_type("2s")).unwrap().remaining(),
            3
        );
        assert_eq!(payments(&narrowed_profile), payments(&full));
    }

    #[test]
    fn a_wait_that_cannot_be_a_red_five_has_a_single_variant() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());
        let wait = profile.wait(tile_type("2s")).unwrap();

        assert_eq!(wait.winning_tiles().len(), 1);
        assert!(!wait.winning_tiles()[0].is_red());
    }

    // ---- WinningContext ----

    #[test]
    fn the_given_win_method_is_used_as_it_is() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();

        assert_eq!(
            payment_of(profile(&hands, ron()).wait(tile_type("2s")).unwrap()),
            Some(7700)
        );
        assert_eq!(
            payment_of(profile(&hands, tsumo()).wait(tile_type("2s")).unwrap()),
            Some(8000)
        );
    }

    #[test]
    fn the_given_situational_facts_are_used_as_they_are() {
        // 海底や一発のような和了時点の事実を、この layer で推測も上書きもしない。
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let haitei = tsumo().with_remaining_live_tiles(Some(0));

        assert_eq!(
            payment_of(profile(&hands, haitei).wait(tile_type("2s")).unwrap()),
            Some(12000)
        );
    }

    #[test]
    fn an_incomplete_context_keeps_the_existing_scoring_error() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron().with_round_wind(None));

        for wait in profile.waits() {
            for winning_tile in wait.winning_tiles() {
                assert_eq!(
                    winning_tile.outcome(),
                    Err(HandValueError::NormalScoring(
                        NormalScoringError::IncompleteContext(MissingScoringFact::RoundWind)
                    ))
                );
                assert!(winning_tile.known().is_none());
            }
        }
    }

    #[test]
    fn a_named_yakuman_keeps_being_known_under_an_incomplete_context() {
        // 既存どおり、named 役満は通常手の exact context を必要としない。
        let tenpai = Tenpai::new(&KOKUSHI_THIRTEEN, &[]);
        let hands = tenpai.hands();
        let context = WinningContext::new(WinMethod::Ron).with_seat_wind(Some(tile_type("S")));
        let profile = profile(&hands, context);

        for wait in profile.waits() {
            assert!(wait.winning_tiles()[0].known().unwrap().is_yakuman());
        }
    }

    #[test]
    fn an_unknown_ura_dora_stays_indeterminate_per_wait() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = evaluate_tenpai_hand_value(&hands, riichi(ron()), &[], None);

        for wait in profile.waits() {
            for winning_tile in wait.winning_tiles() {
                assert_eq!(
                    winning_tile.outcome(),
                    Ok(&HandValueOutcome::IndeterminateBonusHan)
                );
                assert!(winning_tile.known().is_none());
            }
        }
    }

    #[test]
    fn an_observed_empty_ura_dora_is_not_indeterminate() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = evaluate_tenpai_hand_value(&hands, riichi(ron()), &[], Some(&[]));

        // リーチの 1 翻が乗る。
        assert_eq!(
            payment_of(profile.wait(tile_type("2s")).unwrap()),
            Some(8000)
        );
    }

    #[test]
    fn the_dora_indicators_reach_every_wait() {
        let mut tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let dora_indicators = tenpai.indicators(&["1m"]);
        let hands = tenpai.hands();
        let profile = evaluate_tenpai_hand_value(&hands, ron(), &dora_indicators, None);

        // 2m のドラ 1 翻がどちらの待ちにも乗る。
        assert_eq!(
            payments(&profile),
            vec![(tile_type("2s"), Some(8000)), (tile_type("5s"), Some(3900))]
        );
    }

    #[test]
    fn a_wait_without_any_yaku_has_no_candidate() {
        let tenpai = Tenpai::new(
            &["4p", "5p", "6p", "2s", "3s", "4s", "7s", "8s", "9s", "5s"],
            &[(MeldKind::Chi, &["1m", "2m", "3m"])],
        );
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        // 赤5s でアガっても赤ドラは役にならないため、どの和了牌でも候補が無い。
        for wait in profile.waits() {
            for winning_tile in wait.winning_tiles() {
                assert_eq!(winning_tile.outcome(), Ok(&HandValueOutcome::NoCandidate));
            }
        }
    }

    // ---- フリテン ----

    #[test]
    fn the_existing_furiten_diagnostic_is_carried_per_wait() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let availability =
            tenpai.availability(&OwnDiscards::from_river_tile_types([tile_type("2s")]));
        let hands = tenpai.completed_hands(Some(&availability)).unwrap();
        let profile = profile(&hands, ron());

        assert_eq!(
            profile
                .wait_availability()
                .map(TenpaiWaitAvailability::permanent_furiten),
            Some(PermanentFuriten::Yes)
        );
        for wait in profile.waits() {
            assert_eq!(wait.can_ron(), Some(false));
        }
    }

    #[test]
    fn furiten_does_not_change_any_hand_value() {
        // フリテンで変わるのはロン可否だけで、点数計算の入力ではない。
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let furiten = tenpai.availability(&OwnDiscards::from_river_tile_types([tile_type("2s")]));
        let not_furiten =
            tenpai.availability(&OwnDiscards::from_river_tile_types([tile_type("1p")]));
        let furiten_hands = tenpai.completed_hands(Some(&furiten)).unwrap();
        let not_furiten_hands = tenpai.completed_hands(Some(&not_furiten)).unwrap();

        assert_eq!(
            profile(&furiten_hands, ron())
                .wait(tile_type("2s"))
                .unwrap()
                .can_ron(),
            Some(false)
        );
        assert_eq!(
            profile(&not_furiten_hands, ron())
                .wait(tile_type("2s"))
                .unwrap()
                .can_ron(),
            Some(true)
        );
        assert_eq!(
            payments(&profile(&furiten_hands, ron())),
            payments(&profile(&not_furiten_hands, ron()))
        );
        assert_eq!(
            payments(&profile(&furiten_hands, ron())),
            payments(&profile(&tenpai.hands(), ron()))
        );
    }

    #[test]
    fn a_missing_furiten_diagnostic_keeps_the_ron_availability_unknown() {
        let tenpai = Tenpai::new(&RYANMEN_SANSHOKU, &[]);
        let hands = tenpai.hands();
        let profile = profile(&hands, ron());

        assert!(profile.wait_availability().is_none());
        for wait in profile.waits() {
            assert_eq!(wait.can_ron(), None);
        }
    }
}
