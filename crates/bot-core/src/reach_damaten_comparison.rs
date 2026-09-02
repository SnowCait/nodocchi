//! 現在聴牌の Reach / Damaten 判断材料を1か所へ並べる診断層。
//!
//! 通常打牌 selection が選んだ打牌後のテンパイについて、既に別々の診断が持っている材料を1つの
//! 構造へ集める層である。判断も探索もここでは行わず、材料の出どころは既存の source of truth
//! そのままになる。
//!
//! ```text
//! production Reach 判断  → ReachDecisionDiagnostic (reason / should_reach / can_ron)
//! ダマ Ron 打点          → ReachDecisionDiagnostic の DamatenValueDiagnostic
//! self-tsumo 期待支払い  → TenpaiContinuationCandidate の TenpaiSelfTsumoComparison
//! リーチ Ron 打点        → 既存 reach_baseline_context() を同じ完成手集合へ適用した観測値
//! ```
//!
//! # 診断経路だけで評価する
//!
//! リーチ Ron 打点だけは production 判断が評価しない観測値なので、この層が既存 scoring rule で
//! 評価する。診断のために新しく点数計算するのはこの1つで、他の材料は既存値をそのまま読む。
//!
//! 評価するのは診断が有効な経路だけ。通常の [`ShantenAgent::act()`](crate::agents::ShantenAgent)
//! はこの層を通らないので、完成手の組み立ても hand-value evaluation も production には入らない。
//! 完成手は待ちごとの解析を丸ごと所有する重い値なので、production の打牌選択へ持ち回らせない。
//! リーチ判断がダマ打点のために組み立てた集合があればその所有権を受け取り、無い場合だけ選んだ
//! 打牌1件について既存 helper で1回組み立てる。
//!
//! # 2つの軸
//!
//! self-tsumo と Ron は単位が違う別の軸で、この層でも別の軸のまま並べる。
//!
//! ```text
//! self-tsumo   → 自摸確率を含んだ期待ツモ支払い
//! Ron baseline → その待ちで和了した場合の支払い。ロンの発生確率を含まない
//! ```
//!
//! nodocchi はまだ「他家がその牌を切る確率」の模型を持たないため、Ron baseline を期待値へ
//! 変換できない。したがって2つの軸を足した合計も、係数で重み付けした score も作らない。
//! リーチ Ron baseline は実際にリーチが合法で、かつ既存 Ron availability が `Some(true)` の
//! 場合だけ評価する。フリテンとロン可否 unknown では評価しないが、self-tsumo は独立して評価
//! 可能なままにする。
//!
//! # 判断しない
//!
//! winner も新しい `should_reach` も持たない。production の Reach / Damaten 判断は既存
//! [`decide_reach_reason`](crate::reach_policy::decide_reach_reason) だけが決めており、この層が
//! 持つのはその結論の観測値 (`production_reason` / `production_should_reach`) だけで、判断へは
//! 接続していない。
//!
//! # 対応付け
//!
//! self-tsumo 比較は、通常打牌 selection が実際に選んだ打牌に対応する継続候補のものだけを参照
//! する。別の打牌候補の継続を結び付けない。継続診断そのものが構築されていない (2手先探索を
//! 要求していない) 局面と、選んだ打牌に対応する候補が無い局面では推測せず `None` にする。
//!
//! # 評価しない値
//!
//! ダマでロンできない / ロン可否が unknown の場合、ダマ Ron 打点は既存 semantics どおり評価
//! しないまま (`None`) にする。0 点として扱わない。ロンできないことは Tsumo 側の軸とは独立
//! なので、self-tsumo の値を Ron 可否で潰すこともしない。
//!
//! # Ron opportunity
//!
//! Ron 側の baseline は「和了した場合の支払い」だけで、他家が待ち牌を切る確率を持たない。その
//! 前段として、待ちが公開情報上どう見えるかの structural facts を
//! [`RonOpportunityDiagnostic`](crate::ron_opportunity::RonOpportunityDiagnostic) が持つ。確率
//! ではないので、Ron baseline と掛け合わせた EV もここでは作らない。

use bot_logic::{
    DiscardEvaluation, EffectiveAcceptance, TenpaiCompletedHands, TenpaiWaitAvailability, TileType,
};

use crate::action::LegalAction;
use crate::agents::{ReachDecisionDiagnostic, ReachDecisionReason};
use crate::context::GameContext;
use crate::damaten_value::{
    DamatenValueDiagnostic, DamatenValueVerdict, damaten_value_from_hands,
    tenpai_completed_hands_after_discard,
};
use crate::discard_selection::{DiscardActionSelection, selected_discard_tenpai_wait_availability};
use crate::offense_value::{ReachRonBaselineDiagnostic, reach_ron_baseline_from_hands};
use crate::open_hand_threat::OpenHandThreatAssessment;
use crate::ron_opportunity::{
    RonOpportunityDiagnostic, RonOpportunityInputs, diagnose_ron_opportunity,
};
use crate::tenpai_continuation::{TenpaiContinuationDiagnostic, TenpaiSelfTsumoComparison};

// テンパイの向聴数。
const TENPAI_SHANTEN: i8 = 0;

