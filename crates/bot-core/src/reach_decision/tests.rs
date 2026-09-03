use super::*;

use crate::action::LegalAction;
use crate::agent::Agent;
use crate::agents::{AgentActionSource, ShantenAgent};
use crate::context::{GameContext, TableStateFacts};
use crate::damaten_value::{
    DAMATEN_MIN_TOTAL, DamatenValue, DamatenValueDiagnostic, DamatenValueVerdict,
    damaten_baseline_context,
};
use crate::defense::{SujiSafetyRank, WallRank};
use crate::discard_selection::{select_best_normal_discard_evaluation, select_discard_action};
use crate::push_pull::PushPullMode;
use crate::reach_policy::{
    REACH_MIN_REMAINING, ReachDecisionReason, ReachTimingDecision, ReachTimingDiagnostic,
    ReachTimingReason,
};
use crate::ron_opportunity::reach_public_safety_after_discard;
use crate::shanten_diagnostic::{
    DecisionDiagnostics, DiagnosticOptions, ShantenDecisionDiagnostic,
};
use crate::shanten_test_support::{
    TENPAI_DRAWN, TENPAI_HAND, TENPAI_SCARCE_VISIBLE, dahai, fold_actions,
    fold_under_reach_context, tenpai_actions, tenpai_context, tile, weak_tenpai_actions,
    weak_tenpai_under_reach_context,
};
use crate::tenpai_continuation::{
    TenpaiSelfTsumoComparison, selected_tenpai_self_tsumo_comparison,
};
use bot_logic::{HistoryFuritenFacts, PermanentFuriten, RiichiStatus, TileId, TileType, WinMethod};

fn diagnose_matching_act(ctx: &GameContext, actions: &[LegalAction]) -> ShantenDecisionDiagnostic {
    let mut agent = ShantenAgent;
    let expected = agent.act(ctx, actions);
    let diagnostic = ShantenAgent::diagnose(ctx, actions);
    assert_eq!(diagnostic.selected_action, expected);
    diagnostic
}

fn three_sided_hand() -> Vec<TileId> {
    [0u8, 4, 8, 12, 17, 20, 80, 84, 89, 92, 96, 100, 104]
        .iter()
        .map(|&value| tile(value))
        .collect()
}

fn three_sided_context(extra_visible: &[u8], own_river: &[u8]) -> GameContext {
    three_sided_context_with_player_id(Some(0), extra_visible, own_river)
}

// player 0 の河は同じまま、自分の席だけを特定できない局面。
fn three_sided_context_without_player_id(own_river: &[u8]) -> GameContext {
    three_sided_context_with_player_id(None, &[], own_river)
}

fn three_sided_context_with_player_id(
    player_id: Option<u8>,
    extra_visible: &[u8],
    own_river: &[u8],
) -> GameContext {
    let hand = three_sided_hand();
    let drawn = tile(36);
    let discards = [
        own_river.iter().map(|&value| tile(value)).collect(),
        vec![],
        vec![],
        vec![],
    ];

    let mut visible = hand.clone();
    visible.push(drawn);
    visible.extend(extra_visible.iter().map(|&value| tile(value)));
    for river in &discards {
        visible.extend(river.iter().copied());
    }

    GameContext::from_parts_with_table_state(
        Some(drawn),
        hand,
        vec![],
        None,
        None,
        visible,
        player_id,
        Some(0),
        discards,
        [false; 4],
    )
}

fn three_sided_actions() -> Vec<LegalAction> {
    vec![dahai(36), dahai(0)]
}

// 同じテンパイ手牌で visible tiles だけを空にした局面。
fn tenpai_context_without_visible_tiles() -> GameContext {
    let hand: Vec<_> = TENPAI_HAND.iter().map(|&value| tile(value)).collect();
    GameContext::from_parts(Some(tile(TENPAI_DRAWN)), hand)
}

// 123456789m 123p 5s + ツモ 北。打 北 で 5s 単騎テンパイになり、待ちは3枚だけ。
//
// 14枚をそのまま評価すると受け入れは {5s, 北} の6枚に見えるが、実際に切る打牌を決めた後の
// 待ちは3枚しかない。旧判断と新判断で結論が変わる代表局面。
const TANKI_TENPAI_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 44, 89];
const TANKI_TENPAI_DRAWN: u8 = 116;

fn tanki_tenpai_context() -> GameContext {
    let hand: Vec<_> = TANKI_TENPAI_HAND.iter().map(|&value| tile(value)).collect();
    GameContext::from_parts(Some(tile(TANKI_TENPAI_DRAWN)), hand)
}

// 5s を1枚、北 を2枚見せて、打 北 の 5s 単騎を2枚待ちにした同じ単騎テンパイ。打 5s の
// 北 単騎は1枚しか残らないので、選ばれる打牌は 北 のまま。
const TANKI_TENPAI_SCARCE_VISIBLE: [u8; 3] = [88, 117, 118];

fn tanki_tenpai_context_with_visible(extra_visible: &[u8]) -> GameContext {
    let hand: Vec<_> = TANKI_TENPAI_HAND.iter().map(|&value| tile(value)).collect();
    let mut visible = hand.clone();
    visible.push(tile(TANKI_TENPAI_DRAWN));
    visible.extend(extra_visible.iter().map(|&value| tile(value)));
    GameContext::from_parts_with_visible_tiles(
        Some(tile(TANKI_TENPAI_DRAWN)),
        hand,
        vec![],
        None,
        None,
        visible,
    )
}

fn tanki_tenpai_actions() -> Vec<LegalAction> {
    TANKI_TENPAI_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(TANKI_TENPAI_DRAWN)])
        .chain([LegalAction::Reach])
        .collect()
}

// 114477m 114477p + 1s + ツモ E。どちらの孤立牌を切っても七対子単騎テンパイになり、
// 生き枚数が同じなので待ち牌の品質で打牌が決まる。
const CHIITOITSU_TANKI_HAND: [u8; 13] = [0, 1, 12, 13, 24, 25, 36, 37, 48, 49, 60, 61, 72];
const CHIITOITSU_TANKI_DRAWN: u8 = 108;
const CHIITOITSU_TANKI_DISCARD: u8 = 72;

fn chiitoitsu_tanki_context() -> GameContext {
    let hand: Vec<_> = CHIITOITSU_TANKI_HAND
        .iter()
        .map(|&value| tile(value))
        .collect();
    GameContext::from_parts(Some(tile(CHIITOITSU_TANKI_DRAWN)), hand)
}

fn chiitoitsu_tanki_dahai_actions() -> Vec<LegalAction> {
    CHIITOITSU_TANKI_HAND
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(CHIITOITSU_TANKI_DRAWN)])
        .collect()
}

#[test]

fn reaches_on_a_chiitoitsu_tanki_wait() {
    // 七対子単騎は生牌でも3枚。選んだ打牌後の待ちで先制リーチする。
    let ctx = chiitoitsu_tanki_context();
    let actions: Vec<LegalAction> = chiitoitsu_tanki_dahai_actions()
        .into_iter()
        .chain([LegalAction::Reach])
        .collect();
    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let reach = diagnostic.reach.as_ref().expect("リーチを検討している");

    assert_eq!(
        reach.selected_discard,
        Some(dahai(CHIITOITSU_TANKI_DISCARD))
    );
    assert_eq!(reach.tsumo_remaining(), Some(3));
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert_eq!(diagnostic.selected_action, LegalAction::Reach);
}

// act() が実際に通ったリーチ判断。診断専用に判断し直さないことを毎回確かめる。
fn reach_diagnostic(ctx: &GameContext, actions: &[LegalAction]) -> ReachDecisionDiagnostic {
    let diagnostic = diagnose_matching_act(ctx, actions);
    let reach = diagnostic.reach.expect("リーチを検討している");
    assert_eq!(
        reach.should_reach(),
        diagnostic.selected_source == AgentActionSource::Reach
    );
    reach
}

#[test]
fn reaches_when_visible_waits_are_plentiful() {
    let mut agent = ShantenAgent;
    let ctx = tenpai_context(&[]);
    assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
}

