use bot_logic::{FixedMeldCount, HistoryFuritenFacts, TileId, TileType};

use crate::meld::{Meld, fixed_meld_count};

/// 指定 player の自風を親から導出する pure helper。4人麻雀の席順だけを前提にする。
///
/// `seat_index = (player + 4 - oya) % 4` で、`oya` 自身は東家。範囲外の `player` / `oya` は
/// 推測で補完せず `None`。`GameContext::seat_wind()` は自分の自風なので相手の判定には使わない。
pub fn seat_wind_for_player(player: usize, oya: u8) -> Option<TileType> {
    if player >= 4 || oya >= 4 {
        return None;
    }
    let seat_index = (player as u8 + 4 - oya) % 4;
    TileType::wind_from_seat_index(seat_index)
}

/// 局面まわりの table state を表す観測事実。
///
/// すべて optional で、unknown (`None`) と観測できた 0 (`Some(0)`) を区別する。取得できない
/// 入力経路では `None` のままにし、`0` や 25000 点などの初期値で補完しない。
///
/// 単位は field ごとに固定する。client 固有の単位はここへ入れる前に変換する。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TableStateFacts {
    /// 山の残りツモ可能枚数 [枚]。
    pub remaining_tiles: Option<u32>,
    /// 本場 [本]。
    pub honba: Option<u32>,
    /// 供託 [点]。リーチ棒の本数ではない。
    pub kyotaku_points: Option<u32>,
    /// 各 player の現在持ち点 [点]。index は player id で、席順に並ぶ。
    pub scores: Option<[i32; 4]>,
    /// 場風の中で何局目か [局]。東1 / 南1 を `1` とする 1-based で、場風は `round_wind` が持つ。
    pub kyoku: Option<u8>,
}

