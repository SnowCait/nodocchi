use bot_logic::{
    HandValueOutcome, RiichiStatus, TileCounts, TileId, TileType, WinMethod, WinningContext,
    analyze_completed_hand, evaluate_hand_value, evaluate_ron_deal_in_payment,
};

use crate::context::{GameContext, RiichiSituationFacts};
use crate::defense::hidden_hand_states::ReachedHiddenHandStates;
use crate::defense::structural_expected_deal_in_loss::{
    StructuralExpectedDealInLossEvidence, StructuralExpectedDealInLossMetrics,
    StructuralExpectedDealInLossUnavailable, structural_expected_deal_in_loss,
    structural_expected_deal_in_loss_with_metrics,
};
use crate::defense::{CompressedHiddenHandStates, remaining_tile_copies};
use crate::meld::{Meld, MeldKind};

use super::common::tile_type;

const REACHER: usize = 1;
const TILE_COPIES: u8 = 4;

// 期待損失の検証用に、リーチ者の未知牌 pool そのものを指定する fixture。
//
// `live` で指定した牌種だけを未知に残し、それ以外の牌種は4枚すべてを見え牌にして残枚数を0に
// する。隠れ手牌の state space と裏ドラ pool を小さく固定できるので、production の数え上げと
// test 側の総当たり reference を突き合わせられる。
struct ReacherFixture {
    live: Vec<(TileType, u8)>,
    ankans: Vec<TileType>,
    discard: TileId,
    dora_indicators: Vec<TileType>,
    reacher_river: Vec<TileType>,
    oya: u8,
    round_wind: TileType,
    honba: Option<u32>,
    remaining_tiles: Option<u32>,
    declared_double_riichi: Option<bool>,
    ippatsu: Option<bool>,
    seen_red_fives: Vec<TileType>,
}

impl ReacherFixture {
    // 3暗槓のリーチ者を既定にして、隠れ手牌を4枚に固定する。
    fn new(live: &[(&str, u8)], discard: TileId) -> Self {
        Self {
            live: live
                .iter()
                .map(|(mjai, remaining)| (tile_type(mjai), *remaining))
                .collect(),
            ankans: ["2m", "3p", "4s"].iter().map(|m| tile_type(m)).collect(),
            discard,
            dora_indicators: vec![tile_type("9m")],
            reacher_river: vec![tile_type("C")],
            oya: 0,
            round_wind: tile_type("E"),
            honba: Some(0),
            remaining_tiles: Some(20),
            declared_double_riichi: Some(false),
            ippatsu: Some(false),
            seen_red_fives: Vec::new(),
        }
    }

    fn with_dora_indicators(mut self, dora_indicators: &[&str]) -> Self {
        self.dora_indicators = dora_indicators.iter().map(|m| tile_type(m)).collect();
        self
    }

    fn with_honba(mut self, honba: Option<u32>) -> Self {
        self.honba = honba;
        self
    }

    fn with_oya(mut self, oya: u8) -> Self {
        self.oya = oya;
        self
    }

    fn with_declared_double_riichi(mut self, declared: Option<bool>) -> Self {
        self.declared_double_riichi = declared;
        self
    }

    fn with_ippatsu(mut self, ippatsu: Option<bool>) -> Self {
        self.ippatsu = ippatsu;
        self
    }

    fn with_seen_red_five(mut self, tile: &str) -> Self {
        self.seen_red_fives.push(tile_type(tile));
        self
    }

    fn with_reacher_river(mut self, river: &[&str]) -> Self {
        self.reacher_river = river.iter().map(|m| tile_type(m)).collect();
        self
    }