#[test]
fn skips_reach_when_visible_waits_are_scarce() {
    let mut agent = ShantenAgent;
    let ctx = tenpai_context(&TENPAI_SCARCE_VISIBLE);
    let selected = agent.act(&ctx, &tenpai_actions());
    assert!(matches!(selected, LegalAction::Dahai { .. }));
}

#[test]
fn reaches_when_visible_tiles_empty_even_with_hand() {
    // visible tiles が空でも「空だから無条件にリーチ」ではなく、選んだ打牌の受け入れで
    // 判断する。この手牌は見え牌補正が無くても待ちが8枚あるのでリーチになる。
    let mut agent = ShantenAgent;
    let ctx = tenpai_context_without_visible_tiles();
    assert!(ctx.visible_tiles().is_empty());
    assert_eq!(agent.act(&ctx, &tenpai_actions()), LegalAction::Reach);
}

#[test]
fn does_not_reach_without_hand_information() {
    // 手牌が無く通常打牌 selection が打牌を選べない局面では、リーチ専用の fallback で
    // 無条件にリーチしない。
    let mut agent = ShantenAgent;
    let ctx = GameContext::default();
    let actions = vec![LegalAction::Reach];

    assert_ne!(agent.act(&ctx, &actions), LegalAction::Reach);
    let reach = ShantenAgent::diagnose(&ctx, &actions)
        .reach
        .expect("Push mode でリーチを検討する");
    assert!(!reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::NoSelectedDiscard);
    assert_eq!(reach.selected_discard, None);
    assert_eq!(reach.shanten_after_discard, None);
    assert_eq!(reach.tenpai_wait, None);
}

// ---- リーチ判断 ----

#[test]
fn reach_uses_the_wait_of_the_selected_discard() {
    // 選んだ打牌後がテンパイで待ちが threshold 以上なら Push mode でリーチする。
    let ctx = tenpai_context(&[]);
    let actions = tenpai_actions();
    let reach = reach_diagnostic(&ctx, &actions);

    assert!(reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert_eq!(reach.selected, Some(LegalAction::Reach));
    assert_eq!(reach.selected_discard, Some(dahai(TENPAI_DRAWN)));
    assert_eq!(reach.shanten_after_discard, Some(0));
    assert_eq!(reach.tsumo_remaining(), Some(8));
    assert_eq!(reach.tsumo_type_count(), Some(2));
}

#[test]
fn does_not_reach_when_the_live_wait_is_below_the_threshold() {
    // テンパイでも待ち枚数が threshold 未満なら、リーチせず通常打牌へ進む。
    let ctx = tenpai_context(&TENPAI_SCARCE_VISIBLE);
    let actions = tenpai_actions();
    let normal = select_discard_action(&ctx, &actions).expect("通常打牌を選べる");
    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let reach = diagnostic.reach.as_ref().expect("リーチを検討している");

    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
    assert_eq!(diagnostic.selected_action, normal);
    assert!(!reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::InsufficientLiveWait);
    assert_eq!(reach.shanten_after_discard, Some(0));
    assert!(reach.tsumo_remaining().expect("テンパイ") < REACH_MIN_REMAINING);
}

#[test]
fn does_not_reach_when_the_selected_discard_is_not_tenpai() {
    // 合法手にリーチがあっても、選んだ打牌後がテンパイでなければ選ばない。
    let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 56, 89];
    let ctx = GameContext::from_parts(
        Some(tile(116)),
        hand_values.iter().map(|&value| tile(value)).collect(),
    );
    let actions: Vec<LegalAction> = hand_values
        .iter()
        .map(|&value| dahai(value))
        .chain([dahai(116), LegalAction::Reach])
        .collect();
    let reach = reach_diagnostic(&ctx, &actions);

    assert!(!reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::NotTenpai);
    assert!(reach.shanten_after_discard.expect("打牌を選べる") > 0);
    assert_eq!(reach.tenpai_wait, None);
}

#[test]
fn does_not_reach_without_a_legal_reach() {
    // 合法手にリーチが無ければ、テンパイでもリーチを選ばず待ちも求めない。
    let ctx = tenpai_context(&[]);
    let actions: Vec<LegalAction> = tenpai_actions()
        .into_iter()
        .filter(|action| !matches!(action, LegalAction::Reach))
        .collect();
    let reach = reach_diagnostic(&ctx, &actions);

    assert!(!reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::NoLegalReach);
    assert_eq!(reach.tenpai_wait, None);
}

#[test]
fn reach_decision_is_shared_by_act_and_every_diagnose_entry_point() {
    for (ctx, actions) in [
        (tenpai_context(&[]), tenpai_actions()),
        (tenpai_context(&TENPAI_SCARCE_VISIBLE), tenpai_actions()),
        (tanki_tenpai_context(), tanki_tenpai_actions()),
        (three_sided_context(&[], &[81]), {
            three_sided_actions()
                .into_iter()
                .chain([LegalAction::Reach])
                .collect()
        }),
    ] {
        let mut agent = ShantenAgent;
        let acted = agent.act(&ctx, &actions);
        let diagnostic = ShantenAgent::diagnose(&ctx, &actions);
        let with_lookahead =
            ShantenAgent::diagnose_with_options(&ctx, &actions, DiagnosticOptions::WITH_LOOKAHEAD);

        assert_eq!(diagnostic.selected_action, acted);
        assert_eq!(with_lookahead.selected_action, acted);
        assert_eq!(with_lookahead.reach, diagnostic.reach);
    }
}

#[test]
fn reach_evaluation_does_not_build_the_analysis_diagnostics() {
    let ctx = tenpai_context(&[]);
    let mut diagnostics = DecisionDiagnostics::disabled();
    let decision = ShantenAgent.decide_with_diagnostics(&ctx, &tenpai_actions(), &mut diagnostics);

    assert_eq!(decision.action, LegalAction::Reach);
    assert!(diagnostics.normal_discard.is_none());
    assert!(diagnostics.normal_discard_furiten.is_none());
    assert!(diagnostics.normal_discard_lookahead.is_none());
    assert!(
        decision
            .reach
            .expect("リーチを検討している")
            .tenpai_wait
            .is_some()
    );
}

#[test]
fn fold_does_not_evaluate_reach() {
    for (ctx, actions) in [
        (weak_tenpai_under_reach_context(), weak_tenpai_actions()),
        (fold_under_reach_context(), fold_actions()),
    ] {
        let diagnostic = diagnose_matching_act(&ctx, &actions);
        assert_eq!(
            diagnostic.push_pull_decision.map(|decision| decision.mode),
            Some(PushPullMode::Fold)
        );
        assert_eq!(diagnostic.reach, None);
        assert_ne!(diagnostic.selected_action, LegalAction::Reach);
        assert_ne!(diagnostic.selected_source, AgentActionSource::Reach);
    }
}

// ---- ダマ打点によるリーチ判断 ----

// ダマ打点を判断できる局面の組み立て。場風・自風・自分の河・履歴依存フリテンをすべて既知に
// して、ダマでロンできる (`can_ron() == Some(true)`) 通常ケースを作る。自分は子 (南家) で、
// 他家リーチが無いので押し引きは Push になる。
struct DamatenCase {
    ctx: GameContext,
    actions: Vec<LegalAction>,
    // ツモ切りしてテンパイを保つ打牌。通常打牌 selection が選ぶはずの action。
    tsumogiri: LegalAction,
}

impl DamatenCase {
    // act() が実際に通ったリーチ判断。診断専用に判断し直していないことも同時に確かめる。
    fn reach(&self) -> ReachDecisionDiagnostic {
        reach_diagnostic(&self.ctx, &self.actions)
    }

    fn act(&self) -> LegalAction {
        ShantenAgent.act(&self.ctx, &self.actions)
    }

    fn damaten(&self) -> DamatenValueDiagnostic {
        self.reach().damaten_value.expect("ダマ打点を評価している")
    }