/// 現在聴牌の Reach / Damaten 判断材料をまとめた統合診断。
///
/// リーチ Ron baseline 以外はどれも既存診断が持っているものそのままで、この診断のために探索も
/// 集計もやり直さない。打牌選択・押し引き・リーチ判断のどれにも接続していない解析専用の情報で、
/// 構築の有無は選択結果を変えない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachDamatenComparisonDiagnostic {
    /// 通常打牌 selection が選んだ合法 Dahai。リーチ判断が見たものと同じ action。
    pub selected_discard: Option<LegalAction>,
    /// production のリーチ判断の理由。既存判断の結論そのもので、ここで決め直さない。
    pub production_reason: ReachDecisionReason,
    /// production がリーチを採用したか。既存 [`ReachDecisionDiagnostic::should_reach`] そのもの。
    ///
    /// 観測値であって、この層が作った別の結論ではない。
    pub production_should_reach: bool,
    /// 現在局面の合法手に [`LegalAction::Reach`] があるか。
    ///
    /// self-tsumo 比較の `reach now` と同じく、実際の合法手だけを source of truth にする。
    pub reach_legal: bool,
    /// 選んだ打牌に対応する継続候補の self-tsumo 比較そのもの。
    ///
    /// 2手先探索を要求していない局面と、選んだ打牌に対応する継続候補が無い局面では `None`。
    /// この診断のために2手先探索を追加しない。
    pub self_tsumo: Option<TenpaiSelfTsumoComparison>,
    /// 今リーチしてロン和了した場合の最低保証打点。ロンの発生確率は含まない。
    ///
    /// 実際にリーチでき、かつ既存 Ron availability が `Some(true)` の局面だけ評価する。リーチが
    /// 合法でない場合、フリテンの場合、ロン可否が unknown の場合は `None` にする。
    pub reach_ron_baseline: Option<ReachRonBaselineDiagnostic>,
    /// ダマのままロンできるか。既存フリテン診断の結論そのもの。
    pub can_ron: Option<bool>,
    /// ダマのままロン和了した場合の打点。
    ///
    /// ダマでロンできない場合とロン可否が unknown の場合は既存 semantics どおり評価しないので
    /// `None`。0 点として扱わない。
    pub damaten_ron_value: Option<DamatenValueDiagnostic>,
    /// 現在の待ちが公開情報上どう見えるかの structural facts。ロン確率ではない。
    ///
    /// 実際にロンできる (`can_ron() == Some(true)`) 局面だけで構築し、フリテンとロン可否 unknown
    /// では 0 として扱わず `None` (unavailable) にする。
    pub ron_opportunity: Option<RonOpportunityDiagnostic>,
}

impl ReachDamatenComparisonDiagnostic {
    /// 今すぐリーチした場合の期待ツモ支払い。self-tsumo 材料が無ければ `None`。
    pub fn reach_now_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.reach_now
    }

    /// 従来名との互換 accessor。terminal mode は forced Damaten ではなく production policy。
    pub fn damaten_continuation_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.defer_production()
    }

    /// 1巡 defer し、terminal tenpai では production policy に従う期待ツモ支払い合計。
    pub fn defer_production_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.defer_production()
    }

    /// 1巡 defer し、terminal tenpai では合法な場合に forced Reach とする期待ツモ支払い合計。
    pub fn defer_forced_reach_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.defer_forced_reach()
    }

    /// 1巡 defer し、terminal tenpai では forced Damaten とする期待ツモ支払い合計。
    pub fn defer_forced_damaten_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.defer_forced_damaten()
    }

    /// ダマのまま最初の1自摸で現在の待ちをツモ和了する期待支払い。
    pub fn damaten_immediate_tsumo_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.damaten_immediate_tsumo
    }

    /// 非和了牌を引いて手変わりした先の期待支払い合計。
    pub fn damaten_continuation_branches_self_tsumo(&self) -> Option<u64> {
        self.self_tsumo?.damaten_continuation_branches
    }

    /// ダマ打点から畳んだ結論。ダマ打点を評価しなかった場合は `None`。
    pub fn damaten_verdict(&self) -> Option<DamatenValueVerdict> {
        self.damaten_ron_value.as_ref().map(|value| value.verdict)
    }
}

/// 統合診断を組み立てるための材料。
///
/// production 判断と通常打牌 selection が既に構築済みの値をそのまま受け取る。`hands` だけは
/// 所有権ごと受け取り、診断のために deep clone しない。
pub(crate) struct ReachDamatenComparisonInputs<'a> {
    pub context: &'a GameContext,
    /// 現在局面の合法手に [`LegalAction::Reach`] があるか。
    pub reach_legal: bool,
    /// production のリーチ判断の結果。理由・ロン可否・ダマ打点の source of truth。
    pub reach: &'a ReachDecisionDiagnostic,
    /// 通常打牌 selection の結果。選んだ打牌の評価と、計算済みの待ち・ダマ打点を持つ。
    pub selection: &'a DiscardActionSelection,
    /// リーチ判断がダマ打点の評価のために組み立てた打牌後テンパイの完成手。
    ///
    /// 判断がその経路を通らなかった場合は `None`。完成手は待ちごとの解析を丸ごと所有する重い値
    /// なので、production 側で持ち回らずここで所有権を受け取る。
    pub hands: Option<TenpaiCompletedHands>,
    /// 現在聴牌のダマ継続診断。2手先探索を要求していない局面では `None`。
    pub continuation: Option<&'a TenpaiContinuationDiagnostic>,
    /// 押し引きが既に構築した全4席分の OpenHandThreat classification。
    ///
    /// Ron opportunity の external threats がそのまま借りる。診断のために分類し直さない。
    pub open_hand_threats: &'a [OpenHandThreatAssessment; 4],
}