    fn context(&self) -> GameContext {
        let mut allocator = VisibleTileAllocator::default();
        let melds: Vec<Meld> = self
            .ankans
            .iter()
            .map(|tile| {
                let tiles: Vec<TileId> = (0..TILE_COPIES).map(|_| allocator.take(*tile)).collect();
                Meld::new(MeldKind::Ankan, tiles, None)
            })
            .collect();
        assert!(
            self.ankans
                .iter()
                .all(|tile| !self.live.iter().any(|(live, _)| live == tile)),
            "暗槓の牌種は未知 pool に残らない"
        );

        allocator.reserve(self.discard);
        let dora_indicators: Vec<TileId> = self
            .dora_indicators
            .iter()
            .map(|tile| allocator.take(*tile))
            .collect();
        let reacher_river: Vec<TileId> = self
            .reacher_river
            .iter()
            .map(|tile| allocator.take(*tile))
            .collect();
        let seen_red_fives: Vec<TileId> = self
            .seen_red_fives
            .iter()
            .map(|tile| allocator.take_red(*tile))
            .collect();

        let mut visible = vec![self.discard];
        visible.extend(melds.iter().flat_map(|meld| meld.tiles().to_vec()));
        visible.extend(dora_indicators.iter().copied());
        visible.extend(reacher_river.iter().copied());
        visible.extend(seen_red_fives);

        // 未知に残す牌種だけ残枚数を指定どおりにし、他は4枚すべてを見え牌にする。
        for tile in TileType::all() {
            let remaining = self
                .live
                .iter()
                .find(|(live, _)| *live == tile)
                .map_or(0, |(_, remaining)| *remaining);
            let visible_count = visible.iter().filter(|id| id.tile_type() == tile).count() as u8;
            let target = TILE_COPIES - remaining;
            assert!(
                visible_count <= target,
                "{} は見え牌が多すぎる",
                tile.to_mjai_string()
            );
            for _ in visible_count..target {
                visible.push(allocator.take(tile));
            }
        }

        let mut player_melds: [Vec<Meld>; 4] = Default::default();
        player_melds[REACHER] = melds;
        let mut discards: [Vec<TileId>; 4] = Default::default();
        discards[REACHER] = reacher_river;
        let mut reached = [false; 4];
        reached[REACHER] = true;

        let mut declared_double_riichi = [None; 4];
        declared_double_riichi[REACHER] = self.declared_double_riichi;
        let mut ippatsu = [None; 4];
        ippatsu[REACHER] = self.ippatsu;

        GameContext::from_parts_with_melds(
            None,
            vec![self.discard],
            dora_indicators,
            Some(self.round_wind),
            None,
            visible,
            Some(0),
            Some(self.oya),
            discards,
            reached,
            player_melds,
        )
        .with_table_state_facts(crate::context::TableStateFacts {
            remaining_tiles: self.remaining_tiles,
            honba: self.honba,
            kyotaku_points: Some(3000),
            scores: None,
            kyoku: None,
        })
        .with_riichi_situation_facts(RiichiSituationFacts {
            declared_double_riichi,
            ippatsu,
        })
    }
}

// 見え牌へ回す物理牌を1枚ずつ確保する allocator。
//
// 赤5は最後に回すので、残枚数が1枚以上ある5牌種では赤5が未知のまま残る。
#[derive(Debug)]
struct VisibleTileAllocator {
    used: [bool; TileId::COUNT],
}

impl Default for VisibleTileAllocator {
    fn default() -> Self {
        Self {
            used: [false; TileId::COUNT],
        }
    }
}

impl VisibleTileAllocator {
    fn reserve(&mut self, id: TileId) {
        self.used[id.index()] = true;
    }

    fn take(&mut self, tile: TileType) -> TileId {
        let mut copies: Vec<TileId> = TileId::copies(tile).collect();
        copies.sort_by_key(|id| (id.is_red(), std::cmp::Reverse(id.copy_index())));
        self.take_from(copies)
    }

    fn take_red(&mut self, tile: TileType) -> TileId {
        let copies: Vec<TileId> = TileId::copies(tile).filter(|id| id.is_red()).collect();
        self.take_from(copies)
    }

    fn take_from(&mut self, copies: Vec<TileId>) -> TileId {
        let id = copies
            .into_iter()
            .find(|id| !self.used[id.index()])
            .expect("at most four copies per tile type");
        self.reserve(id);
        id
    }
}