    // 和了牌の物理牌ごとの「牌・残枚数・ダマの支払い合計」を待ちの順に並べたもの。
    fn damaten_totals(&self) -> Vec<(String, u8, Option<u32>)> {
        self.damaten()
            .winning_tile_values()
            .map(|value| {
                (
                    value.winning_tile.to_mjai_string(),
                    value.remaining,
                    value.value.total(),
                )
            })
            .collect()
    }

    fn damaten_values(&self) -> Vec<DamatenValue> {
        self.damaten()
            .winning_tile_values()
            .map(|value| value.value)
            .collect()
    }
}

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

    fn tile(&mut self, s: &str) -> TileId {
        let tile_type = TileType::from_mjai_type_str(s.trim_end_matches('r')).unwrap();
        let red = s.ends_with('r');
        let id = TileId::copies(tile_type)
            .find(|id| id.is_red() == red && !self.used[id.index()])
            .expect("同じ物理牌を使い回していない");
        self.used[id.index()] = true;
        id
    }
}

// ダマ打点を判断できる局面の材料。既定は場風・自風も自分の席も既知で、`own_river` を持たない
// 非フリテンのテンパイになる。
struct DamatenSpec<'a> {
    hand: &'a [&'a str],
    drawn: &'a str,
    dora_indicators: &'a [&'a str],
    extra_visible: &'a [&'a str],
    own_river: &'a [&'a str],
    /// 場風・自風が既知か。不明にすると点数計算の入力が足りない局面になる。
    known_winds: bool,
    /// 自分の席が既知か。不明にすると自分の河を特定できず、恒常フリテンが
    /// [`PermanentFuriten::Unknown`] になる。
    known_seat: bool,
}

impl Default for DamatenSpec<'_> {
    fn default() -> Self {
        Self {
            hand: &PINFU_TANYAO_HAND,
            drawn: "N",
            dora_indicators: &[],
            extra_visible: &[],
            own_river: &[],
            known_winds: true,
            known_seat: true,
        }
    }
}

impl DamatenSpec<'_> {
    fn build(self) -> DamatenCase {
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(self.hand);
        let drawn_tile = source.tile(self.drawn);
        let dora_indicators = source.tiles(self.dora_indicators);
        let extra_visible = source.tiles(self.extra_visible);
        let own_river = source.tiles(self.own_river);

        let visible: Vec<TileId> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .chain(dora_indicators.iter())
            .chain(extra_visible.iter())
            .chain(own_river.iter())
            .copied()
            .collect();
        let actions: Vec<LegalAction> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .map(|&tile| LegalAction::Dahai { tile })
            .chain([LegalAction::Reach])
            .collect();

        let ctx = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            dora_indicators,
            self.known_winds
                .then(|| TileType::from_mjai_type_str("E").unwrap()),
            self.known_winds
                .then(|| TileType::from_mjai_type_str("S").unwrap()),
            visible,
            self.known_seat.then_some(0),
            Some(3),
            [own_river, vec![], vec![], vec![]],
            [false; 4],
        )
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });

        DamatenCase {
            ctx,
            actions,
            tsumogiri: LegalAction::Dahai { tile: drawn_tile },
        }
    }
}

fn damaten_case(hand: &[&str], drawn: &str, dora_indicators: &[&str]) -> DamatenCase {
    damaten_case_with(hand, drawn, dora_indicators, &[], &[])
}

fn damaten_case_with(
    hand: &[&str],
    drawn: &str,
    dora_indicators: &[&str],
    extra_visible: &[&str],
    own_river: &[&str],
) -> DamatenCase {
    DamatenSpec {
        hand,
        drawn,
        dora_indicators,
        extra_visible,
        own_river,
        ..DamatenSpec::default()
    }
    .build()
}

// 平和 + 断幺の 3s / 6s 両面テンパイ。ドラ表示牌だけを変えてダマ打点の階段を作る。
// 3s であがると一盃口が付くため、同じ手牌でも待ちごとに打点が違う。
const PINFU_TANYAO_HAND: [&str; 13] = [
    "2m", "3m", "4m", "6m", "7m", "8m", "2p", "2p", "3s", "4s", "5s", "4s", "5s",
];

// 3s を全て見せて 6s の1種待ちにするための見え牌。待ちごとの打点差を消し、ダマ打点の
// threshold そのものを確かめるために使う。
const PINFU_TANYAO_SINGLE_WAIT_VISIBLE: [&str; 3] = ["3s", "3s", "3s"];

// 3p 嵌張の役なしテンパイ。9s の対子で断幺が消え、嵌張なので平和も付かない。
const NO_YAKU_HAND: [&str; 13] = [
    "2m", "3m", "4m", "4m", "5m", "6m", "6m", "7m", "8m", "2p", "4p", "9s", "9s",
];

// 断幺の 5s 単騎テンパイ。赤5と黒5で打点が変わる待ちを作る。
const RED_FIVE_TANKI_HAND: [&str; 13] = [
    "2m", "3m", "4m", "6m", "7m", "8m", "2p", "3p", "4p", "6p", "7p", "8p", "5s",
];

// 2s 単騎の七対子テンパイ。9p の対子で断幺が消え、2m の3枚目をツモ切りしてテンパイを保つ。
const CHIITOITSU_HAND: [&str; 13] = [
    "2m", "2m", "5m", "5m", "8m", "8m", "2p", "2p", "5p", "5p", "9p", "9p", "2s",
];

// 13面待ちの国士無双テンパイ。
const KOKUSHI_HAND: [&str; 13] = [
    "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
];

#[test]
fn damaten_baseline_is_a_hypothetical_ron_context() {
    // 打点比較の baseline は局面から推測せず policy が決める。場風・自風だけを既知 fact から
    // 取り、リーチ後の裏ドラや海底が付かない状況として組み立てる。
    let case = damaten_case(&PINFU_TANYAO_HAND, "N", &[]);
    let baseline = damaten_baseline_context(&case.ctx);

    assert_eq!(baseline, case.damaten().baseline);
    assert_eq!(baseline.win_method(), WinMethod::Ron);
    assert_eq!(baseline.riichi(), RiichiStatus::NotDeclared);
    assert_eq!(baseline.chankan(), Some(false));
    // 海底 / 河底を付けないための policy input で、実際の山残枚数ではない。
    assert_eq!(baseline.remaining_live_tiles(), Some(1));
    assert!(!baseline.is_last_live_tile());
    assert_eq!(baseline.round_wind(), case.ctx.round_wind());
    assert_eq!(baseline.seat_wind(), case.ctx.seat_wind());
    assert_eq!(baseline.ippatsu(), None);
}