/// production 判断と既存診断から、Reach / Damaten の判断材料を1つの診断へまとめる。
///
/// 呼ぶのは診断が有効な経路だけ。通常の `act()` はこの層を通らないので、リーチ Ron baseline の
/// 完成手の組み立ても点数計算も production には入らない。
pub(crate) fn diagnose_reach_damaten_comparison(
    inputs: ReachDamatenComparisonInputs,
) -> ReachDamatenComparisonDiagnostic {
    let ReachDamatenComparisonInputs {
        context,
        reach_legal,
        reach,
        selection,
        hands,
        continuation,
        open_hand_threats,
    } = inputs;
    let tenpai = selected_tenpai(context, reach, selection, hands);

    ReachDamatenComparisonDiagnostic {
        selected_discard: reach.selected_discard.clone(),
        production_reason: reach.reason,
        production_should_reach: reach.should_reach(),
        reach_legal,
        self_tsumo: selected_discard_self_tsumo(selection, continuation),
        reach_ron_baseline: tenpai.as_ref().and_then(|tenpai| {
            // Reach の合法性と Ron availability は別の条件。リーチが合法でもフリテンは解除されず、
            // unknown を非フリテンとも推測しない。Ron 軸は既存の総合値だけを source of truth にする。
            (reach_legal && tenpai.wait.can_ron() == Some(true))
                .then(|| reach_ron_baseline_from_hands(context, &tenpai.hands))
        }),
        can_ron: tenpai.as_ref().and_then(|tenpai| tenpai.wait.can_ron()),
        damaten_ron_value: tenpai
            .as_ref()
            .and_then(|tenpai| damaten_ron_value(context, reach, selection, tenpai)),
        ron_opportunity: tenpai.as_ref().and_then(|tenpai| {
            diagnose_ron_opportunity(RonOpportunityInputs {
                context,
                reach_legal,
                wait: &tenpai.wait,
                acceptance: tenpai.acceptance,
                open_hand_threats,
            })
        }),
    }
}

// 選んだ打牌に対応する継続候補の self-tsumo 比較。対応する候補が無ければ推測せず `None`。
fn selected_discard_self_tsumo(
    selection: &DiscardActionSelection,
    continuation: Option<&TenpaiContinuationDiagnostic>,
) -> Option<TenpaiSelfTsumoComparison> {
    let discard: TileType = selection.evaluation.as_ref()?.discard;
    Some(continuation?.candidate(discard)?.self_tsumo)
}

// 選んだ打牌後のテンパイの待ちと完成手。Ron 側の2つの baseline はこの1組を共有する。
struct SelectedTenpai<'a> {
    wait: TenpaiWaitAvailability,
    hands: TenpaiCompletedHands,
    // 打牌後の受け入れ。待ち牌種ごとの残枚数の source of truth で、借用するだけで複製しない。
    acceptance: &'a EffectiveAcceptance,
}

// 選んだ打牌後のテンパイを、既に計算済みの値から組み立てる。
//
// 待ちはリーチ判断か通常打牌 selection が計算済みならそれを使い、どちらも計算していない経路だけ
// 同じ既存 helper を通す。完成手はリーチ判断がダマ打点のために組み立てた集合を所有権ごと受け取り、
// 組み立てていない経路 (ダマでロンできない・ロン可否 unknown・現在聴牌比較がダマ打点を評価済み)
// だけ、選んだ打牌1件について既存 helper で1回組み立てる。
//
// 再構築が入るのは診断経路だけで、production の打牌選択・押し引き・リーチ判断は増えない。
// 診断のために production 側へ完成手の deep clone を持ち回るより、必要な診断でだけ1回組み立てる
// 方を選ぶ。待ちは既存の受け入れ (`acceptance_after_discard`) から求めるので、向聴も受け入れも
// ここで計算し直すことにはならない。
fn selected_tenpai<'a>(
    context: &GameContext,
    reach: &ReachDecisionDiagnostic,
    selection: &'a DiscardActionSelection,
    hands: Option<TenpaiCompletedHands>,
) -> Option<SelectedTenpai<'a>> {
    let evaluation: &DiscardEvaluation = selection.evaluation.as_ref()?;
    if evaluation.min_shanten_after_discard() != TENPAI_SHANTEN {
        return None;
    }

    let wait = reach
        .tenpai_wait
        .clone()
        .or_else(|| selection.tenpai_wait.clone())
        .or_else(|| selected_discard_tenpai_wait_availability(context, evaluation))?;
    let hands =
        hands.or_else(|| tenpai_completed_hands_after_discard(context, evaluation, &wait))?;

    Some(SelectedTenpai {
        wait,
        hands,
        acceptance: &evaluation.acceptance_after_discard,
    })
}