fn held(mjai: &str) -> TileId {
    TileId::copies(tile_type(mjai))
        .find(|id| !id.is_red())
        .expect("every tile type has a black copy")
}

fn red(mjai: &str) -> TileId {
    TileId::copies(tile_type(mjai))
        .find(|id| id.is_red())
        .expect("the tile type has a red copy")
}

// ------------------------------------------------------------------------------------------------
// production とは独立した総当たり reference
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Reference {
    loss_weighted_sum: u128,
    ron_capable_weight: u128,
    ura_arrangements: u128,
    payments: Vec<u32>,
}

// 隠れ手牌を素朴に総当たりして、同じ整数 evidence を組み立てる reference。
//
// 数え上げの順序も weight の掛け方も production と分けてある。ロン可否だけは既存 exact model の
// 公開判定 (`is_ron_capable_hidden_hand`) を source of truth として共有する。裏ドラは牌種の
// 多重集合ではなく、未知の物理牌から `slots` 枚を選ぶ部分集合をすべて数え上げる。
fn reference_evidence(discard: TileId, context: &GameContext) -> Reference {
    let mut remaining = [0u8; TileType::COUNT];
    for tile in TileType::all() {
        remaining[tile.index()] = remaining_tile_copies(tile, context);
    }
    let states = ReachedHiddenHandStates::new(REACHER, context).expect("menzen reacher");
    let concealed_len = usize::from(states.concealed_hand_len());
    let melds = context.melds_of(REACHER).expect("known player").to_vec();
    let seat_wind = context.seat_wind_of(REACHER).expect("known seat wind");
    let riichi = match context.declared_double_riichi_of(REACHER) {
        Some(true) => RiichiStatus::DoubleRiichi,
        Some(false) => RiichiStatus::Riichi,
        None => panic!("reference needs a known riichi kind"),
    };
    let winning_context = WinningContext::new(WinMethod::Ron)
        .with_round_wind(context.round_wind())
        .with_seat_wind(Some(seat_wind))
        .with_riichi(riichi)
        .with_ippatsu(context.ippatsu_of(REACHER))
        .with_rinshan(Some(false))
        .with_chankan(Some(false))
        .with_remaining_live_tiles(context.remaining_tiles());

    let mut reference = Reference::default();
    let mut hand = [0u8; TileType::COUNT];
    enumerate_hands(&remaining, concealed_len, 0, &mut hand, &mut |hand| {
        let counts = TileCounts::try_from(*hand).expect("at most four copies per tile type");
        if !states.is_ron_capable_hidden_hand(&counts, discard.tile_type()) {
            return;
        }
        reference.ron_capable_weight += hand_weight(&remaining, hand);

        for (red_fives, variant_weight) in red_five_variants(&remaining, hand, context) {
            if variant_weight == 0 {
                continue;
            }
            let mut ids: Vec<TileId> = Vec::new();
            for tile in TileType::all() {
                let count = hand[tile.index()];
                let mut copies: Vec<TileId> = TileId::copies(tile)
                    .filter(|id| !id.is_red())
                    .take(usize::from(count))
                    .collect();
                if red_fives.contains(&tile) {
                    copies.pop();
                    copies.push(red(&tile.to_mjai_string()));
                }
                ids.extend(copies);
            }
            ids.push(discard);
            let analysis = analyze_completed_hand(&ids, &melds).expect("a legal complete hand");

            let mut pool: Vec<TileType> = Vec::new();
            for tile in TileType::all() {
                for _ in 0..(remaining[tile.index()] - hand[tile.index()]) {
                    pool.push(tile);
                }
            }
            let slots = context.dora_indicators().len();
            let mut arrangements = 0u128;
            for indicators in subsets(&pool, slots) {
                let indicator_ids: Vec<TileId> = indicators
                    .iter()
                    .map(|tile| {
                        TileId::copies(*tile)
                            .find(|id| !id.is_red())
                            .expect("a black copy")
                    })
                    .collect();
                let outcome = evaluate_hand_value(
                    &analysis,
                    winning_context,
                    discard.tile_type(),
                    context.dora_indicators(),
                    Some(&indicator_ids),
                )
                .expect("the scoring layer evaluates the hand");
                let HandValueOutcome::Known(hand_value) = outcome else {
                    panic!("the reference needs a known hand value");
                };
                let payment = evaluate_ron_deal_in_payment(&hand_value, context.honba())
                    .expect("a known ron payment");
                reference.payments.push(payment);
                reference.loss_weighted_sum += variant_weight * u128::from(payment);
                arrangements += 1;
            }
            reference.ura_arrangements = arrangements;
        }
    });
    reference.payments.sort_unstable();
    reference.payments.dedup();
    reference
}