#[test]
fn reaches_when_the_damaten_hand_has_no_yaku() {
    // ダマでは役が無く、そもそもロンできない待ちならリーチする。
    let case = damaten_case(&NO_YAKU_HAND, "N", &[]);
    let reach = case.reach();

    assert_eq!(case.damaten_values(), [DamatenValue::NoYaku]);
    assert_eq!(reach.damaten_verdict(), Some(DamatenValueVerdict::NoYaku));
    assert_eq!(reach.reason, ReachDecisionReason::EligibleNoDamatenYaku);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn reaches_when_the_damaten_value_is_below_the_threshold() {
    // ダマ 3900 は threshold 未満なのでリーチする。
    let case = damaten_case_with(
        &PINFU_TANYAO_HAND,
        "N",
        &["1m"],
        &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
        &[],
    );
    let reach = case.reach();

    assert_eq!(case.damaten_totals(), [("6s".to_string(), 4, Some(3900))]);
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::BelowThreshold)
    );
    assert_eq!(reach.reason, ReachDecisionReason::EligibleLowValue);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn reaches_just_below_the_damaten_threshold() {
    // threshold 直下のダマ 6400 でもリーチする。7700 は inclusive で、そこに届かない実点数は
    // すべてリーチ側。
    let case = damaten_case(&CHIITOITSU_HAND, "2m", &["1m"]);
    let reach = case.reach();

    assert_eq!(case.damaten_totals(), [("2s".to_string(), 3, Some(6400))]);
    assert!(case.damaten_totals()[0].2.unwrap() < DAMATEN_MIN_TOTAL);
    assert_eq!(reach.reason, ReachDecisionReason::EligibleLowValue);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn stays_damaten_at_the_threshold() {
    // threshold ちょうどのダマ 7700 はダマにする。7700 は inclusive。
    let case = damaten_case_with(
        &PINFU_TANYAO_HAND,
        "N",
        &["1p"],
        &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
        &[],
    );
    let reach = case.reach();

    assert_eq!(
        case.damaten_totals(),
        [("6s".to_string(), 4, Some(DAMATEN_MIN_TOTAL))]
    );
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::AboveThreshold)
    );
    assert_eq!(reach.reason, ReachDecisionReason::HighValueDamaten);
    assert!(!reach.should_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn stays_damaten_above_the_threshold() {
    // ダマ 8000 はダマにする。
    let case = damaten_case_with(
        &PINFU_TANYAO_HAND,
        "N",
        &["1p", "1m"],
        &PINFU_TANYAO_SINGLE_WAIT_VISIBLE,
        &[],
    );
    let reach = case.reach();

    assert_eq!(case.damaten_totals(), [("6s".to_string(), 4, Some(8000))]);
    assert_eq!(reach.reason, ReachDecisionReason::HighValueDamaten);
    assert!(!reach.should_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn stays_damaten_on_a_named_yakuman() {
    // 名前の付いた役満はダマにする。13面待ちでも全ての待ちが役満なので判断は変わらない。
    let case = damaten_case(&KOKUSHI_HAND, "5m", &[]);
    let reach = case.reach();

    assert_eq!(case.damaten_totals().len(), 13);
    assert!(
        case.damaten_values()
            .iter()
            .all(|value| value.is_yakuman() && value.meets_threshold() == Some(true))
    );
    assert_eq!(reach.reason, ReachDecisionReason::HighValueDamaten);
    assert!(!reach.should_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn reaches_when_a_mixed_wait_is_below_the_threshold() {
    // 待ちごとに打点が違う場合、平均や期待値を取らずに1つでも threshold 未満ならリーチする。
    let case = damaten_case(&PINFU_TANYAO_HAND, "N", &["1m"]);
    let reach = case.reach();

    assert_eq!(
        case.damaten_totals(),
        [
            ("3s".to_string(), 3, Some(7700)),
            ("6s".to_string(), 4, Some(3900)),
        ]
    );
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::BelowThreshold)
    );
    assert_eq!(reach.reason, ReachDecisionReason::EligibleLowValue);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn stays_damaten_when_every_mixed_wait_reaches_the_threshold() {
    // 打点が違っても全ての待ちが threshold 以上ならダマにする。
    let case = damaten_case(&PINFU_TANYAO_HAND, "N", &["1p"]);
    let reach = case.reach();

    assert_eq!(
        case.damaten_totals(),
        [
            ("3s".to_string(), 3, Some(8000)),
            ("6s".to_string(), 4, Some(7700)),
        ]
    );
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::AboveThreshold)
    );
    assert_eq!(reach.reason, ReachDecisionReason::HighValueDamaten);
    assert!(!reach.should_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn reaches_when_only_the_black_five_is_below_the_threshold() {
    // 赤5と黒5は別 variant として同じ規則を適用する。黒であがると threshold 未満なので、
    // 赤であがれば足りていてもリーチする。
    let case = damaten_case(&RED_FIVE_TANKI_HAND, "N", &["1m", "1p"]);
    let reach = case.reach();

    assert_eq!(
        case.damaten_totals(),
        [
            ("5sr".to_string(), 1, Some(8000)),
            ("5s".to_string(), 2, Some(5200)),
        ]
    );
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::BelowThreshold)
    );
    assert_eq!(reach.reason, ReachDecisionReason::EligibleLowValue);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn reaches_on_a_scarce_wait_below_the_damaten_threshold() {
    // 待ち枚数の threshold をダマ打点より先に適用しない。1～2枚待ちでもダマが安ければリーチ
    // する。
    let case = damaten_case_with(
        &PINFU_TANYAO_HAND,
        "N",
        &[],
        &["3s", "3s", "6s", "6s", "6s"],
        &[],
    );
    let reach = case.reach();

    assert_eq!(reach.tsumo_remaining(), Some(2));
    assert!(reach.tsumo_remaining().expect("テンパイ") < REACH_MIN_REMAINING);
    assert_eq!(
        case.damaten_totals(),
        [
            ("3s".to_string(), 1, Some(3900)),
            ("6s".to_string(), 1, Some(2000)),
        ]
    );
    assert_eq!(reach.reason, ReachDecisionReason::EligibleLowValue);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn does_not_reach_without_a_live_wait() {
    // 待ちが1枚も残っていなければ、ダマ打点を問わずリーチしない。
    let case = damaten_case_with(
        &PINFU_TANYAO_HAND,
        "N",
        &["1p"],
        &["3s", "3s", "3s", "6s", "6s", "6s", "6s"],
        &[],
    );
    let reach = case.reach();

    assert_eq!(reach.shanten_after_discard, Some(0));
    assert_eq!(reach.tsumo_remaining(), Some(0));
    assert_eq!(case.damaten_totals(), []);
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::NoLiveWait)
    );
    assert_eq!(reach.reason, ReachDecisionReason::NoLiveWait);
    assert!(!reach.should_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn furiten_keeps_the_existing_reach_policy() {
    // フリテンではダマ打点の policy を適用せず、待ち枚数だけを見る既存判断を維持する。
    // 同じ手牌・同じドラでも、フリテンでなければダマにする打点である。
    let case = damaten_case_with(&PINFU_TANYAO_HAND, "N", &["1p"], &[], &["6s"]);
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.can_ron(), Some(false));
    assert_eq!(reach.damaten_value, None);
    assert_eq!(reach.damaten_verdict(), None);
    assert!(!reach.used_damaten_value());
    assert!(reach.tsumo_remaining().expect("テンパイ") >= REACH_MIN_REMAINING);
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);

    let not_furiten = damaten_case(&PINFU_TANYAO_HAND, "N", &["1p"]);
    assert_eq!(
        not_furiten.reach().reason,
        ReachDecisionReason::HighValueDamaten
    );
}

#[test]
fn an_unknown_ron_availability_keeps_the_existing_reach_policy() {
    // ロン可否が unknown の場合、非フリテンだと推測してダマ打点の policy を適用しない。
    let case = damaten_case(&PINFU_TANYAO_HAND, "N", &["1p"]);
    let ctx = case
        .ctx
        .clone()
        .with_history_furiten_facts(HistoryFuritenFacts::default());
    let reach = reach_diagnostic(&ctx, &case.actions);

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::No));
    assert_eq!(reach.can_ron(), None);
    assert_eq!(reach.damaten_value, None);
    assert!(!reach.used_damaten_value());
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert!(reach.should_reach());
}

#[test]
fn damaten_value_uses_the_selected_discard_wait_and_the_known_dora() {
    // ダマ打点は選択済み打牌の受け入れそのものを待ちとして使い、リーチ用に待ちを求め直さない。
    let case = damaten_case(&PINFU_TANYAO_HAND, "N", &["1m"]);
    let reach = case.reach();
    let tenpai_wait = reach.tenpai_wait.as_ref().expect("テンパイ");
    let damaten = case.damaten();

    assert_eq!(reach.selected_discard, Some(case.tsumogiri.clone()));
    assert_eq!(
        damaten
            .waits
            .iter()
            .map(|wait| wait.winning_tile)
            .collect::<Vec<_>>(),
        tenpai_wait.live_waits
    );
    assert_eq!(
        damaten.waits.iter().map(|wait| wait.remaining).sum::<u8>(),
        tenpai_wait.tsumo_remaining
    );
    // 待ち全体の残枚数は赤 / 黒の内訳の合計と一致する。
    for wait in &damaten.waits {
        assert_eq!(
            wait.winning_tiles
                .iter()
                .map(|winning_tile| winning_tile.remaining)
                .sum::<u8>(),
            wait.remaining
        );
    }
    assert!(reach.used_damaten_value());
}