// ダマのままロン和了した場合の打点。
//
// 入口条件は既存 policy と同じで、自分が未リーチと確定していてダマでロンできる場合だけ評価する。
// フリテンとロン可否 unknown では非フリテンだと推測せず、0 点でもなく評価しないままにする。
// production 判断か通常打牌 selection が既に評価していればその診断そのもので、同じ hand-value
// evaluation を二度実行しない。
fn damaten_ron_value(
    context: &GameContext,
    reach: &ReachDecisionDiagnostic,
    selection: &DiscardActionSelection,
    tenpai: &SelectedTenpai<'_>,
) -> Option<DamatenValueDiagnostic> {
    if context.own_reached() != Some(false) || tenpai.wait.can_ron() != Some(true) {
        return None;
    }

    reach
        .damaten_value
        .clone()
        .or_else(|| selection.damaten_value.clone())
        .or_else(|| Some(damaten_value_from_hands(context, &tenpai.hands)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::LazyLock;

    use bot_logic::{HistoryFuritenFacts, RiichiStatus, TileId, WinMethod};

    use crate::agent::Agent;
    use crate::agents::{DiagnosticOptions, ShantenAgent, ShantenDecisionDiagnostic};
    use crate::context::TableStateFacts;
    use crate::damaten_value::DamatenValue;
    use crate::defense::{
        honor_safety_rank, is_genbutsu_for, suited_safety_evidence_for_players, visible_count_of,
    };
    use crate::meld::{Meld, MeldKind};
    use crate::offense_value::{TenpaiOffenseMode, reach_baseline_context};
    use crate::open_hand_defense::high_open_hand_threat_players_from_context;
    use crate::reach_policy::ReachDecisionReason;
    use crate::tenpai_continuation::TenpaiContinuationCandidate;

    // 234m 567m 789m 345p + 1s + 赤5s。打 1s で赤5s を残した 5s 単騎、打 5sr で 1s 単騎になる。
    // 赤5s を持つかどうかで打点が変わるので、2つの現在聴牌候補は別の self-tsumo 比較を持つ。
    const ASYMMETRIC_HAND: [&str; 14] = [
        "2m", "3m", "4m", "5m", "6m", "7m", "7m", "8m", "9m", "3p", "4p", "5p", "1s", "5sr",
    ];

    // 123m 456m 789m 123p 東 の門前13枚に南をツモった単騎テンパイ。打 E で南単騎になり、一気通貫
    // だけの手なのでダマでも役があり、リーチすると1翻増える。
    const ITTSUU_HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "E",
    ];
    const ITTSUU_DRAW: &str = "S";

    // 123m をチーした副露手。打 N が 3m / 6m / 9m のテンパイになり、ダマツモで役があるのは
    // 一気通貫になる 9m だけ。副露しているのでリーチは合法にならない。
    const OPEN_HAND: [&str; 10] = ["4m", "5m", "6m", "7m", "8m", "9p", "9p", "2s", "3s", "4s"];
    const OPEN_DRAW: &str = "N";
    const OPEN_MELD: [&str; 3] = ["1m", "2m", "3m"];

    // 一気通貫の 5s 単騎。待ちの 5s は赤5が1枚と黒5が2枚に分かれる。
    const RED_WAIT_HAND: [&str; 13] = [
        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "5s",
    ];
    const RED_WAIT_DRAW: &str = "E";

    // 山の残枚数。4人で分けて自分の残り自摸機会になる。
    const REMAINING_TILES: u32 = 70;

    // リーチ宣言の条件を満たす持ち点。
    const REACH_SCORE: i32 = 25_000;

    // 同じ牌種の赤5 / 黒5を取り違えないよう、物理牌を1枚ずつ払い出す。
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
            let red = s.ends_with('r');
            let id = TileId::copies(tile(s))
                .find(|id| id.is_red() == red && !self.used[id.index()])
                .expect("同じ物理牌を使い回していない");
            self.used[id.index()] = true;
            id
        }
    }

    fn tile(s: &str) -> TileType {
        TileType::from_mjai_type_str(s.trim_end_matches('r')).expect("牌種として読める")
    }

    struct MeldSpec<'a> {
        kind: MeldKind,
        tiles: &'a [&'a str],
    }

    struct CaseSpec<'a> {
        /// 打牌前の手牌。ツモ牌を含まない13枚か、ツモ牌まで含んだ14枚。副露牌は含まない。
        hand: &'a [&'a str],
        draw: Option<&'a str>,
        melds: &'a [MeldSpec<'a>],
        /// 自分の河。待ち牌を置くとフリテンになる。
        own_discards: &'a [&'a str],
        /// 合法手にリーチを含めるか。現在局面のリーチ可否はこの合法手だけが source of truth。
        legal_reach: bool,
        /// 合法 Dahai を絞る牌。`None` では手牌とツモ牌のすべてを切れる。
        legal_dahai: Option<&'a [&'a str]>,
        /// 選んだ打牌後の Ron availability に使う履歴依存フリテン facts。
        history_furiten: HistoryFuritenFacts,
        options: DiagnosticOptions,
    }

    impl Default for CaseSpec<'_> {
        fn default() -> Self {
            Self {
                hand: &ASYMMETRIC_HAND,
                draw: None,
                melds: &[],
                own_discards: &[],
                legal_reach: true,
                legal_dahai: None,
                history_furiten: HistoryFuritenFacts {
                    same_turn: Some(false),
                    riichi_missed_win: Some(false),
                },
                options: DiagnosticOptions::WITH_LOOKAHEAD,
            }
        }
    }

    // 局面と、その局面で act() / 診断が使う合法手。
    struct Case {
        context: GameContext,
        actions: Vec<LegalAction>,
        diagnostic: ShantenDecisionDiagnostic,
    }

    impl CaseSpec<'_> {
        fn build(&self) -> Case {
            let mut source = TileIdSource::new();
            let hand_tiles = source.tiles(self.hand);
            let drawn_tile = self.draw.map(|draw| source.tile(draw));
            let melds: Vec<Meld> = self
                .melds
                .iter()
                .map(|meld| {
                    let tiles = source.tiles(meld.tiles);
                    let called_tile = meld.kind.is_open().then(|| tiles[0]);
                    Meld::new(meld.kind, tiles, called_tile)
                })
                .collect();
            let own_discards = source.tiles(self.own_discards);

            let tiles: Vec<TileId> = hand_tiles.iter().copied().chain(drawn_tile).collect();
            let visible: Vec<TileId> = tiles
                .iter()
                .chain(melds.iter().flat_map(|meld| meld.tiles()))
                .chain(own_discards.iter())
                .copied()
                .collect();
            let actions: Vec<LegalAction> = tiles
                .iter()
                .filter(|id| {
                    self.legal_dahai.is_none_or(|dahai| {
                        dahai
                            .iter()
                            .any(|s| id.tile_type() == tile(s) && id.is_red() == s.ends_with('r'))
                    })
                })
                .map(|&tile| LegalAction::Dahai { tile })
                .chain(self.legal_reach.then_some(LegalAction::Reach))
                .collect();

            let mut discards: [Vec<TileId>; 4] = Default::default();
            let mut own_melds: [Vec<Meld>; 4] = Default::default();
            discards[0] = own_discards;
            own_melds[0] = melds;

            let context = GameContext::from_parts_with_melds(
                drawn_tile,
                hand_tiles,
                Vec::new(),
                Some(tile("E")),
                Some(tile("S")),
                visible,
                Some(0),
                Some(3),
                discards,
                [false; 4],
                own_melds,
            )
            .with_table_state_facts(TableStateFacts {
                remaining_tiles: Some(REMAINING_TILES),
                scores: Some([REACH_SCORE; 4]),
                ..Default::default()
            })
            .with_history_furiten_facts(self.history_furiten);

            let diagnostic = ShantenAgent::diagnose_with_options(&context, &actions, self.options);

            Case {
                context,
                actions,
                diagnostic,
            }
        }
    }

    impl Case {
        fn comparison(&self) -> &ReachDamatenComparisonDiagnostic {
            self.diagnostic
                .reach_damaten_comparison
                .as_ref()
                .expect("統合診断が構築されている")
        }

        fn reach(&self) -> &ReachDecisionDiagnostic {
            self.diagnostic
                .reach
                .as_ref()
                .expect("リーチを検討している")
        }

        fn continuation_candidate(&self, discard: &str) -> &TenpaiContinuationCandidate {
            self.diagnostic
                .normal_discard_tenpai_continuation
                .as_ref()
                .expect("継続診断が構築されている")
                .candidate(tile(discard))
                .unwrap_or_else(|| panic!("打 {discard} の継続候補がある"))
        }
    }

    fn ittsuu_spec() -> CaseSpec<'static> {
        CaseSpec {
            hand: &ITTSUU_HAND,
            draw: Some(ITTSUU_DRAW),
            ..CaseSpec::default()
        }
    }

    fn open_spec() -> CaseSpec<'static> {
        CaseSpec {
            hand: &OPEN_HAND,
            draw: Some(OPEN_DRAW),
            melds: &[MeldSpec {
                kind: MeldKind::Chi,
                tiles: &OPEN_MELD,
            }],
            legal_reach: false,
            ..CaseSpec::default()
        }
    }

    // 2手先探索は重いので、同じ局面を使う複数のテストで構築結果を共有する。
    static ASYMMETRIC: LazyLock<Case> = LazyLock::new(|| CaseSpec::default().build());
    static ITTSUU: LazyLock<Case> = LazyLock::new(|| ittsuu_spec().build());
    static OPEN: LazyLock<Case> = LazyLock::new(|| open_spec().build());

    // ---- 選んだ打牌との対応付け ----

    #[test]
    fn the_self_tsumo_comparison_comes_from_the_selected_discard_candidate() {
        // 通常打牌 selection が選んだ打牌に対応する継続候補の比較そのものを載せる。
        let case = &*ASYMMETRIC;
        let comparison = case.comparison();

        assert_eq!(comparison.selected_discard, Some(dahai(case, "1s")));
        assert_eq!(
            comparison.self_tsumo,
            Some(case.continuation_candidate("1s").self_tsumo)
        );
        assert!(comparison.reach_now_self_tsumo().is_some());
        assert!(comparison.damaten_continuation_self_tsumo().is_some());
    }

    #[test]
    fn the_self_tsumo_comparison_never_comes_from_another_candidate() {
        // 打 5sr は赤5を切る別の現在聴牌候補で、比較の値も違う。選ばなかった候補の値を
        // 結び付けない。
        let case = &*ASYMMETRIC;
        let other = case.continuation_candidate("5s").self_tsumo;

        assert_ne!(case.continuation_candidate("1s").self_tsumo, other);
        assert_ne!(case.comparison().self_tsumo, Some(other));
    }

    #[test]
    fn the_self_tsumo_comparison_is_unavailable_without_the_continuation_diagnostic() {
        // 2手先探索を要求していない局面では、統合診断のために探索を追加せず材料無しにする。
        let case = CaseSpec {
            options: DiagnosticOptions::NONE,
            ..CaseSpec::default()
        }
        .build();

        assert!(case.diagnostic.normal_discard_tenpai_continuation.is_none());
        assert_eq!(case.comparison().self_tsumo, None);
        assert_eq!(case.comparison().reach_now_self_tsumo(), None);
        assert_eq!(case.comparison().damaten_continuation_self_tsumo(), None);
        assert_eq!(case.comparison().damaten_immediate_tsumo_self_tsumo(), None);
        assert_eq!(
            case.comparison().damaten_continuation_branches_self_tsumo(),
            None
        );
        // Ron 側は self-tsumo の材料が無くても評価できる。
        assert!(case.comparison().reach_ron_baseline.is_some());
    }

    // ---- production 判断の観測 ----

    #[test]
    fn the_production_decision_is_observed_as_is() {
        for case in [&*ASYMMETRIC, &*ITTSUU, &*OPEN] {
            let reach = case.reach();
            let comparison = case.comparison();

            assert_eq!(comparison.production_reason, reach.reason);
            assert_eq!(comparison.production_should_reach, reach.should_reach());
            assert_eq!(comparison.selected_discard, reach.selected_discard);
        }

        // リーチ判断が待ちとダマ打点まで進んだ局面では、その結論そのものを載せる。
        for case in [&*ASYMMETRIC, &*ITTSUU] {
            let reach = case.reach();
            let comparison = case.comparison();

            assert!(reach.tenpai_wait.is_some());
            assert_eq!(comparison.can_ron, reach.can_ron());
            assert_eq!(comparison.damaten_verdict(), reach.damaten_verdict());
            assert_eq!(comparison.damaten_ron_value, reach.damaten_value);
        }
    }

    #[test]
    fn the_comparison_does_not_change_the_selected_action() {
        for spec in [CaseSpec::default(), ittsuu_spec(), open_spec()] {
            let case = spec.build();
            let mut agent = ShantenAgent;

            assert_eq!(
                case.diagnostic.selected_action,
                agent.act(&case.context, &case.actions)
            );
            assert!(case.diagnostic.reach_damaten_comparison.is_some());
        }
    }

    // ---- ダマ Ron 軸 ----

    #[test]
    fn the_damaten_ron_value_is_the_production_diagnostic_itself() {
        // 待ちも赤 / 黒 variant も production 判断が評価した診断そのもので、点数計算をやり直さない。
        let case = &*ITTSUU;
        let comparison = case.comparison();
        let production = case
            .reach()
            .damaten_value
            .as_ref()
            .expect("ダマ打点を評価している");

        assert_eq!(case.comparison().can_ron, Some(true));
        assert_eq!(comparison.damaten_ron_value.as_ref(), Some(production));
        assert_eq!(comparison.damaten_verdict(), Some(production.verdict));
        assert_eq!(
            comparison
                .damaten_ron_value
                .as_ref()
                .expect("ダマ打点がある")
                .winning_tile_values()
                .map(|value| (value.winning_tile, value.remaining, value.value))
                .collect::<Vec<_>>(),
            production
                .winning_tile_values()
                .map(|value| (value.winning_tile, value.remaining, value.value))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_furiten_tenpai_keeps_the_damaten_ron_value_unavailable() {
        // ダマでロンできない待ちのダマ打点は評価しないまま。0 点として扱わない。
        // 1s / 5s のどちらを切っても自分の河と重なる恒常フリテンにする。
        let case = CaseSpec {
            own_discards: &["1s", "5s"],
            ..CaseSpec::default()
        }
        .build();
        let comparison = case.comparison();

        assert_eq!(comparison.selected_discard, case.reach().selected_discard);
        assert!(comparison.reach_legal);
        assert_eq!(comparison.can_ron, Some(false));
        assert_eq!(comparison.reach_ron_baseline, None);
        assert_eq!(comparison.damaten_ron_value, None);
        assert_eq!(comparison.damaten_verdict(), None);

        // Tsumo 側は独立した軸なので、ロンできなくても評価する。
        assert!(comparison.damaten_continuation_self_tsumo().is_some());
        assert!(comparison.reach_now_self_tsumo().is_some());
    }

    #[test]
    fn an_unknown_ron_availability_is_not_guessed() {
        // ロン可否が分からない局面では非フリテンだと推測せず、どちらの Ron baseline も評価
        // しない。Tsumo 側は独立しており、入力が揃っているので引き続き評価できる。
        let case = CaseSpec {
            history_furiten: HistoryFuritenFacts::default(),
            ..ittsuu_spec()
        }
        .build();
        let comparison = case.comparison();

        assert!(comparison.reach_legal);
        assert_eq!(comparison.can_ron, None);
        assert_eq!(comparison.reach_ron_baseline, None);
        assert_eq!(comparison.damaten_ron_value, None);
        assert_eq!(comparison.damaten_verdict(), None);
        assert!(comparison.reach_now_self_tsumo().is_some());
        assert!(comparison.damaten_continuation_self_tsumo().is_some());
    }

    // ---- リーチ Ron baseline ----

    #[test]
    fn the_reach_ron_baseline_uses_the_existing_forced_reach_context() {
        let case = &*ITTSUU;
        assert!(case.comparison().reach_legal);
        assert_eq!(case.comparison().can_ron, Some(true));
        let baseline = case
            .comparison()
            .reach_ron_baseline
            .as_ref()
            .expect("リーチ Ron baseline を評価している");

        assert_eq!(baseline.baseline, reach_baseline_context(&case.context));
        assert_eq!(baseline.baseline.win_method(), WinMethod::Ron);
        assert_eq!(baseline.baseline.riichi(), RiichiStatus::Riichi);
        assert_eq!(baseline.baseline.ippatsu(), Some(false));
        assert_eq!(baseline.baseline.chankan(), Some(false));
    }

    #[test]
    fn the_reach_ron_baseline_adds_the_riichi_han_and_nothing_else() {
        // 一気通貫だけのダマ 2600 に対し、リーチ1翻を足した 5200 が最低保証打点。一発も裏ドラも
        // 加算しない。
        let case = &*ITTSUU;
        let comparison = case.comparison();
        let baseline = comparison
            .reach_ron_baseline
            .as_ref()
            .expect("リーチ Ron baseline を評価している");

        assert_eq!(
            baseline
                .winning_tile_values()
                .map(|value| value.total)
                .collect::<Vec<_>>(),
            [Some(5200)]
        );
        assert_eq!(
            comparison
                .damaten_ron_value
                .as_ref()
                .expect("ダマ打点がある")
                .winning_tile_values()
                .map(|value| value.value.total())
                .collect::<Vec<_>>(),
            [Some(2600)]
        );
    }

    #[test]
    fn the_reach_ron_baseline_shares_the_aggregation_of_the_offense_value() {
        // 集約規則も裏ドラの扱いも押し引きのリーチ打点と同じ1本。診断用の別集計を作らない。
        let case = &*ITTSUU;
        let offense = case
            .diagnostic
            .push_pull_inputs
            .as_ref()
            .and_then(|inputs| inputs.offense.as_ref())
            .and_then(|offense| offense.tenpai_offense_value_after_discard)
            .expect("押し引きの攻撃打点を評価している");
        let baseline = case
            .comparison()
            .reach_ron_baseline
            .as_ref()
            .expect("リーチ Ron baseline を評価している");

        assert_eq!(offense.mode, TenpaiOffenseMode::Reach);
        assert_eq!(baseline.value, offense.value);
    }

    #[test]
    fn the_reach_ron_baseline_keeps_the_red_and_black_variants() {
        // 待ちが赤5 / 黒5に分かれる場合、variant を潰さずそれぞれの打点を残す。赤5で和了する
        // 枝だけドラ1枚分高い。
        let case = CaseSpec {
            hand: &RED_WAIT_HAND,
            draw: Some(RED_WAIT_DRAW),
            legal_dahai: Some(&[RED_WAIT_DRAW]),
            options: DiagnosticOptions::NONE,
            ..CaseSpec::default()
        }
        .build();
        let baseline = case
            .comparison()
            .reach_ron_baseline
            .as_ref()
            .expect("リーチ Ron baseline を評価している");

        assert_eq!(
            baseline
                .winning_tile_values()
                .map(|value| (value.is_red(), value.remaining, value.total))
                .collect::<Vec<_>>(),
            [(true, 1, Some(8000)), (false, 2, Some(5200))]
        );
    }

    // ---- 副露手 ----

    #[test]
    fn an_open_hand_keeps_the_damaten_axis_without_a_legal_reach() {
        // 副露手ではリーチできないので reach now は材料が無い。ダマ側は既存情報で評価できる。
        let case = &*OPEN;
        let comparison = case.comparison();

        assert!(!comparison.reach_legal);
        assert_eq!(
            comparison.production_reason,
            ReachDecisionReason::NoLegalReach
        );
        assert!(!comparison.production_should_reach);
        assert_eq!(comparison.reach_now_self_tsumo(), None);
        assert_eq!(comparison.reach_ron_baseline, None);
        assert!(comparison.damaten_continuation_self_tsumo().is_some());
        assert!(comparison.damaten_immediate_tsumo_self_tsumo().is_some());
    }

    #[test]
    fn an_open_hand_keeps_the_no_yaku_waits_of_the_damaten_ron_value() {
        // 3m / 6m はダマで役が無い。役なしを 0 点にせず、役なしのまま残す。
        let case = &*OPEN;
        let damaten = case
            .comparison()
            .damaten_ron_value
            .as_ref()
            .expect("ダマ打点を評価している");
        let no_yaku: Vec<_> = damaten
            .winning_tile_values()
            .filter(|value| value.value == DamatenValue::NoYaku)
            .map(|value| value.winning_tile.tile_type())
            .collect();

        assert_eq!(no_yaku, [tile("3m"), tile("6m")]);
        assert_eq!(
            case.comparison().damaten_verdict(),
            Some(DamatenValueVerdict::NoYaku)
        );
    }

    // ---- Ron opportunity ----

    #[test]
    fn the_ron_opportunity_only_lists_the_live_waits_of_the_selected_tenpai() {
        // 選んだ打牌後のテンパイが既に持つ live wait そのままで、待ちを別経路で数え直さない。
        let case = &*ASYMMETRIC;
        let comparison = case.comparison();
        let wait = case
            .reach()
            .tenpai_wait
            .as_ref()
            .expect("選んだ打牌後の待ちを計算している");
        let opportunity = comparison
            .ron_opportunity
            .as_ref()
            .expect("ロンできる待ちなので Ron opportunity がある");

        assert_eq!(comparison.can_ron, Some(true));
        assert_eq!(
            opportunity
                .waits
                .iter()
                .map(|opportunity_wait| opportunity_wait.tile)
                .collect::<Vec<_>>(),
            wait.live_waits
        );
        assert!(
            opportunity
                .waits
                .iter()
                .all(|opportunity_wait| opportunity_wait.live_copies > 0)
        );
    }

    #[test]
    fn the_ron_opportunity_shares_the_tile_type_of_the_red_and_black_variants() {
        // Ron baseline は赤5 / 黒5を別 variant のまま残すが、structural safety は牌種1件を
        // 共有する。物理 variant ごとの打点と safety evidence を混同しない。
        let case = CaseSpec {
            hand: &RED_WAIT_HAND,
            draw: Some(RED_WAIT_DRAW),
            legal_dahai: Some(&[RED_WAIT_DRAW]),
            options: DiagnosticOptions::NONE,
            ..CaseSpec::default()
        }
        .build();
        let comparison = case.comparison();
        let opportunity = comparison
            .ron_opportunity
            .as_ref()
            .expect("ロンできる待ちなので Ron opportunity がある");
        let baseline = comparison
            .reach_ron_baseline
            .as_ref()
            .expect("リーチ Ron baseline を評価している");

        assert_eq!(
            opportunity
                .waits
                .iter()
                .map(|wait| (wait.tile, wait.live_copies))
                .collect::<Vec<_>>(),
            [(tile("5s"), 3)]
        );
        assert_eq!(
            baseline
                .winning_tile_values()
                .map(|value| (value.is_red(), value.remaining))
                .collect::<Vec<_>>(),
            [(true, 1), (false, 2)]
        );
    }

    #[test]
    fn the_reach_public_safety_matches_the_existing_defense_helpers() {
        // 公開 safety は既存 Defense helper の観測値そのもの。新しい rank も係数も作らない。
        let case = &*ASYMMETRIC;
        let opportunity = case
            .comparison()
            .ron_opportunity
            .as_ref()
            .expect("ロンできる待ちなので Ron opportunity がある");

        for wait in &opportunity.waits {
            let safety = wait
                .reach_public_safety
                .expect("リーチが合法なら公開 safety を評価する");

            assert!(safety.declaration_visible);
            assert_eq!(
                safety.genbutsu,
                is_genbutsu_for(wait.tile, 0, &case.context)
            );
            assert_eq!(
                safety.suited,
                suited_safety_evidence_for_players(wait.tile, &[0], &case.context)
            );
            assert_eq!(
                safety.honor.map(|honor| honor.rank),
                honor_safety_rank(wait.tile, &case.context)
            );
            // ダマ側には同じ rank を付けず、宣言が公開されない事実だけを持つ。
            assert!(!wait.damaten_declaration_visible);
        }
    }

    #[test]
    fn an_honor_wait_carries_the_existing_honor_safety() {
        // 南単騎。字牌の待ちは既存の字牌 safety と見え枚数を載せ、数牌 evidence は持たない。
        let case = &*ITTSUU;
        let opportunity = case
            .comparison()
            .ron_opportunity
            .as_ref()
            .expect("ロンできる待ちなので Ron opportunity がある");

        assert_eq!(
            opportunity
                .waits
                .iter()
                .map(|wait| wait.tile)
                .collect::<Vec<_>>(),
            [tile("S")]
        );
        let safety = opportunity.waits[0]
            .reach_public_safety
            .expect("リーチが合法なら公開 safety を評価する");
        let honor = safety.honor.expect("字牌の evidence がある");

        assert_eq!(
            Some(honor.rank),
            honor_safety_rank(tile("S"), &case.context)
        );
        assert_eq!(
            honor.visible_count,
            visible_count_of(tile("S"), &case.context)
        );
        assert_eq!(safety.suited, None);
    }

    #[test]
    fn an_open_hand_keeps_the_reach_public_safety_unavailable() {
        // 副露手ではリーチできないので、リーチ時の公開 safety は評価しない。ダマ側の事実と
        // 待ちそのものは残す。
        let case = &*OPEN;
        let comparison = case.comparison();
        let opportunity = comparison
            .ron_opportunity
            .as_ref()
            .expect("ロンはできるので Ron opportunity がある");

        assert!(!comparison.reach_legal);
        assert_eq!(comparison.can_ron, Some(true));
        assert!(!opportunity.waits.is_empty());
        for wait in &opportunity.waits {
            assert_eq!(wait.reach_public_safety, None);
            assert!(!wait.damaten_declaration_visible);
        }
    }

    #[test]
    fn a_furiten_tenpai_has_no_ron_opportunity() {
        // ロンできない待ちを確率候補のように並べない。0 として扱わず評価しないままにする。
        let case = CaseSpec {
            own_discards: &["1s", "5s"],
            ..CaseSpec::default()
        }
        .build();
        let comparison = case.comparison();

        assert_eq!(comparison.can_ron, Some(false));
        assert_eq!(comparison.ron_opportunity, None);
        // Tsumo 側は独立した軸なので評価したまま。
        assert!(comparison.reach_now_self_tsumo().is_some());
    }

    #[test]
    fn an_unknown_ron_availability_has_no_ron_opportunity() {
        // ロン可否が分からない局面では非フリテンだと推測しない。
        let case = CaseSpec {
            history_furiten: HistoryFuritenFacts::default(),
            ..ittsuu_spec()
        }
        .build();
        let comparison = case.comparison();

        assert_eq!(comparison.can_ron, None);
        assert_eq!(comparison.ron_opportunity, None);
    }

    #[test]
    fn the_external_threats_match_the_existing_sources() {
        for case in [&*ASYMMETRIC, &*ITTSUU, &*OPEN] {
            let threats = &case
                .comparison()
                .ron_opportunity
                .as_ref()
                .expect("ロンできる待ちなので Ron opportunity がある")
                .external_threats;

            assert_eq!(threats.reached_opponents, case.context.reached_opponents());
            assert_eq!(
                threats.high_open_hand_targets,
                high_open_hand_threat_players_from_context(&case.context)
            );
        }
    }

    // ---- 単位 ----

    #[test]
    fn the_comparison_only_carries_observations() {
        // self-tsumo と Ron baseline は単位の違う別の軸で、足した aggregate も winner も
        // 持たない。観測値以外のフィールドが増えるとこの構築が壊れる。
        let comparison = ReachDamatenComparisonDiagnostic {
            selected_discard: None,
            production_reason: ReachDecisionReason::NoLegalReach,
            production_should_reach: false,
            reach_legal: false,
            self_tsumo: None,
            reach_ron_baseline: None,
            can_ron: None,
            damaten_ron_value: None,
            ron_opportunity: None,
        };

        assert_eq!(comparison.reach_now_self_tsumo(), None);
        assert_eq!(comparison.damaten_continuation_self_tsumo(), None);
        assert_eq!(comparison.damaten_verdict(), None);
    }

    // 打牌候補の物理牌に対応する合法 Dahai。
    fn dahai(case: &Case, discard: &str) -> LegalAction {
        case.actions
            .iter()
            .find(|action| match action {
                LegalAction::Dahai { tile: id } => {
                    id.tile_type() == tile(discard) && id.is_red() == discard.ends_with('r')
                }
                _ => false,
            })
            .unwrap_or_else(|| panic!("打 {discard} が合法手にある"))
            .clone()
    }
}