fn enumerate_hands(
    remaining: &[u8; TileType::COUNT],
    left: usize,
    start: usize,
    hand: &mut [u8; TileType::COUNT],
    visit: &mut impl FnMut(&[u8; TileType::COUNT]),
) {
    if left == 0 {
        visit(hand);
        return;
    }
    for index in start..TileType::COUNT {
        let available = usize::from(remaining[index]).min(left);
        for count in 1..=available {
            hand[index] = count as u8;
            enumerate_hands(remaining, left - count, index + 1, hand, visit);
            hand[index] = 0;
        }
    }
}

fn hand_weight(remaining: &[u8; TileType::COUNT], hand: &[u8; TileType::COUNT]) -> u128 {
    let mut weight = 1u128;
    for index in 0..TileType::COUNT {
        weight *= binomial(remaining[index], hand[index]);
    }
    weight
}

// 赤5を含む物理配置と黒5だけの配置へ、組み合わせ数で分ける。
fn red_five_variants(
    remaining: &[u8; TileType::COUNT],
    hand: &[u8; TileType::COUNT],
    context: &GameContext,
) -> Vec<(Vec<TileType>, u128)> {
    let red_capable: Vec<TileType> = TileType::all()
        .filter(|tile| TileId::copies(*tile).any(|id| id.is_red()))
        .filter(|tile| hand[tile.index()] > 0)
        .collect();
    let mut variants: Vec<(Vec<TileType>, u128)> = vec![(Vec::new(), 1)];
    for tile in TileType::all() {
        let count = hand[tile.index()];
        if count == 0 || red_capable.contains(&tile) {
            continue;
        }
        let factor = binomial(remaining[tile.index()], count);
        for (_, weight) in &mut variants {
            *weight *= factor;
        }
    }
    for tile in red_capable {
        let count = hand[tile.index()];
        let left = remaining[tile.index()] - 1;
        let seen = context
            .visible_tiles()
            .iter()
            .any(|id| id.is_red() && id.tile_type() == tile);
        variants = variants
            .into_iter()
            .flat_map(|(held, weight)| {
                if seen {
                    return vec![(held, weight * binomial(remaining[tile.index()], count))];
                }
                let mut with_red = held.clone();
                with_red.push(tile);
                vec![
                    (with_red, weight * binomial(left, count - 1)),
                    (held, weight * binomial(left, count)),
                ]
            })
            .collect();
    }
    variants
}

fn subsets(pool: &[TileType], size: usize) -> Vec<Vec<TileType>> {
    if size == 0 {
        return vec![Vec::new()];
    }
    let mut collected = Vec::new();
    for index in 0..pool.len() {
        for mut rest in subsets(&pool[index + 1..], size - 1) {
            let mut subset = vec![pool[index]];
            subset.append(&mut rest);
            collected.push(subset);
        }
    }
    collected
}

fn binomial(n: u8, k: u8) -> u128 {
    if k > n {
        return 0;
    }
    let mut value = 1u128;
    for step in 0..u128::from(k) {
        value = value * (u128::from(n) - step) / (step + 1);
    }
    value
}

fn evidence(discard: TileId, context: &GameContext) -> StructuralExpectedDealInLossEvidence {
    structural_expected_deal_in_loss(REACHER, discard, context).expect("an available expected loss")
}