#[test]
fn reach_wait_matches_the_selected_discard_evaluation() {
    // 待ち枚数・種類数は選ばれた打牌評価の受け入れそのもので、リーチ用に計算し直さない。
    for ctx in [
        tenpai_context(&[]),
        tenpai_context(&TENPAI_SCARCE_VISIBLE),
        tenpai_context_without_visible_tiles(),
    ] {
        let actions = tenpai_actions();
        let diagnostic = diagnose_matching_act(&ctx, &actions);
        let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
        let evaluation = diagnostic
            .normal_discard
            .as_ref()
            .expect("通常打牌を評価している")
            .selected
            .as_ref()
            .expect("打牌を選べる");

        assert_eq!(
            reach.shanten_after_discard,
            Some(evaluation.min_shanten_after_discard())
        );
        assert_eq!(
            reach.tsumo_remaining(),
            Some(evaluation.acceptance_total_remaining())
        );
        assert_eq!(
            reach.tsumo_type_count(),
            Some(evaluation.acceptance_type_count())
        );
    }
}

#[test]
fn reach_wait_reflects_visible_tiles_through_the_discard_evaluation() {
    // 見え牌の反映は打牌評価の時点で済んでいる。リーチ判断は補正済みの残枚数をそのまま使う。
    let plentiful = reach_diagnostic(&tenpai_context(&[]), &tenpai_actions());
    let scarce = reach_diagnostic(&tenpai_context(&TENPAI_SCARCE_VISIBLE), &tenpai_actions());

    assert_eq!(plentiful.tsumo_remaining(), Some(8));
    assert_eq!(scarce.tsumo_remaining(), Some(2));
    // 見え牌で消えるのは残枚数だけで、構造上の待ちは変わらない。
    assert_eq!(
        plentiful
            .tenpai_wait
            .as_ref()
            .map(|wait| &wait.structural_waits),
        scarce
            .tenpai_wait
            .as_ref()
            .map(|wait| &wait.structural_waits)
    );
}

#[test]
fn reach_wait_without_visible_tiles_uses_the_evaluation_acceptance() {
    // visible tiles が空でもリーチ専用 fallback を使わず、打牌評価の受け入れで判断する。
    let ctx = tenpai_context_without_visible_tiles();
    let reach = reach_diagnostic(&ctx, &tenpai_actions());

    assert!(ctx.visible_tiles().is_empty());
    assert!(reach.should_reach());
    assert_eq!(reach.tsumo_remaining(), Some(8));

    // 同じく visible tiles が空の単騎3枚待ちも、打牌評価の受け入れそのもので判断する。
    let tanki = reach_diagnostic(&tanki_tenpai_context(), &tanki_tenpai_actions());
    assert!(tanki_tenpai_context().visible_tiles().is_empty());
    assert!(tanki.should_reach());
    assert_eq!(tanki.reason, ReachDecisionReason::Eligible);
    assert_eq!(tanki.tsumo_remaining(), Some(3));
}

#[test]
fn reaches_with_a_three_tile_tanki_wait() {
    // 生牌の単騎は3枚。最低生き待ち枚数の境界そのものなので、先制リーチする。
    let ctx = tanki_tenpai_context();
    let actions = tanki_tenpai_actions();
    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let reach = diagnostic.reach.as_ref().expect("リーチを検討している");

    assert_eq!(reach.tsumo_remaining(), Some(3));
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert!(reach.should_reach());
    assert_eq!(diagnostic.selected_source, AgentActionSource::Reach);
    assert_eq!(diagnostic.selected_action, LegalAction::Reach);
}

#[test]
fn does_not_reach_with_a_two_tile_tanki_wait() {
    // 同じ単騎テンパイで待ち牌を1枚見せると2枚になり、境界未満なのでリーチしない。
    let ctx = tanki_tenpai_context_with_visible(&TANKI_TENPAI_SCARCE_VISIBLE);
    let actions = tanki_tenpai_actions();
    let diagnostic = diagnose_matching_act(&ctx, &actions);
    let reach = diagnostic.reach.as_ref().expect("リーチを検討している");

    assert_eq!(reach.selected_discard, Some(dahai(TANKI_TENPAI_DRAWN)));
    assert_eq!(reach.tsumo_remaining(), Some(2));
    assert_eq!(reach.reason, ReachDecisionReason::InsufficientLiveWait);
    assert!(!reach.should_reach());
    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
}

#[test]
fn permanent_furiten_is_reported_without_changing_the_reach_policy() {
    // 恒常フリテンのテンパイでも、待ち枚数だけで判断する今回の policy は変えない。
    let ctx = three_sided_context(&[], &[81]);
    let actions: Vec<LegalAction> = three_sided_actions()
        .into_iter()
        .chain([LegalAction::Reach])
        .collect();
    let reach = reach_diagnostic(&ctx, &actions);

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.can_ron(), Some(false));
    assert_eq!(reach.discarded_waits(), [tile(80).tile_type()]);
    // フリテンを理由にリーチを止めない。
    assert!(reach.should_reach());
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
}

#[test]
fn an_unknown_player_id_leaves_the_reach_furiten_unknown() {
    // 自分の河を特定できない場合、非フリテンだと推測しない。
    let ctx = three_sided_context_without_player_id(&[81]);
    let actions: Vec<LegalAction> = three_sided_actions()
        .into_iter()
        .chain([LegalAction::Reach])
        .collect();
    let reach = reach_diagnostic(&ctx, &actions);

    assert_eq!(ctx.own_discards(), None);
    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Unknown));
    assert_eq!(reach.can_ron(), None);
    assert!(reach.discarded_waits().is_empty());
    assert!(reach.should_reach());
}

#[test]
fn a_fully_visible_discarded_wait_keeps_the_reach_furiten_diagnostic() {
    // 構造上の待ちの一部が残枚数 0 でも、恒常フリテンは解除されない。
    let ctx = three_sided_context(&[82, 83], &[81]);
    let actions: Vec<LegalAction> = three_sided_actions()
        .into_iter()
        .chain([LegalAction::Reach])
        .collect();
    let reach = reach_diagnostic(&ctx, &actions);
    let tenpai = reach.tenpai_wait.as_ref().expect("テンパイになる");

    let three_sou = tile(80).tile_type();
    assert!(tenpai.structural_waits.contains(&three_sou));
    assert!(!tenpai.live_waits.contains(&three_sou));
    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.can_ron(), Some(false));
    assert_eq!(reach.discarded_waits(), [three_sou]);
}

#[test]
fn reach_and_push_pull_share_the_selected_discard_evaluation() {
    // リーチ判断と押し引きが別々の打牌評価を参照しないことを固定する。
    for (ctx, actions) in [
        (tenpai_context(&[]), tenpai_actions()),
        (tenpai_context(&TENPAI_SCARCE_VISIBLE), tenpai_actions()),
        (tanki_tenpai_context(), tanki_tenpai_actions()),
    ] {
        let diagnostic = diagnose_matching_act(&ctx, &actions);
        let reach = diagnostic.reach.as_ref().expect("リーチを検討している");
        let offense = diagnostic
            .push_pull_inputs
            .as_ref()
            .expect("押し引き入力がある")
            .offense
            .expect("攻撃評価がある");

        assert_eq!(
            reach.shanten_after_discard,
            Some(offense.min_shanten_after_discard)
        );
        assert_eq!(
            reach.tsumo_remaining(),
            Some(offense.acceptance_total_remaining)
        );
        assert_eq!(
            reach.tsumo_type_count(),
            Some(offense.acceptance_type_count)
        );
    }
}

// ---- 恒常フリテンの named 役満 ----