impl TableStateFacts {
    /// 指定 player の現在持ち点。点棒が unknown な場合や範囲外の `player` では `None`。
    pub fn score_of(&self, player: usize) -> Option<i32> {
        self.scores?.get(player).copied()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameContext {
    drawn_tile: Option<TileId>,
    hand_tiles: Vec<TileId>,
    dora_indicators: Vec<TileId>,
    round_wind: Option<TileType>,
    seat_wind: Option<TileType>,
    visible_tiles: Vec<TileId>,
    player_id: Option<u8>,
    oya: Option<u8>,
    discards: [Vec<TileId>; 4],
    reached: [bool; 4],
    melds: [Vec<Meld>; 4],
    post_reach_passed_tiles: [Vec<TileType>; 4],
    temporary_passed_tiles: Option<[Vec<TileType>; 4]>,
    same_hand_passed_tiles: Option<[Vec<TileType>; 4]>,
    table_state: TableStateFacts,
    history_furiten: HistoryFuritenFacts,
}

impl GameContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_drawn_tile(drawn_tile: TileId) -> Self {
        Self {
            drawn_tile: Some(drawn_tile),
            ..Self::default()
        }
    }

    pub fn with_hand_tiles(hand_tiles: Vec<TileId>) -> Self {
        Self {
            hand_tiles,
            ..Self::default()
        }
    }

    pub fn from_parts(drawn_tile: Option<TileId>, hand_tiles: Vec<TileId>) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            ..Self::default()
        }
    }

    pub fn from_parts_with_dora(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
    ) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            dora_indicators,
            ..Self::default()
        }
    }

    pub fn from_parts_with_context(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
    ) -> Self {
        Self::from_parts_with_visible_tiles(
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            Vec::new(),
        )
    }

    pub fn from_parts_with_visible_tiles(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible_tiles: Vec<TileId>,
    ) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            visible_tiles,
            ..Self::default()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_parts_with_table_state(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible_tiles: Vec<TileId>,
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> Self {
        Self::from_parts_with_melds(
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            visible_tiles,
            player_id,
            oya,
            discards,
            reached,
            Default::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_parts_with_melds(
        drawn_tile: Option<TileId>,
        hand_tiles: Vec<TileId>,
        dora_indicators: Vec<TileId>,
        round_wind: Option<TileType>,
        seat_wind: Option<TileType>,
        visible_tiles: Vec<TileId>,
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
        melds: [Vec<Meld>; 4],
    ) -> Self {
        Self {
            drawn_tile,
            hand_tiles,
            dora_indicators,
            round_wind,
            seat_wind,
            visible_tiles,
            player_id,
            oya,
            discards,
            reached,
            melds,
            ..Self::default()
        }
    }

    pub fn with_post_reach_passed_tiles(
        mut self,
        post_reach_passed_tiles: [Vec<TileType>; 4],
    ) -> Self {
        self.post_reach_passed_tiles = post_reach_passed_tiles;
        self
    }

    /// 各 player の次のツモまでに、打牌・加槓のロン機会で通った牌種。
    ///
    /// `None` は入力経路から履歴を取得できないことを表し、空配列 (`Some`) と区別する。
    pub fn with_temporary_passed_tiles(
        mut self,
        temporary_passed_tiles: Option<[Vec<TileType>; 4]>,
    ) -> Self {
        self.temporary_passed_tiles = temporary_passed_tiles;
        self
    }

    /// 各 player の concealed hand が最後に変化して以降に、打牌・加槓のロン機会で通った牌種。
    ///
    /// `None` は入力経路から履歴を取得できないことを表し、空配列 (`Some`) と区別する。
    pub fn with_same_hand_passed_tiles(
        mut self,
        same_hand_passed_tiles: Option<[Vec<TileType>; 4]>,
    ) -> Self {
        self.same_hand_passed_tiles = same_hand_passed_tiles;
        self
    }

    pub fn with_table_state_facts(mut self, table_state: TableStateFacts) -> Self {
        self.table_state = table_state;
        self
    }

    pub fn with_history_furiten_facts(mut self, history_furiten: HistoryFuritenFacts) -> Self {
        self.history_furiten = history_furiten;
        self
    }

    pub fn drawn_tile(&self) -> Option<TileId> {
        self.drawn_tile
    }

    /// 今回の打牌の前に自分がツモしたと確定できるか。
    ///
    /// `drawn_tile` は全入力経路で「今回の打牌の前に自分が引いた牌」を表す。RiichiLab の
    /// observation adapter は自摸牌込み手牌から自摸牌を1枚分離して渡し、地鳳は `GET sutehai?`
    /// のツモ牌、JSON scenario は `draw` を渡す。どの経路でも `hand_tiles` と合わせて自摸後
    /// 14枚の手牌を作る値なので、他家の自摸牌が入ることはない。
    ///
    /// Chi / Pon 後の打牌や、自摸牌を特定できない経路では `None` になる。したがって `false` は
    /// 「ツモしていない」ではなく「自分のツモを経たと確認できない」を表す。
    pub fn is_after_own_draw(&self) -> bool {
        self.drawn_tile.is_some()
    }

    /// 現在時点 (今回の打牌の前) の履歴依存フリテン。
    ///
    /// 打牌後を評価する診断には、そのまま渡さず
    /// [`history_furiten_after_own_discard`](Self::history_furiten_after_own_discard) を使う。
    pub fn history_furiten(&self) -> HistoryFuritenFacts {
        self.history_furiten
    }

    /// 今回の打牌を1枚切り終えた時点の履歴依存フリテン。
    ///
    /// 打牌後のテンパイを評価する
    /// [`TenpaiWaitAvailability`](bot_logic::TenpaiWaitAvailability) は「選択した打牌を切った
    /// 後」の状態なので、履歴依存フリテンも同じ評価時点へ補正する必要がある。自分のツモを経た
    /// 打牌なら同巡内フリテンは解除されるため `same_turn` は `Some(false)` で確定し、鳴き後の
    /// 打牌や自摸を確認できない経路では現在の値を維持する。`riichi_missed_win` は局終了まで
    /// 続くので常に維持する。
    ///
    /// 補正規則そのものは pure helper ([`HistoryFuritenFacts::after_discard`]) が持ち、
    /// この経路は評価時点を表す事実 ([`is_after_own_draw`](Self::is_after_own_draw)) を渡す
    /// だけにする。
    pub fn history_furiten_after_own_discard(&self) -> HistoryFuritenFacts {
        self.history_furiten.after_discard(self.is_after_own_draw())
    }

    pub fn hand_tiles(&self) -> &[TileId] {
        &self.hand_tiles
    }

    pub fn dora_indicators(&self) -> &[TileId] {
        &self.dora_indicators
    }

    pub fn round_wind(&self) -> Option<TileType> {
        self.round_wind
    }

    pub fn seat_wind(&self) -> Option<TileType> {
        self.seat_wind
    }

    pub fn visible_tiles(&self) -> &[TileId] {
        &self.visible_tiles
    }

    pub fn player_id(&self) -> Option<u8> {
        self.player_id
    }

    pub fn oya(&self) -> Option<u8> {
        self.oya
    }

    /// 指定 player の自風。親から席順で導出できる場合だけ `Some`。
    ///
    /// 自分・他家を問わず [`seat_wind_for_player`] だけを source of truth にする。`oya` が無い
    /// 場合や範囲外の `player` では推測せず `None`。自分の席でも `seat_wind()` へ fallback せず、
    /// 全席で同じ規則にする。
    pub fn seat_wind_of(&self, player: usize) -> Option<TileType> {
        seat_wind_for_player(player, self.oya?)
    }

    // discards は防御・現物判定用に player ごとの河として保持する。
    pub fn discards(&self) -> &[Vec<TileId>; 4] {
        &self.discards
    }

    pub fn discards_of(&self, player: usize) -> Option<&[TileId]> {
        self.discards.get(player).map(Vec::as_slice)
    }

    /// 自分の河。`player_id` が無い場合は player 0 などを推測せず `None`。
    ///
    /// 恒常フリテン判定のように「自分が捨てた牌」が要る経路で使う。`None` は「河が空」ではなく
    /// 「自分の河を特定できない」を表す。
    pub fn own_discards(&self) -> Option<&[TileId]> {
        self.discards_of(usize::from(self.player_id?))
    }

    pub fn melds(&self) -> &[Vec<Meld>; 4] {
        &self.melds
    }

    pub fn melds_of(&self, player: usize) -> Option<&[Meld]> {
        self.melds.get(player).map(Vec::as_slice)
    }

    pub fn own_melds(&self) -> Option<&[Meld]> {
        self.melds_of(usize::from(self.player_id?))
    }

    pub fn own_fixed_meld_count(&self) -> Option<FixedMeldCount> {
        fixed_meld_count(self.own_melds()?)
    }

    pub fn reached(&self) -> &[bool; 4] {
        &self.reached
    }

    pub fn is_reached(&self, player: usize) -> bool {
        self.reached.get(player).copied().unwrap_or(false)
    }

    /// 自分がリーチ済みか。自分の席を特定できない場合は player 0 などを推測せず `None`。
    ///
    /// 席の特定は `player_id` だけを source of truth にし、`reached` の index を推測しない。
    /// `player_id` が無い場合と範囲外の場合はどちらも `None` で、`is_reached` のように範囲外を
    /// 「リーチしていない」へ倒さない。`None` は「未リーチ」ではなく「自分の席を特定できない」を
    /// 表す。
    pub fn own_reached(&self) -> Option<bool> {
        self.reached.get(usize::from(self.player_id?)).copied()
    }

    pub fn any_opponent_reached(&self) -> bool {
        self.reached
            .iter()
            .enumerate()
            .any(|(player, &reached)| reached && self.player_id != Some(player as u8))
    }

    pub fn post_reach_passed_tiles(&self) -> &[Vec<TileType>; 4] {
        &self.post_reach_passed_tiles
    }

    pub fn post_reach_passed_tiles_of(&self, player: usize) -> Option<&[TileType]> {
        self.post_reach_passed_tiles.get(player).map(Vec::as_slice)
    }

    pub fn is_post_reach_passed(&self, tile: TileType, player: usize) -> bool {
        self.post_reach_passed_tiles_of(player)
            .is_some_and(|tiles| tiles.contains(&tile))
    }

    pub fn temporary_passed_tiles(&self) -> Option<&[Vec<TileType>; 4]> {
        self.temporary_passed_tiles.as_ref()
    }

    pub fn temporary_passed_tiles_of(&self, player: usize) -> Option<&[TileType]> {
        self.temporary_passed_tiles
            .as_ref()?
            .get(player)
            .map(Vec::as_slice)
    }

    pub fn is_temporary_passed(&self, tile: TileType, player: usize) -> bool {
        self.temporary_passed_tiles_of(player)
            .is_some_and(|tiles| tiles.contains(&tile))
    }

    pub fn same_hand_passed_tiles(&self) -> Option<&[Vec<TileType>; 4]> {
        self.same_hand_passed_tiles.as_ref()
    }

    pub fn same_hand_passed_tiles_of(&self, player: usize) -> Option<&[TileType]> {
        self.same_hand_passed_tiles
            .as_ref()?
            .get(player)
            .map(Vec::as_slice)
    }

    pub fn is_same_hand_passed(&self, tile: TileType, player: usize) -> bool {
        self.same_hand_passed_tiles_of(player)
            .is_some_and(|tiles| tiles.contains(&tile))
    }

    pub fn table_state(&self) -> TableStateFacts {
        self.table_state
    }

    pub fn remaining_tiles(&self) -> Option<u32> {
        self.table_state.remaining_tiles
    }

    pub fn honba(&self) -> Option<u32> {
        self.table_state.honba
    }

    pub fn kyotaku_points(&self) -> Option<u32> {
        self.table_state.kyotaku_points
    }

    pub fn scores(&self) -> Option<[i32; 4]> {
        self.table_state.scores
    }

    pub fn kyoku(&self) -> Option<u8> {
        self.table_state.kyoku
    }

    pub fn score_of(&self, player: usize) -> Option<i32> {
        self.table_state.score_of(player)
    }

    /// 自分の現在持ち点。`player_id` が無い場合は player 0 などを推測せず `None`。
    pub fn own_score(&self) -> Option<i32> {
        self.score_of(usize::from(self.player_id?))
    }

    // リーチ者一覧: player_id がある場合は自分を除く。ない場合は reached 全員を返す。
    pub fn reached_opponents(&self) -> Vec<usize> {
        self.reached
            .iter()
            .enumerate()
            .filter(|&(player, &reached)| reached && self.player_id != Some(player as u8))
            .map(|(player, _)| player)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meld::MeldKind;

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn wind(value: u8) -> TileType {
        TileType::new(value).unwrap()
    }

    fn chi() -> Meld {
        Meld::new(
            MeldKind::Chi,
            vec![tile(0), tile(4), tile(8)],
            Some(tile(0)),
        )
    }

    fn pon() -> Meld {
        Meld::new(
            MeldKind::Pon,
            vec![tile(108), tile(109), tile(110)],
            Some(tile(108)),
        )
    }

    fn ankan() -> Meld {
        Meld::new(
            MeldKind::Ankan,
            vec![tile(112), tile(113), tile(114), tile(115)],
            None,
        )
    }

    fn kakan() -> Meld {
        Meld::new(
            MeldKind::Kakan,
            vec![tile(108), tile(109), tile(110), tile(111)],
            Some(tile(108)),
        )
    }

    fn meld_context(player_id: Option<u8>, melds: [Vec<Meld>; 4]) -> GameContext {
        GameContext::from_parts_with_melds(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            None,
            Default::default(),
            [false; 4],
            melds,
        )
    }

    #[test]
    fn default_has_no_drawn_tile() {
        assert_eq!(GameContext::default().drawn_tile(), None);
    }

    #[test]
    fn default_has_no_hand_tiles() {
        assert!(GameContext::default().hand_tiles().is_empty());
    }

    #[test]
    fn default_has_no_dora_indicators() {
        assert!(GameContext::default().dora_indicators().is_empty());
    }

    #[test]
    fn new_has_no_drawn_tile() {
        assert_eq!(GameContext::new().drawn_tile(), None);
    }

    #[test]
    fn new_has_no_hand_tiles() {
        assert!(GameContext::new().hand_tiles().is_empty());
    }

    #[test]
    fn with_drawn_tile_holds_tile() {
        let tile = tile(16);
        assert_eq!(GameContext::with_drawn_tile(tile).drawn_tile(), Some(tile));
    }

    #[test]
    fn with_drawn_tile_has_no_hand_tiles() {
        assert!(
            GameContext::with_drawn_tile(tile(16))
                .hand_tiles()
                .is_empty()
        );
    }

    #[test]
    fn with_drawn_tile_has_no_dora_indicators() {
        assert!(
            GameContext::with_drawn_tile(tile(16))
                .dora_indicators()
                .is_empty()
        );
    }

    #[test]
    fn with_hand_tiles_holds_tiles_without_drawn_tile() {
        let hand_tiles = vec![tile(0), tile(16), tile(56)];
        let context = GameContext::with_hand_tiles(hand_tiles.clone());
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.drawn_tile(), None);
    }

    #[test]
    fn with_hand_tiles_has_no_dora_indicators() {
        assert!(
            GameContext::with_hand_tiles(vec![tile(0)])
                .dora_indicators()
                .is_empty()
        );
    }

    #[test]
    fn from_parts_holds_both() {
        let hand_tiles = vec![tile(0), tile(56)];
        let context = GameContext::from_parts(Some(tile(16)), hand_tiles.clone());
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
    }

    #[test]
    fn from_parts_has_no_dora_indicators() {
        let context = GameContext::from_parts(Some(tile(16)), vec![tile(0)]);
        assert!(context.dora_indicators().is_empty());
    }

    #[test]
    fn from_parts_with_dora_holds_all() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4), tile(20)];
        let context = GameContext::from_parts_with_dora(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
    }

    #[test]
    fn hand_tiles_returns_slice() {
        let context = GameContext::with_hand_tiles(vec![tile(0)]);
        let slice: &[TileId] = context.hand_tiles();
        assert_eq!(slice, &[tile(0)]);
    }

    #[test]
    fn dora_indicators_returns_slice() {
        let context = GameContext::from_parts_with_dora(None, vec![], vec![tile(4)]);
        let slice: &[TileId] = context.dora_indicators();
        assert_eq!(slice, &[tile(4)]);
    }

    #[test]
    fn default_has_no_winds() {
        let context = GameContext::default();
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
    }

    #[test]
    fn existing_constructors_have_no_winds() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
        ] {
            assert_eq!(context.round_wind(), None);
            assert_eq!(context.seat_wind(), None);
        }
    }

    #[test]
    fn from_parts_with_context_holds_winds() {
        let context = GameContext::from_parts_with_context(
            Some(tile(16)),
            vec![tile(0)],
            vec![tile(4)],
            Some(wind(27)),
            Some(wind(28)),
        );
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
    }

    #[test]
    fn from_parts_with_context_keeps_other_parts() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        let context = GameContext::from_parts_with_context(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
            None,
            None,
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        assert_eq!(context.round_wind(), None);
        assert_eq!(context.seat_wind(), None);
    }

    #[test]
    fn from_parts_with_context_none_winds_equals_from_parts_with_dora() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        assert_eq!(
            GameContext::from_parts_with_context(
                Some(tile(16)),
                hand_tiles.clone(),
                dora_indicators.clone(),
                None,
                None,
            ),
            GameContext::from_parts_with_dora(Some(tile(16)), hand_tiles, dora_indicators)
        );
    }

    #[test]
    fn default_has_no_visible_tiles() {
        assert!(GameContext::default().visible_tiles().is_empty());
    }

    #[test]
    fn new_has_no_visible_tiles() {
        assert!(GameContext::new().visible_tiles().is_empty());
    }

    #[test]
    fn existing_constructors_have_no_visible_tiles() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
            ),
        ] {
            assert!(context.visible_tiles().is_empty());
        }
    }

    #[test]
    fn from_parts_with_visible_tiles_holds_visible_tiles() {
        let visible_tiles = vec![tile(0), tile(16), tile(16)];
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(16)),
            vec![tile(0)],
            vec![tile(4)],
            None,
            None,
            visible_tiles.clone(),
        );
        assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
    }

    #[test]
    fn from_parts_with_visible_tiles_keeps_other_parts() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        let visible_tiles = vec![tile(0), tile(56), tile(4)];
        let context = GameContext::from_parts_with_visible_tiles(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
            Some(wind(27)),
            Some(wind(28)),
            visible_tiles.clone(),
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
        assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
    }

    #[test]
    fn from_parts_with_context_has_no_visible_tiles() {
        let context = GameContext::from_parts_with_context(
            Some(tile(16)),
            vec![tile(0)],
            vec![tile(4)],
            Some(wind(27)),
            Some(wind(28)),
        );
        assert!(context.visible_tiles().is_empty());
    }

    #[test]
    fn from_parts_with_visible_tiles_empty_equals_from_parts_with_context() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        assert_eq!(
            GameContext::from_parts_with_visible_tiles(
                Some(tile(16)),
                hand_tiles.clone(),
                dora_indicators.clone(),
                Some(wind(27)),
                Some(wind(28)),
                Vec::new(),
            ),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                hand_tiles,
                dora_indicators,
                Some(wind(27)),
                Some(wind(28)),
            )
        );
    }

    #[test]
    fn visible_tiles_returns_slice() {
        let context = GameContext::from_parts_with_visible_tiles(
            None,
            vec![],
            vec![],
            None,
            None,
            vec![tile(0)],
        );
        let slice: &[TileId] = context.visible_tiles();
        assert_eq!(slice, &[tile(0)]);
    }

    fn table_state_context(
        player_id: Option<u8>,
        oya: Option<u8>,
        discards: [Vec<TileId>; 4],
        reached: [bool; 4],
    ) -> GameContext {
        GameContext::from_parts_with_table_state(
            None,
            vec![],
            vec![],
            None,
            None,
            Vec::new(),
            player_id,
            oya,
            discards,
            reached,
        )
    }

    #[test]
    fn default_has_no_player_id_or_oya() {
        let context = GameContext::default();
        assert_eq!(context.player_id(), None);
        assert_eq!(context.oya(), None);
    }

    #[test]
    fn default_has_empty_discards() {
        let context = GameContext::default();
        assert!(context.discards().iter().all(|d| d.is_empty()));
    }

    #[test]
    fn default_has_no_reached() {
        let context = GameContext::default();
        assert_eq!(context.reached(), &[false; 4]);
    }

    #[test]
    fn existing_constructors_have_empty_table_state() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
            ),
            GameContext::from_parts_with_visible_tiles(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
                vec![tile(0)],
            ),
        ] {
            assert_eq!(context.player_id(), None);
            assert_eq!(context.oya(), None);
            assert!(context.discards().iter().all(|d| d.is_empty()));
            assert_eq!(context.reached(), &[false; 4]);
        }
    }

    #[test]
    fn from_parts_with_table_state_holds_table_state() {
        let discards = [
            vec![tile(0)],
            vec![tile(16), tile(20)],
            vec![],
            vec![tile(104)],
        ];
        let context = table_state_context(
            Some(1),
            Some(2),
            discards.clone(),
            [false, true, false, true],
        );
        assert_eq!(context.player_id(), Some(1));
        assert_eq!(context.oya(), Some(2));
        assert_eq!(context.discards(), &discards);
        assert_eq!(context.reached(), &[false, true, false, true]);
    }

    #[test]
    fn from_parts_with_table_state_keeps_other_parts() {
        let hand_tiles = vec![tile(0), tile(56)];
        let dora_indicators = vec![tile(4)];
        let visible_tiles = vec![tile(0), tile(56)];
        let context = GameContext::from_parts_with_table_state(
            Some(tile(16)),
            hand_tiles.clone(),
            dora_indicators.clone(),
            Some(wind(27)),
            Some(wind(28)),
            visible_tiles.clone(),
            Some(0),
            Some(0),
            Default::default(),
            [false; 4],
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), hand_tiles.as_slice());
        assert_eq!(context.dora_indicators(), dora_indicators.as_slice());
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
        assert_eq!(context.visible_tiles(), visible_tiles.as_slice());
    }

    #[test]
    fn discards_returns_reference() {
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        let context = table_state_context(None, None, discards.clone(), [false; 4]);
        let reference: &[Vec<TileId>; 4] = context.discards();
        assert_eq!(reference, &discards);
    }

    #[test]
    fn reached_returns_reference() {
        let context =
            table_state_context(None, None, Default::default(), [true, false, false, false]);
        let reference: &[bool; 4] = context.reached();
        assert_eq!(reference, &[true, false, false, false]);
    }

    #[test]
    fn discards_of_returns_player_river() {
        let discards = [vec![tile(0)], vec![tile(16), tile(20)], vec![], vec![]];
        let context = table_state_context(None, None, discards, [false; 4]);
        assert_eq!(context.discards_of(0), Some([tile(0)].as_slice()));
        assert_eq!(
            context.discards_of(1),
            Some([tile(16), tile(20)].as_slice())
        );
        assert_eq!(context.discards_of(2), Some([].as_slice()));
    }

    #[test]
    fn discards_of_out_of_range_returns_none() {
        let context = GameContext::default();
        assert_eq!(context.discards_of(4), None);
        assert_eq!(context.discards_of(usize::MAX), None);
    }

    #[test]
    fn own_discards_follow_player_id() {
        let discards = [vec![tile(0)], vec![tile(16), tile(20)], vec![], vec![]];
        let context = table_state_context(Some(1), None, discards, [false; 4]);
        assert_eq!(
            context.own_discards(),
            Some([tile(16), tile(20)].as_slice())
        );
    }

    #[test]
    fn own_discards_are_empty_when_the_known_player_has_not_discarded() {
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        let context = table_state_context(Some(2), None, discards, [false; 4]);
        assert_eq!(context.own_discards(), Some([].as_slice()));
    }

    #[test]
    fn own_discards_are_none_when_player_id_is_unknown() {
        // player 0 の河を自分の河と推測しない。
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        let context = table_state_context(None, None, discards, [false; 4]);
        assert_eq!(context.own_discards(), None);
    }

    #[test]
    fn is_reached_reports_per_player() {
        let context =
            table_state_context(None, None, Default::default(), [false, true, false, false]);
        assert!(!context.is_reached(0));
        assert!(context.is_reached(1));
    }

    #[test]
    fn is_reached_out_of_range_returns_false() {
        let context = table_state_context(None, None, Default::default(), [true; 4]);
        assert!(!context.is_reached(4));
    }

    #[test]
    fn any_opponent_reached_detects_other_players() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, false, true, false],
        );
        assert!(context.any_opponent_reached());
    }

    #[test]
    fn any_opponent_reached_ignores_own_reach() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [true, false, false, false],
        );
        assert!(!context.any_opponent_reached());
    }

    #[test]
    fn any_opponent_reached_without_player_id_reports_any_reach() {
        let context =
            table_state_context(None, None, Default::default(), [true, false, false, false]);
        assert!(context.any_opponent_reached());
    }

    #[test]
    fn any_opponent_reached_is_false_when_nobody_reached() {
        let context = table_state_context(Some(0), None, Default::default(), [false; 4]);
        assert!(!context.any_opponent_reached());
    }

    #[test]
    fn reached_opponents_excludes_self() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [false, true, false, false],
        );
        assert_eq!(context.reached_opponents(), vec![1]);
    }

    #[test]
    fn reached_opponents_ignores_own_reach() {
        let context = table_state_context(
            Some(0),
            None,
            Default::default(),
            [true, false, false, false],
        );
        assert!(context.reached_opponents().is_empty());
    }

    #[test]
    fn reached_opponents_collects_multiple() {
        let context = table_state_context(
            Some(2),
            None,
            Default::default(),
            [true, false, false, true],
        );
        assert_eq!(context.reached_opponents(), vec![0, 3]);
    }

    #[test]
    fn reached_opponents_without_player_id_reports_all_reached() {
        let context =
            table_state_context(None, None, Default::default(), [true, false, true, false]);
        assert_eq!(context.reached_opponents(), vec![0, 2]);
    }

    #[test]
    fn reached_opponents_empty_when_nobody_reached() {
        let context = table_state_context(Some(0), None, Default::default(), [false; 4]);
        assert!(context.reached_opponents().is_empty());
    }

    #[test]
    fn default_has_no_post_reach_passed_tiles() {
        let context = GameContext::default();
        assert!(context.post_reach_passed_tiles().iter().all(Vec::is_empty));
    }

    #[test]
    fn existing_constructors_have_no_post_reach_passed_tiles() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
            table_state_context(Some(0), Some(0), Default::default(), [false; 4]),
            meld_context(Some(0), [vec![pon()], vec![], vec![], vec![]]),
        ] {
            assert!(context.post_reach_passed_tiles().iter().all(Vec::is_empty));
        }
    }

    #[test]
    fn with_post_reach_passed_tiles_holds_tiles_per_player() {
        let four_sou = tile(84).tile_type();
        let context = GameContext::default().with_post_reach_passed_tiles([
            vec![],
            vec![four_sou],
            vec![],
            vec![],
        ]);
        assert_eq!(context.post_reach_passed_tiles_of(0), Some([].as_slice()));
        assert_eq!(
            context.post_reach_passed_tiles_of(1),
            Some([four_sou].as_slice())
        );
    }

    #[test]
    fn with_post_reach_passed_tiles_keeps_other_parts() {
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        let base = table_state_context(
            Some(1),
            Some(2),
            discards.clone(),
            [false, true, false, false],
        );
        let context = base.clone().with_post_reach_passed_tiles([
            vec![],
            vec![tile(84).tile_type()],
            vec![],
            vec![],
        ]);
        assert_eq!(context.player_id(), base.player_id());
        assert_eq!(context.oya(), base.oya());
        assert_eq!(context.discards(), &discards);
        assert_eq!(context.reached(), base.reached());
    }

    #[test]
    fn is_post_reach_passed_reports_per_player() {
        let four_sou = tile(84).tile_type();
        let five_sou = tile(88).tile_type();
        let context = GameContext::default().with_post_reach_passed_tiles([
            vec![],
            vec![four_sou],
            vec![],
            vec![],
        ]);
        assert!(context.is_post_reach_passed(four_sou, 1));
        assert!(!context.is_post_reach_passed(four_sou, 2));
        assert!(!context.is_post_reach_passed(five_sou, 1));
    }

    #[test]
    fn post_reach_passed_tiles_out_of_range_returns_none() {
        let context = GameContext::default();
        assert_eq!(context.post_reach_passed_tiles_of(4), None);
        assert!(!context.is_post_reach_passed(tile(84).tile_type(), 4));
    }

    fn full_table_state_facts() -> TableStateFacts {
        TableStateFacts {
            remaining_tiles: Some(42),
            honba: Some(1),
            kyotaku_points: Some(2000),
            scores: Some([12300, 28700, 40100, 18900]),
            kyoku: Some(3),
        }
    }

    #[test]
    fn default_table_state_is_all_unknown() {
        let context = GameContext::default();
        assert_eq!(context.table_state(), TableStateFacts::default());
        assert_eq!(context.remaining_tiles(), None);
        assert_eq!(context.honba(), None);
        assert_eq!(context.kyotaku_points(), None);
        assert_eq!(context.scores(), None);
        assert_eq!(context.kyoku(), None);
    }

    #[test]
    fn existing_constructors_have_unknown_table_state() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
            ),
            GameContext::from_parts_with_visible_tiles(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
                vec![tile(0)],
            ),
            table_state_context(Some(0), Some(0), Default::default(), [false; 4]),
            meld_context(Some(0), [vec![pon()], vec![], vec![], vec![]]),
        ] {
            assert_eq!(context.table_state(), TableStateFacts::default());
        }
    }

    #[test]
    fn with_table_state_facts_holds_every_fact() {
        let context = GameContext::default().with_table_state_facts(full_table_state_facts());
        assert_eq!(context.remaining_tiles(), Some(42));
        assert_eq!(context.honba(), Some(1));
        assert_eq!(context.kyotaku_points(), Some(2000));
        assert_eq!(context.scores(), Some([12300, 28700, 40100, 18900]));
        assert_eq!(context.kyoku(), Some(3));
    }

    #[test]
    fn with_table_state_facts_keeps_other_parts() {
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        let base = table_state_context(
            Some(1),
            Some(2),
            discards.clone(),
            [false, true, false, false],
        );
        let context = base
            .clone()
            .with_table_state_facts(full_table_state_facts());
        assert_eq!(context.player_id(), base.player_id());
        assert_eq!(context.oya(), base.oya());
        assert_eq!(context.discards(), &discards);
        assert_eq!(context.reached(), base.reached());
    }

    #[test]
    fn with_post_reach_passed_tiles_keeps_table_state_facts() {
        let context = GameContext::default()
            .with_table_state_facts(full_table_state_facts())
            .with_post_reach_passed_tiles([vec![tile(84).tile_type()], vec![], vec![], vec![]]);
        assert_eq!(context.table_state(), full_table_state_facts());
    }

    #[test]
    fn known_zero_table_state_differs_from_unknown() {
        let known_zero = GameContext::default().with_table_state_facts(TableStateFacts {
            remaining_tiles: Some(0),
            honba: Some(0),
            kyotaku_points: Some(0),
            scores: Some([0; 4]),
            kyoku: Some(1),
        });
        let unknown = GameContext::default();

        assert_eq!(known_zero.remaining_tiles(), Some(0));
        assert_eq!(known_zero.honba(), Some(0));
        assert_eq!(known_zero.kyotaku_points(), Some(0));
        assert_eq!(known_zero.scores(), Some([0; 4]));
        assert_ne!(known_zero.remaining_tiles(), unknown.remaining_tiles());
        assert_ne!(known_zero.honba(), unknown.honba());
        assert_ne!(known_zero.kyotaku_points(), unknown.kyotaku_points());
        assert_ne!(known_zero.scores(), unknown.scores());
        assert_ne!(known_zero.table_state(), unknown.table_state());
    }

    #[test]
    fn score_of_returns_the_score_of_each_seat() {
        let context = GameContext::default().with_table_state_facts(full_table_state_facts());
        assert_eq!(context.score_of(0), Some(12300));
        assert_eq!(context.score_of(1), Some(28700));
        assert_eq!(context.score_of(2), Some(40100));
        assert_eq!(context.score_of(3), Some(18900));
    }

    #[test]
    fn score_of_out_of_range_returns_none() {
        let context = GameContext::default().with_table_state_facts(full_table_state_facts());
        assert_eq!(context.score_of(4), None);
        assert_eq!(context.score_of(usize::MAX), None);
    }

    #[test]
    fn score_of_is_none_when_scores_are_unknown() {
        let context = GameContext::default();
        assert_eq!(context.score_of(0), None);
    }

    #[test]
    fn own_reached_follows_player_id() {
        let context = table_state_context(
            Some(2),
            None,
            Default::default(),
            [true, false, true, false],
        );
        assert_eq!(context.own_reached(), Some(true));

        let context = table_state_context(
            Some(1),
            None,
            Default::default(),
            [true, false, true, false],
        );
        assert_eq!(context.own_reached(), Some(false));
    }

    #[test]
    fn own_reached_is_none_when_player_id_is_unknown() {
        // player 0 のリーチを自分のリーチと推測しない。
        let context =
            table_state_context(None, None, Default::default(), [true, false, false, false]);
        assert_eq!(context.player_id(), None);
        assert_eq!(context.own_reached(), None);
    }

    #[test]
    fn own_reached_is_none_for_an_out_of_range_player_id() {
        // 席を特定できない以上「未リーチ」とは確定できない。is_reached() の範囲外 fallback へ
        // 倒さず、自分の席を特定できないことを None で表す。
        for player_id in [4, u8::MAX] {
            let context = table_state_context(
                Some(player_id),
                None,
                Default::default(),
                [true, false, false, false],
            );
            assert_eq!(context.own_reached(), None);
        }
    }

    #[test]
    fn own_score_follows_player_id() {
        let context = table_state_context(Some(2), None, Default::default(), [false; 4])
            .with_table_state_facts(full_table_state_facts());
        assert_eq!(context.own_score(), Some(40100));
    }

    #[test]
    fn own_score_is_none_when_player_id_is_unknown() {
        // player 0 の点棒を自分の点棒と推測しない。
        let context = GameContext::default().with_table_state_facts(full_table_state_facts());
        assert_eq!(context.player_id(), None);
        assert_eq!(context.own_score(), None);
    }

    #[test]
    fn own_score_is_none_when_scores_are_unknown() {
        let context = table_state_context(Some(0), None, Default::default(), [false; 4]);
        assert_eq!(context.own_score(), None);
    }

    #[test]
    fn table_state_facts_score_of_matches_the_context_accessor() {
        let facts = full_table_state_facts();
        let context = GameContext::default().with_table_state_facts(facts);
        for player in 0..5 {
            assert_eq!(facts.score_of(player), context.score_of(player));
        }
    }

    #[test]
    fn seat_wind_for_player_derives_from_oya() {
        assert_eq!(seat_wind_for_player(0, 0), Some(wind(27)));
        assert_eq!(seat_wind_for_player(1, 0), Some(wind(28)));
        assert_eq!(seat_wind_for_player(2, 0), Some(wind(29)));
        assert_eq!(seat_wind_for_player(3, 0), Some(wind(30)));
        assert_eq!(seat_wind_for_player(1, 1), Some(wind(27)));
        assert_eq!(seat_wind_for_player(0, 3), Some(wind(28)));
    }

    #[test]
    fn seat_wind_for_player_rejects_out_of_range() {
        assert_eq!(seat_wind_for_player(4, 0), None);
        assert_eq!(seat_wind_for_player(0, 4), None);
        assert_eq!(seat_wind_for_player(usize::MAX, 0), None);
    }

    #[test]
    fn seat_wind_of_derives_every_seat_from_oya() {
        let context = table_state_context(Some(0), Some(2), Default::default(), [false; 4]);
        assert_eq!(context.seat_wind_of(2), Some(wind(27)));
        assert_eq!(context.seat_wind_of(3), Some(wind(28)));
        assert_eq!(context.seat_wind_of(0), Some(wind(29)));
        assert_eq!(context.seat_wind_of(1), Some(wind(30)));
    }

    #[test]
    fn seat_wind_of_is_none_when_oya_is_unknown() {
        // 自分の seat_wind が分かっていても、oya が無ければどの席の自風も推測しない。
        let context = GameContext::from_parts_with_context(
            None,
            vec![],
            vec![],
            Some(wind(27)),
            Some(wind(28)),
        );
        assert_eq!(context.oya(), None);
        assert_eq!(context.seat_wind_of(0), None);
        assert_eq!(context.seat_wind_of(1), None);
    }

    #[test]
    fn seat_wind_of_out_of_range_returns_none() {
        let context = table_state_context(Some(0), Some(0), Default::default(), [false; 4]);
        assert_eq!(context.seat_wind_of(4), None);
        assert_eq!(context.seat_wind_of(usize::MAX), None);
    }

    #[test]
    fn default_has_empty_melds() {
        let context = GameContext::default();
        assert!(context.melds().iter().all(|melds| melds.is_empty()));
    }

    #[test]
    fn default_has_no_own_melds_because_player_is_unknown() {
        let context = GameContext::default();
        assert_eq!(context.player_id(), None);
        assert_eq!(context.own_melds(), None);
        assert_eq!(context.own_fixed_meld_count(), None);
    }

    #[test]
    fn default_with_known_player_has_empty_own_melds() {
        let context = meld_context(Some(0), Default::default());
        assert_eq!(context.own_melds(), Some([].as_slice()));
        assert_eq!(context.own_fixed_meld_count(), Some(FixedMeldCount::NONE));
    }

    #[test]
    fn existing_constructors_have_empty_melds() {
        for context in [
            GameContext::new(),
            GameContext::with_drawn_tile(tile(16)),
            GameContext::with_hand_tiles(vec![tile(0)]),
            GameContext::from_parts(Some(tile(16)), vec![tile(0)]),
            GameContext::from_parts_with_dora(Some(tile(16)), vec![tile(0)], vec![tile(4)]),
            GameContext::from_parts_with_context(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
            ),
            GameContext::from_parts_with_visible_tiles(
                Some(tile(16)),
                vec![tile(0)],
                vec![tile(4)],
                Some(wind(27)),
                Some(wind(28)),
                vec![tile(0)],
            ),
            table_state_context(Some(0), Some(0), Default::default(), [false; 4]),
        ] {
            assert!(context.melds().iter().all(|melds| melds.is_empty()));
        }
    }

    #[test]
    fn from_parts_with_melds_empty_equals_from_parts_with_table_state() {
        let discards = [vec![tile(0)], vec![], vec![], vec![]];
        assert_eq!(
            GameContext::from_parts_with_melds(
                Some(tile(16)),
                vec![tile(4)],
                vec![tile(8)],
                Some(wind(27)),
                Some(wind(28)),
                vec![tile(4)],
                Some(1),
                Some(2),
                discards.clone(),
                [false, true, false, false],
                Default::default(),
            ),
            GameContext::from_parts_with_table_state(
                Some(tile(16)),
                vec![tile(4)],
                vec![tile(8)],
                Some(wind(27)),
                Some(wind(28)),
                vec![tile(4)],
                Some(1),
                Some(2),
                discards,
                [false, true, false, false],
            )
        );
    }

    #[test]
    fn from_parts_with_melds_keeps_other_parts() {
        let context = GameContext::from_parts_with_melds(
            Some(tile(16)),
            vec![tile(4)],
            vec![tile(8)],
            Some(wind(27)),
            Some(wind(28)),
            vec![tile(4), tile(8)],
            Some(1),
            Some(2),
            [vec![tile(0)], vec![], vec![], vec![]],
            [false, true, false, false],
            [vec![pon()], vec![], vec![], vec![]],
        );
        assert_eq!(context.drawn_tile(), Some(tile(16)));
        assert_eq!(context.hand_tiles(), [tile(4)]);
        assert_eq!(context.dora_indicators(), [tile(8)]);
        assert_eq!(context.round_wind(), Some(wind(27)));
        assert_eq!(context.seat_wind(), Some(wind(28)));
        assert_eq!(context.visible_tiles(), [tile(4), tile(8)]);
        assert_eq!(context.player_id(), Some(1));
        assert_eq!(context.oya(), Some(2));
        assert_eq!(context.discards_of(0), Some([tile(0)].as_slice()));
        assert_eq!(context.reached(), &[false, true, false, false]);
    }

    #[test]
    fn melds_of_returns_player_melds() {
        let context = meld_context(Some(0), [vec![pon()], vec![chi()], vec![], vec![]]);
        assert_eq!(context.melds_of(0), Some([pon()].as_slice()));
        assert_eq!(context.melds_of(1), Some([chi()].as_slice()));
        assert_eq!(context.melds_of(2), Some([].as_slice()));
    }

    #[test]
    fn melds_of_out_of_range_returns_none() {
        let context = GameContext::default();
        assert_eq!(context.melds_of(4), None);
        assert_eq!(context.melds_of(usize::MAX), None);
    }

    #[test]
    fn single_pon_makes_own_fixed_meld_count_one() {
        let context = meld_context(Some(0), [vec![pon()], vec![], vec![], vec![]]);
        assert_eq!(context.own_melds(), Some([pon()].as_slice()));
        assert_eq!(
            context.own_fixed_meld_count().map(FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn own_melds_follow_player_id() {
        let context = meld_context(Some(2), [vec![pon()], vec![], vec![chi()], vec![]]);
        assert_eq!(context.own_melds(), Some([chi()].as_slice()));
        assert_eq!(
            context.own_fixed_meld_count().map(FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn opponent_melds_do_not_change_own_fixed_meld_count() {
        let context = meld_context(Some(0), [vec![], vec![pon(), chi()], vec![ankan()], vec![]]);
        assert_eq!(context.own_fixed_meld_count(), Some(FixedMeldCount::NONE));
    }

    #[test]
    fn chi_pon_and_ankan_count_as_three_fixed_melds() {
        let context = meld_context(
            Some(0),
            [vec![chi(), pon(), ankan()], vec![], vec![], vec![]],
        );
        assert_eq!(
            context.own_fixed_meld_count().map(FixedMeldCount::get),
            Some(3)
        );
    }

    #[test]
    fn ankan_alone_counts_as_one_fixed_meld_but_is_not_open() {
        let context = meld_context(Some(0), [vec![ankan()], vec![], vec![], vec![]]);
        assert_eq!(
            context.own_fixed_meld_count().map(FixedMeldCount::get),
            Some(1)
        );
        let own_melds = context.own_melds().unwrap();
        assert_eq!(own_melds[0].kind(), MeldKind::Ankan);
        assert!(!own_melds[0].is_open());
        assert!(own_melds.iter().all(|meld| !meld.is_open()));
    }

    #[test]
    fn open_melds_are_distinguished_from_ankan() {
        let context = meld_context(Some(0), [vec![chi(), ankan()], vec![], vec![], vec![]]);
        let own_melds = context.own_melds().unwrap();
        assert_eq!(
            own_melds
                .iter()
                .map(|meld| meld.is_open())
                .collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn kakan_replaces_the_pon_and_stays_one_fixed_meld() {
        let context = meld_context(Some(0), [vec![kakan()], vec![], vec![], vec![]]);
        let own_melds = context.own_melds().unwrap();
        assert_eq!(own_melds.len(), 1);
        assert_eq!(own_melds[0].kind(), MeldKind::Kakan);
        assert_eq!(
            context.own_fixed_meld_count().map(FixedMeldCount::get),
            Some(1)
        );
    }

    #[test]
    fn own_fixed_meld_count_is_none_when_player_id_is_unknown() {
        let context = meld_context(None, [vec![pon()], vec![], vec![], vec![]]);
        assert_eq!(context.own_melds(), None);
        assert_eq!(context.own_fixed_meld_count(), None);
    }

    #[test]
    fn own_fixed_meld_count_is_none_when_melds_exceed_the_maximum() {
        let melds = vec![pon(), chi(), ankan(), pon(), chi()];
        let context = meld_context(Some(0), [melds, vec![], vec![], vec![]]);
        assert_eq!(context.own_melds().map(<[Meld]>::len), Some(5));
        assert_eq!(context.own_fixed_meld_count(), None);
    }

    #[test]
    fn history_furiten_defaults_to_unknown_and_preserves_known_values() {
        assert_eq!(
            GameContext::default().history_furiten(),
            HistoryFuritenFacts::default()
        );
        for facts in [
            HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            },
            HistoryFuritenFacts {
                same_turn: Some(true),
                riichi_missed_win: Some(true),
            },
        ] {
            assert_eq!(
                GameContext::default()
                    .with_history_furiten_facts(facts)
                    .history_furiten(),
                facts
            );
        }
    }
}