fn ron_risk(discard: TileId, context: &GameContext) -> crate::defense::RonRiskEvidence {
    CompressedHiddenHandStates::new(REACHER, context)
        .expect("menzen reacher")
        .ron_risk_evidence(discard.tile_type())
}

// ------------------------------------------------------------------------------------------------
// tests
// ------------------------------------------------------------------------------------------------

#[test]
fn the_expected_loss_matches_a_brute_force_reference() {
    let discard = held("6m");
    let context = ReacherFixture::new(
        &[("5m", 2), ("6m", 3), ("7m", 2), ("8p", 2), ("9p", 1)],
        discard,
    )
    .context();

    let reference = reference_evidence(discard, &context);
    let evidence = evidence(discard, &context);

    assert!(reference.ron_capable_weight > 0, "{reference:?}");
    assert_eq!(
        evidence.loss_weighted_sum, reference.loss_weighted_sum,
        "{reference:?}"
    );
    assert_eq!(evidence.ura_arrangement_weight, reference.ura_arrangements);
    assert_eq!(
        evidence.tenpai_weight,
        ron_risk(discard, &context).tenpai_weight
    );
}

#[test]
fn a_single_payment_is_the_ron_risk_ratio_times_that_payment() {
    // 裏ドラ slot が無く、ロン可能 state の支払点が1種類しかない局面。
    let discard = held("6m");
    let context = ReacherFixture::new(&[("5m", 1), ("6m", 1), ("7m", 1), ("9p", 2)], discard)
        .with_dora_indicators(&[])
        .context();

    let reference = reference_evidence(discard, &context);
    assert_eq!(reference.payments.len(), 1, "{reference:?}");

    let payment = u128::from(reference.payments[0]);
    let evidence = evidence(discard, &context);
    let risk = ron_risk(discard, &context);

    assert_eq!(evidence.ura_arrangement_weight, 1);
    assert_eq!(evidence.tenpai_weight, risk.tenpai_weight);
    assert_eq!(
        evidence.loss_weighted_sum,
        risk.ron_capable_weight * payment
    );
    assert_eq!(
        evidence.expected_loss(),
        Some((risk.ron_capable_weight * payment) as f64 / risk.tenpai_weight as f64)
    );
}

#[test]
fn the_same_ron_risk_with_a_different_payment_distribution_differs() {
    // ドラ表示牌だけを変えて、同じ `R/T` のまま打点分布を変える。
    let discard = held("6m");
    let cheap = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_dora_indicators(&["9m"])
        .context();
    let expensive = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_dora_indicators(&["4m"])
        .context();

    let cheap_risk = ron_risk(discard, &cheap);
    let expensive_risk = ron_risk(discard, &expensive);
    assert_eq!(
        cheap_risk.ron_capable_weight,
        expensive_risk.ron_capable_weight
    );
    assert_eq!(cheap_risk.tenpai_weight, expensive_risk.tenpai_weight);

    let cheap_loss = evidence(discard, &cheap);
    let expensive_loss = evidence(discard, &expensive);

    assert_eq!(
        cheap_loss.compare_expected_loss(&expensive_loss),
        Some(std::cmp::Ordering::Less),
        "cheap: {cheap_loss:?}, expensive: {expensive_loss:?}"
    );
}

#[test]
fn a_genbutsu_discard_has_no_expected_loss() {
    let discard = held("C");
    let context = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_reacher_river(&["C"])
        .context();

    let risk = ron_risk(discard, &context);
    let evidence = evidence(discard, &context);

    assert_eq!(risk.ron_capable_weight, 0);
    assert_eq!(evidence.loss_weighted_sum, 0);
    assert_eq!(evidence.expected_loss(), Some(0.0));
}

#[test]
fn a_genbutsu_discard_has_no_expected_loss_without_the_scoring_facts() {
    // ロン可能 state が無い場合の期待損失は scoring 事実に依らず0なので、打点を確定できない
    // 局面でも `unavailable` にしない。
    let discard = held("C");
    let context = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_reacher_river(&["C"])
        .with_declared_double_riichi(None)
        .with_ippatsu(None)
        .with_honba(None)
        .context();

    assert_eq!(evidence(discard, &context).loss_weighted_sum, 0);
}