// 白白白 發發發 中中 + 234m + 5m5m の 中 / 5m シャンポン。中ツモは大三元だが、5m ツモは
// 小三元にしかならないので、生きた variant の一部だけが named 役満になる。
const DAISANGEN_SHANPON_HAND: [&str; 13] = [
    "P", "P", "P", "F", "F", "F", "C", "C", "2m", "3m", "4m", "5m", "5m",
];

// 555m 123p 789p + 5s5s 9s9s の 5s / 9s シャンポン。ドラ表示牌 4m を4枚見せると 5m のドラ
// 12翻になり、門前ツモと合わせて数え役満になる。名前の付いた役満は付かない。
const KAZOE_SHANPON_HAND: [&str; 13] = [
    "5m", "5m", "5m", "1p", "2p", "3p", "7p", "8p", "9p", "5s", "5s", "9s", "9s",
];
const KAZOE_DORA_INDICATORS: [&str; 4] = ["4m", "4m", "4m", "4m"];

#[test]
fn a_permanent_furiten_named_yakuman_tenpai_never_declares_the_reach() {
    // 国士無双を自分で切って恒常フリテンになった聴牌。ロンできないので、生きた待ちはすべて
    // ツモ和了の役満だけになる。リーチしても和了形が変わらないので宣言しない。
    let case = DamatenSpec {
        hand: &KOKUSHI_HAND,
        drawn: "5m",
        own_river: &["1m"],
        ..DamatenSpec::default()
    }
    .build();
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.can_ron(), Some(false));
    assert!(
        reach
            .tsumo_remaining()
            .is_some_and(|remaining| remaining > 0)
    );
    // ロンできない聴牌ではダマ打点そのものを評価しない。判断材料は既存 scoring の Tsumo
    // 評価だけで、ダマ打点 threshold の結論ではない。
    assert_eq!(reach.damaten_value, None);
    assert_eq!(reach.reason, ReachDecisionReason::NamedYakumanDamaten);
    assert!(!reach.base_selects_reach());

    // base policy がダマなので timing 判断そのものを行わない。DeferReach でもない。
    assert_eq!(reach.timing, None);
    assert!(!reach.defers_reach());
    assert!(!reach.should_reach());
    assert_eq!(reach.selected, None);

    // 通常打牌 selection が選んだ打牌をそのまま行う。
    assert_eq!(reach.selected_discard, Some(case.tsumogiri.clone()));
    assert_eq!(case.act(), case.tsumogiri);
    let diagnostic = ShantenAgent::diagnose(&case.ctx, &case.actions);
    assert_eq!(diagnostic.selected_source, AgentActionSource::NormalDiscard);
    assert_eq!(
        diagnostic.normal_discard_action,
        Some(case.tsumogiri.clone())
    );
}

#[test]
fn a_non_furiten_named_yakuman_keeps_the_damaten_value_reason() {
    // 非フリテンの国士はダマでロンできるので、従来どおりダマ打点 threshold の結論になる。
    let case = damaten_case(&KOKUSHI_HAND, "5m", &[]);
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::No));
    assert_eq!(reach.can_ron(), Some(true));
    assert_eq!(
        reach.damaten_verdict(),
        Some(DamatenValueVerdict::AboveThreshold)
    );
    assert_eq!(reach.reason, ReachDecisionReason::HighValueDamaten);
    assert!(!reach.should_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn a_permanent_furiten_ordinary_tenpai_keeps_the_existing_reach_policy() {
    // 恒常フリテンでも役満でなければ categorical rule に入らない。base policy も timing も
    // 従来のまま。
    let case = damaten_case_with(&PINFU_TANYAO_HAND, "N", &[], &[], &["3s"]);
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert!(reach.base_selects_reach());

    // base policy がリーチなので timing 判断へ進む。この局面は山の残枚数が不明で self-tsumo
    // 比較が確定しないため、既存どおり base のリーチを維持する。
    let timing = reach
        .timing
        .expect("base policy がリーチなら timing を評価する");
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(timing.reason, ReachTimingReason::SelfTsumoComparisonUnknown);
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn a_partly_named_yakuman_permanent_furiten_tenpai_keeps_the_existing_reach_policy() {
    // 中ツモだけが大三元で、5m ツモは小三元。全ての生きた variant が named 役満ではないので
    // categorical rule に入らない。
    let case = DamatenSpec {
        hand: &DAISANGEN_SHANPON_HAND,
        own_river: &["C"],
        ..DamatenSpec::default()
    }
    .build();
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_ne!(reach.reason, ReachDecisionReason::NamedYakumanDamaten);
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);

    // 同じ手牌を非フリテンにすると、役満なのは中だけだと既存のダマ打点でも確認できる。
    let non_furiten = DamatenSpec {
        hand: &DAISANGEN_SHANPON_HAND,
        ..DamatenSpec::default()
    }
    .build();
    let yakuman: Vec<bool> = non_furiten
        .damaten_values()
        .iter()
        .map(|value| value.is_yakuman())
        .collect();
    assert!(
        yakuman.contains(&true) && yakuman.contains(&false),
        "{yakuman:?}"
    );
}

#[test]
fn an_unknown_scoring_input_does_not_infer_a_named_yakuman() {
    // 自風が不明で親子が決まらないと支払いを確定できない。役満だと推測せず、既存 policy へ
    // fallback する。
    let case = DamatenSpec {
        hand: &KOKUSHI_HAND,
        drawn: "5m",
        own_river: &["1m"],
        known_winds: false,
        ..DamatenSpec::default()
    }
    .build();
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_ne!(reach.reason, ReachDecisionReason::NamedYakumanDamaten);
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
}

#[test]
fn a_kazoe_yakuman_permanent_furiten_tenpai_is_not_a_named_yakuman() {
    // 数え役満は名前の付いた役満ではないので categorical rule に入らない。
    let case = DamatenSpec {
        hand: &KAZOE_SHANPON_HAND,
        dora_indicators: &KAZOE_DORA_INDICATORS,
        own_river: &["5s"],
        ..DamatenSpec::default()
    }
    .build();
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_ne!(reach.reason, ReachDecisionReason::NamedYakumanDamaten);
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
}

#[test]
fn an_unknown_permanent_furiten_named_yakuman_keeps_the_existing_reach_policy() {
    // 自分の席が分からず自分の河を特定できない局面。恒常フリテンだと推測しないので
    // categorical rule に入らない。
    let case = DamatenSpec {
        hand: &KOKUSHI_HAND,
        drawn: "5m",
        own_river: &["1m"],
        known_seat: false,
        ..DamatenSpec::default()
    }
    .build();
    let reach = case.reach();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Unknown));
    assert_eq!(reach.can_ron(), None);
    assert_ne!(reach.reason, ReachDecisionReason::NamedYakumanDamaten);
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
}

// ---- リーチ timing ----

// リーチ timing の局面。恒常フリテンにするための自分の河と、self-tsumo 比較の材料
// (山の残枚数・持ち点) を指定できるようにする。
struct ReachTimingSpec<'a> {
    hand: &'a [&'a str],
    draw: &'a str,
    own_river: &'a [&'a str],
    /// 下家のリーチ後の河。空なら他家リーチ無しで、押し引きは Push になる。
    opponent_reach_river: &'a [&'a str],
    remaining_tiles: Option<u32>,
    legal_reach: bool,
}

impl Default for ReachTimingSpec<'_> {
    fn default() -> Self {
        Self {
            hand: &FURITEN_RYANMEN_HAND,
            draw: TSUMOGIRI_DRAW,
            own_river: &[],
            opponent_reach_river: &[],
            remaining_tiles: Some(REACH_TIMING_REMAINING_TILES),
            legal_reach: true,
        }
    }
}

struct ReachTimingCase {
    ctx: GameContext,
    actions: Vec<LegalAction>,
    tsumogiri: LegalAction,
}

impl ReachTimingCase {
    fn reach(&self) -> ReachDecisionDiagnostic {
        reach_diagnostic(&self.ctx, &self.actions)
    }

    fn timing(&self) -> ReachTimingDiagnostic {
        self.reach()
            .timing
            .expect("base policy がリーチを選んでいる")
    }

    fn act(&self) -> LegalAction {
        ShantenAgent.act(&self.ctx, &self.actions)
    }

    // 選択済み候補1件の self-tsumo 比較。timing 判断が対象外にした局面でも counterfactual を
    // 確かめられるよう、production と同じ入口をテストから直接呼ぶ。
    fn self_tsumo_comparison(&self) -> Option<TenpaiSelfTsumoComparison> {
        let tiles: Vec<TileId> = self
            .ctx
            .hand_tiles()
            .iter()
            .copied()
            .chain(self.ctx.drawn_tile())
            .collect();
        let evaluation = select_best_normal_discard_evaluation(&self.ctx, &tiles, &self.actions)?;
        selected_tenpai_self_tsumo_comparison(&self.ctx, &evaluation, true)
    }
}

impl ReachTimingSpec<'_> {
    fn build(&self) -> ReachTimingCase {
        let mut source = TileIdSource::new();
        let hand_tiles = source.tiles(self.hand);
        let drawn_tile = source.tile(self.draw);
        let own_river = source.tiles(self.own_river);
        let opponent_river = source.tiles(self.opponent_reach_river);

        let visible: Vec<TileId> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .chain(own_river.iter())
            .chain(opponent_river.iter())
            .copied()
            .collect();
        let actions: Vec<LegalAction> = hand_tiles
            .iter()
            .chain([&drawn_tile])
            .map(|&tile| LegalAction::Dahai { tile })
            .chain(self.legal_reach.then_some(LegalAction::Reach))
            .collect();

        let ctx = GameContext::from_parts_with_table_state(
            Some(drawn_tile),
            hand_tiles,
            Vec::new(),
            TileType::from_mjai_type_str("E").ok(),
            TileType::from_mjai_type_str("S").ok(),
            visible,
            Some(0),
            Some(3),
            [own_river, opponent_river, vec![], vec![]],
            [false, !self.opponent_reach_river.is_empty(), false, false],
        )
        .with_table_state_facts(TableStateFacts {
            remaining_tiles: self.remaining_tiles,
            scores: Some([REACH_TIMING_SCORE; 4]),
            ..Default::default()
        })
        .with_history_furiten_facts(HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        });

        ReachTimingCase {
            ctx,
            actions,
            tsumogiri: LegalAction::Dahai { tile: drawn_tile },
        }
    }
}

// 345m 678m 789p 22s 55s の 2s / 5s シャンポンテンパイ。ツモ切りでテンパイを保つ。
const SHANPON_HAND: [&str; 13] = [
    "3m", "4m", "5m", "6m", "7m", "8m", "7p", "8p", "9p", "2s", "2s", "5s", "5s",
];

// 345m 678m 789p 789s + 3s4s の実戦形 (5m は赤5)。打 3s の 4s 単騎から 3m ツモで
// 3m / 6m / 9m の三面待ちへ変わるため、counterfactual では defer が上回る。
const TANKI_HAND: [&str; 13] = [
    "3m", "4m", "5mr", "6m", "7m", "8m", "7p", "8p", "9p", "4s", "7s", "8s", "9s",
];
const TANKI_DRAW: &str = "3s";

// 123m 456m 789m 123p + 1s の 1s 単騎。么九牌 gate の regression。
const TERMINAL_TANKI_HAND: [&str; 13] = [
    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1s",
];

// 114477m 225588p + 4s の七対子専用 4s 単騎。hand family では除外しない regression。
const CHIITOITSU_MIDDLE_TANKI_HAND: [&str; 13] = [
    "1m", "1m", "4m", "4m", "7m", "7m", "2p", "2p", "5p", "5p", "8p", "8p", "4s",
];

// 2s を自分で切ってある恒常フリテンのシャンポン。生きた待ちは 2s 1枚 + 5s 2枚で、
// base policy はリーチを選ぶ。
fn furiten_shanpon_spec() -> ReachTimingSpec<'static> {
    ReachTimingSpec {
        hand: &SHANPON_HAND,
        own_river: &["2s"],
        ..ReachTimingSpec::default()
    }
}

fn furiten_shanpon_case() -> ReachTimingCase {
    furiten_shanpon_spec().build()
}

// 234m 678m 789p 45s 99s の 3s / 6s 両面テンパイ。ツモ切りでテンパイを保つ。
const FURITEN_RYANMEN_HAND: [&str; 13] = [
    "2m", "3m", "4m", "6m", "7m", "8m", "7p", "8p", "9p", "4s", "5s", "9s", "9s",
];
// どの面子にも関係しないツモ牌。
const TSUMOGIRI_DRAW: &str = "N";
// 山の残枚数。4人で分けて自分の残り自摸機会になる。
const REACH_TIMING_REMAINING_TILES: u32 = 70;
// リーチ宣言の条件を満たす持ち点。
const REACH_TIMING_SCORE: i32 = 25_000;

#[test]
fn a_permanent_furiten_tenpai_defers_the_reach_when_one_more_draw_scores_higher() {
    // 恒常フリテンで現在の待ちではロンできないので、「今リーチ」と「1巡 defer して次の
    // テンパイでリーチ」を self-tsumo だけで比べられる。defer が上回るならリーチを見送り、
    // 通常打牌 selection が既に選んだ打牌をそのまま行う。
    let case = furiten_shanpon_case();
    let reach = case.reach();
    let timing = case.timing();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.can_ron(), Some(false));
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert!(reach.base_selects_reach());

    assert_eq!(timing.decision, ReachTimingDecision::DeferReach);
    assert_eq!(timing.reason, ReachTimingReason::PermanentFuritenSelfTsumo);
    assert!(timing.defer_forced_reach > timing.reach_now, "{timing:?}");

    // 今回の request でリーチを宣言しないだけで、defer 用に別の打牌を探索しない。
    assert!(reach.defers_reach());
    assert!(!reach.should_reach());
    assert_eq!(reach.selected, None);
    assert_eq!(reach.selected_discard, Some(case.tsumogiri.clone()));
    assert_eq!(case.act(), case.tsumogiri);
    assert_eq!(
        ShantenAgent::diagnose(&case.ctx, &case.actions).selected_source,
        AgentActionSource::NormalDiscard
    );
}

#[test]
fn a_permanent_furiten_tenpai_reaches_now_when_deferring_does_not_score_higher() {
    // 同じ恒常フリテンでも defer が上回らなければ今リーチする。同値も ReachNow。
    let case = ReachTimingSpec {
        own_river: &["3s"],
        ..ReachTimingSpec::default()
    }
    .build();
    let reach = case.reach();
    let timing = case.timing();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(timing.reason, ReachTimingReason::PermanentFuritenSelfTsumo);
    assert!(timing.reach_now >= timing.defer_forced_reach, "{timing:?}");

    assert!(!reach.defers_reach());
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn an_unresolvable_comparison_keeps_the_base_reach() {
    // 山の残枚数が分からないと self-tsumo 確率模型の材料が揃わない。比較不能を 0 点と
    // 混同せず、既存 base Reach を維持する。
    let case = ReachTimingSpec {
        remaining_tiles: None,
        ..furiten_shanpon_spec()
    }
    .build();
    let reach = case.reach();
    let timing = case.timing();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::Yes));
    assert_eq!(reach.reason, ReachDecisionReason::Eligible);
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(timing.reason, ReachTimingReason::SelfTsumoComparisonUnknown);
    assert_eq!(timing.reach_now, None);
    assert_eq!(timing.defer_forced_reach, None);

    // 同じ手牌でも残枚数が既知なら defer していた局面。
    assert!(furiten_shanpon_case().reach().defers_reach());
    assert!(reach.should_reach());
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn a_non_furiten_bad_single_wait_defers_when_one_more_draw_scores_higher() {
    // 既知局面: 打 3s → 4s 単騎3枚。非フリテンでも限定 structural gate をすべて満たし、
    // counterfactual 上 defer が上回るため、今回は selected Dahai を行う。
    let case = ReachTimingSpec {
        hand: &TANKI_HAND,
        draw: TANKI_DRAW,
        ..ReachTimingSpec::default()
    }
    .build();
    let reach = case.reach();
    let timing = case.timing();
    let comparison = case
        .self_tsumo_comparison()
        .expect("self-tsumo 比較の材料が揃っている");

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::No));
    assert_eq!(reach.can_ron(), Some(true));
    assert_eq!(reach.tsumo_type_count(), Some(1));
    assert_eq!(reach.tsumo_remaining(), Some(3));
    assert_eq!(reach.selected_discard, Some(case.tsumogiri.clone()));
    assert!(reach.base_selects_reach());
    assert!(
        comparison.defer_forced_reach() > comparison.reach_now,
        "{comparison:?}"
    );

    let safety = reach_public_safety_after_discard(
        &case.ctx,
        reach.selected_discard.as_ref().expect("selected Dahai"),
        reach.tenpai_wait.as_ref().expect("テンパイ").live_waits[0],
    )
    .expect("Reach public safety");
    assert!(!safety.genbutsu);
    let suited = safety.suited.expect("数牌の public safety");
    assert_eq!(suited.wall_rank, WallRank::NoWall);
    assert_eq!(suited.suji_rank, SujiSafetyRank::NoSuji);

    assert_eq!(timing.decision, ReachTimingDecision::DeferReach);
    assert_eq!(timing.reason, ReachTimingReason::NonFuritenBadWaitHeuristic);
    assert_eq!(timing.reach_now, comparison.reach_now);
    assert_eq!(timing.defer_forced_reach, comparison.defer_forced_reach());
    assert!(reach.defers_reach());
    assert_eq!(case.act(), case.tsumogiri);
    assert_eq!(
        ShantenAgent::diagnose(&case.ctx, &case.actions).selected_source,
        AgentActionSource::NormalDiscard
    );
}