#[test]
fn more_weight_on_the_expensive_states_raises_the_expected_loss() {
    // ロン可能 state は「待ちブロック + 9p 雀頭」の3通りで、physical weight はどれも1。ドラは
    // 8m なので 7m8m の state だけが高打点になる。リーチ者の河でどの state がフリテンになるかを
    // 入れ替えると、`T` も `R` も同じまま高打点 state の weight だけが 0 と 1 で入れ替わる。
    let discard = held("6m");
    let fixture = || {
        ReacherFixture::new(
            &[("4m", 1), ("5m", 1), ("7m", 1), ("8m", 1), ("9p", 2)],
            discard,
        )
        .with_dora_indicators(&["7m"])
        .with_seen_red_five("5m")
    };
    // 9m は 7m8m の別待ちなので、高打点 state だけがロン不能になる。
    let without_expensive = fixture().with_reacher_river(&["C", "9m"]).context();
    // 3m は 4m5m の別待ちなので、安手 state だけがロン不能になる。
    let with_expensive = fixture().with_reacher_river(&["C", "3m"]).context();

    let cheap_risk = ron_risk(discard, &without_expensive);
    let expensive_risk = ron_risk(discard, &with_expensive);
    assert_eq!(cheap_risk, expensive_risk, "`R/T` は同じ");

    let without = evidence(discard, &without_expensive);
    let with = evidence(discard, &with_expensive);

    assert_eq!(
        without.compare_expected_loss(&with),
        Some(std::cmp::Ordering::Less),
        "without: {without:?}, with: {with:?}"
    );
}

#[test]
fn the_red_five_physical_configurations_are_averaged_by_their_weight() {
    let discard = held("6m");
    let unseen_red = ReacherFixture::new(
        &[("5m", 2), ("6m", 3), ("7m", 2), ("8p", 2), ("9p", 1)],
        discard,
    )
    .context();
    let seen_red = ReacherFixture::new(
        &[("5m", 2), ("6m", 3), ("7m", 2), ("8p", 2), ("9p", 1)],
        discard,
    )
    .with_seen_red_five("5m")
    .context();

    // 赤5がまだ見えていない局面では、5m を含む物理配置が赤入りと黒だけへ分かれる。
    let reference = reference_evidence(discard, &unseen_red);
    assert_eq!(
        evidence(discard, &unseen_red).loss_weighted_sum,
        reference.loss_weighted_sum
    );

    // 赤5が既に見え切っている局面では同じ state space でも赤ドラが付かず、期待損失が下がる。
    let unseen = evidence(discard, &unseen_red);
    let seen = evidence(discard, &seen_red);
    assert_eq!(
        ron_risk(discard, &unseen_red).ron_capable_weight,
        ron_risk(discard, &seen_red).ron_capable_weight
    );
    assert_eq!(
        seen.compare_expected_loss(&unseen),
        Some(std::cmp::Ordering::Less),
        "seen: {seen:?}, unseen: {unseen:?}"
    );
    assert_eq!(
        evidence(discard, &seen_red).loss_weighted_sum,
        reference_evidence(discard, &seen_red).loss_weighted_sum
    );
}

#[test]
fn a_red_five_discard_is_scored_as_a_red_five() {
    let black = held("5m");
    let red_five = red("5m");
    let black_context =
        ReacherFixture::new(&[("4m", 2), ("5m", 3), ("6m", 2), ("9p", 2)], black).context();
    let red_context =
        ReacherFixture::new(&[("4m", 2), ("5m", 3), ("6m", 2), ("9p", 2)], red_five).context();

    let black_risk = ron_risk(black, &black_context);
    let red_risk = ron_risk(red_five, &red_context);
    assert_eq!(black_risk, red_risk, "赤5と黒5で `R/T` は同じ");

    let black_loss = evidence(black, &black_context);
    let red_loss = evidence(red_five, &red_context);

    assert_eq!(
        black_loss.compare_expected_loss(&red_loss),
        Some(std::cmp::Ordering::Less),
        "black: {black_loss:?}, red: {red_loss:?}"
    );
    assert_eq!(
        red_loss.loss_weighted_sum,
        reference_evidence(red_five, &red_context).loss_weighted_sum
    );
}

#[test]
fn the_ura_dora_indicators_are_averaged_over_every_unseen_arrangement() {
    let discard = held("6m");
    for indicators in [vec!["9m"], vec!["9m", "1p"]] {
        let context = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 2)], discard)
            .with_dora_indicators(&indicators)
            .context();

        let reference = reference_evidence(discard, &context);
        let evidence = evidence(discard, &context);

        assert!(reference.payments.len() > 1, "{reference:?}");
        assert_eq!(
            evidence.ura_arrangement_weight, reference.ura_arrangements,
            "indicators: {indicators:?}"
        );
        assert_eq!(
            evidence.loss_weighted_sum, reference.loss_weighted_sum,
            "indicators: {indicators:?}"
        );
    }
}

#[test]
fn the_honba_is_paid_by_the_discarder_but_the_kyotaku_is_not() {
    let discard = held("6m");
    let fixture = || ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard);
    let without_honba = fixture().with_honba(Some(0)).context();
    let with_honba = fixture().with_honba(Some(2)).context();

    let risk = ron_risk(discard, &without_honba);
    let base = evidence(discard, &without_honba);
    let settled = evidence(discard, &with_honba);

    // 本場は state ごとに一定額なので、期待損失の増分は `R/T × 本場点` になる。
    assert_eq!(base.tenpai_weight, settled.tenpai_weight);
    assert_eq!(
        settled.loss_weighted_sum - base.loss_weighted_sum,
        risk.ron_capable_weight * base.ura_arrangement_weight * 600
    );

    // 供託は同じ 3000 点を置いたままで、期待損失には入っていない。
    assert_eq!(without_honba.kyotaku_points(), Some(3000));
    assert!(base.loss_weighted_sum > 0);
}

#[test]
fn an_unknown_riichi_kind_leaves_the_expected_loss_unavailable() {
    let discard = held("6m");
    let context = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_declared_double_riichi(None)
        .context();

    assert_eq!(
        structural_expected_deal_in_loss(REACHER, discard, &context),
        Err(StructuralExpectedDealInLossUnavailable::UnknownDoubleRiichi)
    );
}

#[test]
fn an_unknown_ippatsu_leaves_the_expected_loss_unavailable() {
    let discard = held("6m");
    let context = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_ippatsu(None)
        .context();

    assert_eq!(
        structural_expected_deal_in_loss(REACHER, discard, &context),
        Err(StructuralExpectedDealInLossUnavailable::UnknownIppatsu)
    );
}

#[test]
fn an_unknown_honba_leaves_the_expected_loss_unavailable() {
    let discard = held("6m");
    let context = ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard)
        .with_honba(None)
        .context();

    assert!(matches!(
        structural_expected_deal_in_loss(REACHER, discard, &context),
        Err(StructuralExpectedDealInLossUnavailable::Settlement(_))
    ));
}

#[test]
fn a_double_riichi_costs_more_than_a_riichi() {
    let discard = held("6m");
    let fixture = || ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard);
    let riichi = evidence(discard, &fixture().context());
    let double_riichi = evidence(
        discard,
        &fixture().with_declared_double_riichi(Some(true)).context(),
    );

    assert_eq!(
        riichi.compare_expected_loss(&double_riichi),
        Some(std::cmp::Ordering::Less),
        "riichi: {riichi:?}, double riichi: {double_riichi:?}"
    );
}

#[test]
fn an_ippatsu_costs_more_than_a_plain_riichi() {
    let discard = held("6m");
    let fixture = || ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard);
    let without = evidence(discard, &fixture().context());
    let with = evidence(discard, &fixture().with_ippatsu(Some(true)).context());

    assert_eq!(
        without.compare_expected_loss(&with),
        Some(std::cmp::Ordering::Less),
        "without: {without:?}, with: {with:?}"
    );
}