#[test]
fn an_ordinary_non_furiten_multi_wait_reaches_now() {
    let case = ReachTimingSpec::default().build();
    let reach = case.reach();
    let timing = case.timing();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::No));
    assert_eq!(reach.can_ron(), Some(true));
    assert!(reach.tsumo_type_count().is_some_and(|count| count > 1));
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(
        timing.reason,
        ReachTimingReason::NonFuritenBadWaitHeuristicNotEligible
    );
    assert_eq!(timing.reach_now, None);
    assert_eq!(timing.defer_forced_reach, None);
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn a_terminal_single_wait_is_not_eligible_for_the_non_furiten_heuristic() {
    let case = ReachTimingSpec {
        hand: &TERMINAL_TANKI_HAND,
        ..ReachTimingSpec::default()
    }
    .build();
    let reach = case.reach();
    let timing = case.timing();
    let wait = reach.tenpai_wait.as_ref().expect("テンパイ").live_waits[0];

    assert!(wait.is_yaochu());
    assert_eq!(reach.tsumo_type_count(), Some(1));
    assert!(reach.base_selects_reach());
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(
        timing.reason,
        ReachTimingReason::NonFuritenBadWaitHeuristicNotEligible
    );
    assert_eq!(timing.reach_now, None);
    assert_eq!(timing.defer_forced_reach, None);
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn a_chiitoitsu_middle_tile_single_wait_is_not_excluded_by_hand_family() {
    let mut case = ReachTimingSpec {
        hand: &CHIITOITSU_MIDDLE_TANKI_HAND,
        ..ReachTimingSpec::default()
    }
    .build();
    // この regression は family gate だけを観測するため、ツモ切りで 4s 単騎を維持する合法手に
    // 固定する。別の単騎へ移る通常 selector の優劣は対象外。
    case.actions = vec![case.tsumogiri.clone(), LegalAction::Reach];
    let reach = case.reach();
    let timing = case.timing();
    let wait = reach.tenpai_wait.as_ref().expect("テンパイ").live_waits[0];

    assert!(!wait.is_yaochu());
    assert_eq!(reach.tsumo_type_count(), Some(1));
    assert!(reach.base_selects_reach());
    // eligibility reason まで進むことで、七対子 family を除外していないことを保証する。
    assert_eq!(timing.reason, ReachTimingReason::NonFuritenBadWaitHeuristic);
    assert!(timing.reach_now.is_some());
    assert!(timing.defer_forced_reach.is_some());
}

#[test]
fn a_non_furiten_single_wait_with_public_safety_reaches_now() {
    // 自分の河の 1s により、Reach 後の 4s は片スジ。NoSuji ではないため self-tsumo
    // comparison を production では評価せず、base Reach を維持する。
    let case = ReachTimingSpec {
        hand: &TANKI_HAND,
        draw: TANKI_DRAW,
        own_river: &["1s"],
        ..ReachTimingSpec::default()
    }
    .build();
    let reach = case.reach();
    let timing = case.timing();
    let safety = reach_public_safety_after_discard(
        &case.ctx,
        reach.selected_discard.as_ref().expect("selected Dahai"),
        reach.tenpai_wait.as_ref().expect("テンパイ").live_waits[0],
    )
    .expect("Reach public safety");

    let suited = safety.suited.expect("数牌の public safety");
    assert_eq!(suited.wall_rank, WallRank::NoWall);
    assert_eq!(suited.suji_rank, SujiSafetyRank::HalfSuji);
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(
        timing.reason,
        ReachTimingReason::NonFuritenBadWaitHeuristicNotEligible
    );
    assert_eq!(timing.reach_now, None);
    assert_eq!(timing.defer_forced_reach, None);
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn an_unknown_non_furiten_bad_wait_comparison_reaches_now() {
    let case = ReachTimingSpec {
        hand: &TANKI_HAND,
        draw: TANKI_DRAW,
        remaining_tiles: None,
        ..ReachTimingSpec::default()
    }
    .build();
    let reach = case.reach();
    let timing = case.timing();

    assert_eq!(reach.permanent_furiten(), Some(PermanentFuriten::No));
    assert_eq!(reach.can_ron(), Some(true));
    assert_eq!(timing.decision, ReachTimingDecision::ReachNow);
    assert_eq!(timing.reason, ReachTimingReason::SelfTsumoComparisonUnknown);
    assert_eq!(timing.reach_now, None);
    assert_eq!(timing.defer_forced_reach, None);
    assert_eq!(case.act(), LegalAction::Reach);
}

#[test]
fn a_damaten_base_policy_does_not_evaluate_the_timing() {
    // base policy がダマを選んだ聴牌では timing 判断そのものを行わない。
    let case = damaten_case(&KOKUSHI_HAND, "5m", &[]);
    let reach = case.reach();

    assert_eq!(reach.reason, ReachDecisionReason::HighValueDamaten);
    assert!(!reach.base_selects_reach());
    assert_eq!(reach.timing, None);
    assert!(!reach.defers_reach());
    assert_eq!(case.act(), case.tsumogiri);
}

#[test]
fn the_timing_is_not_evaluated_without_a_base_reach() {
    // 選択済み候補の continuation を評価するのは base policy がリーチを選んだ場合だけ。
    // それ以外では timing 判断そのものを持たない。
    let not_tenpai = {
        let hand_values = [0, 4, 8, 12, 17, 20, 24, 28, 32, 36, 40, 56, 89];
        let ctx = GameContext::from_parts(
            Some(tile(116)),
            hand_values.iter().map(|&value| tile(value)).collect(),
        );
        let actions: Vec<LegalAction> = hand_values
            .iter()
            .map(|&value| dahai(value))
            .chain([dahai(116), LegalAction::Reach])
            .collect();
        reach_diagnostic(&ctx, &actions)
    };
    assert_eq!(not_tenpai.reason, ReachDecisionReason::NotTenpai);
    assert_eq!(not_tenpai.timing, None);

    let illegal_reach = ReachTimingSpec {
        legal_reach: false,
        ..furiten_shanpon_spec()
    }
    .build();
    let illegal_reach = illegal_reach.reach();
    assert_eq!(illegal_reach.reason, ReachDecisionReason::NoLegalReach);
    assert_eq!(illegal_reach.timing, None);
}