#[test]
fn a_dealer_reacher_costs_more_than_a_non_dealer_reacher() {
    let discard = held("6m");
    let fixture = || ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard);
    let non_dealer = evidence(discard, &fixture().context());
    let dealer = evidence(discard, &fixture().with_oya(REACHER as u8).context());

    assert_eq!(
        non_dealer.compare_expected_loss(&dealer),
        Some(std::cmp::Ordering::Less),
        "non dealer: {non_dealer:?}, dealer: {dealer:?}"
    );
}

#[test]
fn the_metrics_count_the_enumerated_states_and_scoring_evaluations() {
    let discard = held("6m");
    let context =
        ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard).context();

    let mut metrics = StructuralExpectedDealInLossMetrics::default();
    structural_expected_deal_in_loss_with_metrics(REACHER, discard, &context, &mut metrics)
        .expect("an available expected loss");

    assert!(metrics.ron_capable_states > 0, "{metrics:?}");
    assert!(
        metrics.red_five_variants >= metrics.ron_capable_states,
        "{metrics:?}"
    );
    assert!(
        metrics.scoring_evaluations >= metrics.red_five_variants,
        "{metrics:?}"
    );
}

#[test]
fn the_evidence_comparison_never_guesses_an_unavailable_ratio() {
    let zero_denominator = StructuralExpectedDealInLossEvidence {
        loss_weighted_sum: 1,
        tenpai_weight: 0,
        ura_arrangement_weight: 1,
    };
    let known = StructuralExpectedDealInLossEvidence {
        loss_weighted_sum: 1,
        tenpai_weight: 2,
        ura_arrangement_weight: 1,
    };

    assert_eq!(zero_denominator.weight_denominator(), None);
    assert_eq!(zero_denominator.expected_loss(), None);
    assert_eq!(zero_denominator.compare_expected_loss(&known), None);
    assert_eq!(known.compare_expected_loss(&zero_denominator), None);

    let overflowing = StructuralExpectedDealInLossEvidence {
        loss_weighted_sum: u128::MAX,
        tenpai_weight: u128::MAX,
        ura_arrangement_weight: 2,
    };
    assert_eq!(overflowing.weight_denominator(), None);
    assert_eq!(overflowing.compare_expected_loss(&known), None);
}

#[test]
fn visiting_the_ron_capable_states_counts_the_same_weight_as_the_existing_r() {
    // 期待損失は既存 `R` の数え上げをそのまま観測する。観測を挿しても weight も state 数も
    // 変わらず、compressed production model の `R` とも一致する。
    let discard = held("6m");
    let context =
        ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard).context();
    let target = discard.tile_type();

    let counted = ReachedHiddenHandStates::new(REACHER, &context)
        .expect("menzen reacher")
        .ron_capable_state_weight(target);
    let mut visited = Vec::new();
    let observed = ReachedHiddenHandStates::new(REACHER, &context)
        .expect("menzen reacher")
        .visit_ron_capable_states(target, &mut |hand, weight| {
            visited.push((*hand, weight));
        });

    assert_eq!(observed, counted);
    assert_eq!(visited.len() as u64, counted.states);
    assert_eq!(
        visited.iter().map(|(_, weight)| weight).sum::<u128>(),
        counted.weight
    );
    assert_eq!(
        counted.weight,
        ron_risk(discard, &context).ron_capable_weight
    );

    // 同じ hidden-hand state が2回観測されることはない。
    let mut unique: Vec<_> = visited.iter().map(|(hand, _)| *hand).collect();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), visited.len());
}

#[test]
fn a_non_reached_player_has_no_expected_loss() {
    let discard = held("6m");
    let context =
        ReacherFixture::new(&[("5m", 2), ("6m", 3), ("7m", 2), ("9p", 1)], discard).context();

    assert!(matches!(
        structural_expected_deal_in_loss(2, discard, &context),
        Err(StructuralExpectedDealInLossUnavailable::HiddenHandModel(_))
    ));
}
