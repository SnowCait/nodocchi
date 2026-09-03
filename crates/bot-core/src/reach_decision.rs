//! 通常打牌 selection が選んだ打牌1件に対する Reach evaluation。
//!
//! [`DiscardActionSelection`] が持つ selected discard / evaluation / tenpai wait / Damaten
//! value を source of truth とし、未計算の値だけを選択済み1候補について補完する。通常打牌・
//! 向聴・受け入れ・current-tenpai candidate comparison は再計算しない。
//!
//! Reach / Damaten と timing の pure policy rule は [`crate::reach_policy`] が所有する。この module
//! は selected-tenpai の scoring facts と公開情報を集めてそれらの rule へ渡し、production decision
//! と diagnostic を同じ評価から返す。

use bot_logic::{
    DiscardEvaluation, PermanentFuriten, TenpaiCompletedHands, TenpaiWaitAvailability, TileType,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::damaten_value::{
    DamatenValueDiagnostic, DamatenValueVerdict, damaten_value_from_hands,
    tenpai_completed_hands_after_discard,
};
use crate::discard_selection::{DiscardActionSelection, selected_discard_tenpai_wait_availability};
use crate::offense_value::TenpaiOffenseMode;
use crate::open_hand_defense::high_open_hand_threat_players;
use crate::open_hand_threat::OpenHandThreatAssessment;
use crate::reach_policy::{
    NonFuritenBadWaitTimingFacts, ReachDecisionReason, ReachTimingDiagnostic,
    decide_non_furiten_bad_wait_reach_timing, decide_permanent_furiten_reach_timing,
    decide_reach_reason, evaluates_named_yakuman_damaten,
    evaluates_non_furiten_bad_wait_reach_timing, evaluates_reach_timing,
    selects_named_yakuman_damaten,
};
use crate::ron_opportunity::reach_public_safety_after_discard;
use crate::tenpai_continuation::selected_tenpai_self_tsumo_comparison;
use crate::tenpai_scoring::{NamedYakumanTsumo, tenpai_tsumo_named_yakuman};

// リーチを検討する打牌後の向聴数。
const REACH_TENPAI_SHANTEN: i8 = 0;

/// リーチ判断の構造化診断。
///
/// 契約:
///
/// - 判断材料は通常打牌 selection が選んだ打牌 ([`DiscardActionSelection`]) の評価だけで、リーチ
///   専用に向聴・受け入れ・待ち・フリテンを計算し直さない。`shanten_after_discard` は
///   [`DiscardEvaluation::min_shanten_after_discard`]、`tenpai_wait` のツモ側は
///   [`DiscardEvaluation::acceptance_after_discard`] そのもの。
/// - `selected` は `ShantenAgent::act()` が実際に採用したリーチそのもので、診断用の別判断
///   ロジックは持たない。
/// - 判定が進まなかった項目は推測せず `None` のままにする。
///
/// `tenpai_wait` のロン可否は、ダマ打点による判断を適用するかどうかの入口になる。ダマでロン
/// できる ([`TenpaiWaitAvailability::can_ron`] が `Some(true)`) 場合だけダマ打点を評価し、
/// フリテンとロン可否 unknown では非フリテンだと推測せず既存判断のままにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachDecisionDiagnostic {
    /// 通常打牌 selection が選んだ合法 Dahai。押し引き入力と同じ selection の action。
    pub selected_discard: Option<LegalAction>,
    /// `selected_discard` を切った後の向聴数。打牌を選べていない場合は `None`。
    pub shanten_after_discard: Option<i8>,
    /// `selected_discard` を切った後がテンパイの場合の待ちとロン可否。
    ///
    /// 構造上のアガリ牌種・生きた待ち・ツモ残枚数と種類数・恒常フリテン・自分の河と重複した
    /// 待ち牌・ロン可否に使った打牌後の履歴依存フリテンを持つ。テンパイにならない場合と
    /// リーチを検討しなかった場合は `None`。
    pub tenpai_wait: Option<TenpaiWaitAvailability>,
    /// `selected_discard` を切った後の待ちごとのダマ打点と、そこから畳んだ結論。
    ///
    /// ダマ打点を評価しなかった場合 (テンパイにならない・ダマでロンできない・ロン可否が
    /// unknown・打牌後の手牌を組み立てられない) は `None`。
    pub damaten_value: Option<DamatenValueDiagnostic>,
    /// 採用したリーチ。採用しなかった場合は `None`。
    pub selected: Option<LegalAction>,
    pub reason: ReachDecisionReason,
    /// base policy がリーチを選んだ場合の、そのリーチを今回宣言するかどうかの判断。
    ///
    /// base policy がダマを選んだ場合とリーチを検討できなかった場合は `None`。`reason` を
    /// この判断で上書きすることはない。
    pub timing: Option<ReachTimingDiagnostic>,
}

impl ReachDecisionDiagnostic {
    /// リーチすべきと判断したか。`selected` が `Some` であることと同値。
    pub fn should_reach(&self) -> bool {
        self.selected.is_some()
    }

    /// base policy はリーチを選んだか。timing で今回の宣言を見送った場合も `true`。
    pub fn base_selects_reach(&self) -> bool {
        self.reason.selects_reach()
    }

    /// base policy がリーチを選んだうえで、timing が今回の宣言を見送ったか。
    pub fn defers_reach(&self) -> bool {
        self.timing
            .as_ref()
            .is_some_and(ReachTimingDiagnostic::defers_reach)
    }

    /// 打牌後テンパイの恒常フリテン状態。テンパイにならない場合は `None`。
    pub fn permanent_furiten(&self) -> Option<PermanentFuriten> {
        self.tenpai_wait
            .as_ref()
            .map(TenpaiWaitAvailability::permanent_furiten)
    }

    /// 恒常フリテンと打牌後の履歴依存フリテンを合わせた総合ロン可否。テンパイにならない場合と
    /// 判断できない場合は `None`。
    pub fn can_ron(&self) -> Option<bool> {
        self.tenpai_wait
            .as_ref()
            .and_then(TenpaiWaitAvailability::can_ron)
    }

    /// ツモ和了できる待ちの残枚数。テンパイにならない場合は `None`。
    ///
    /// 選ばれた [`DiscardEvaluation::acceptance_total_remaining`] と常に一致する。
    pub fn tsumo_remaining(&self) -> Option<u8> {
        self.tenpai_wait.as_ref().map(|wait| wait.tsumo_remaining)
    }

    /// ツモ和了できる待ちの種類数。テンパイにならない場合は `None`。
    ///
    /// 選ばれた [`DiscardEvaluation::acceptance_type_count`] と常に一致する。
    pub fn tsumo_type_count(&self) -> Option<usize> {
        self.tenpai_wait.as_ref().map(|wait| wait.tsumo_type_count)
    }

    /// 自分の河と重複した待ち牌。テンパイにならない場合は空。
    pub fn discarded_waits(&self) -> &[TileType] {
        self.tenpai_wait
            .as_ref()
            .map_or(&[], TenpaiWaitAvailability::discarded_waits)
    }

    /// ダマ打点から畳んだ結論。ダマ打点を評価しなかった場合は `None`。
    pub fn damaten_verdict(&self) -> Option<DamatenValueVerdict> {
        self.damaten_value
            .as_ref()
            .map(|damaten_value| damaten_value.verdict)
    }

    /// ダマ打点による判断でリーチ / ダマを決めたか。
    ///
    /// ダマ打点を評価しなかった場合と、評価しても結論が出なかった場合はどちらも `false`。
    pub fn used_damaten_value(&self) -> bool {
        self.damaten_verdict()
            .is_some_and(DamatenValueVerdict::is_conclusive)
    }
}

// リーチ判断の結果と、その判断のためにここで組み立てた打牌後テンパイの完成手。
//
// 完成手はダマ打点を評価するために組み立てたものそのもの。統合診断のリーチ Ron baseline も
// 同じ集合を別の baseline で評価できるよう、判断の後で捨てずに返すだけで、この診断のために
// 組み立てる条件は増やさない。ダマ打点を評価しない経路 (ダマでロンできない・ロン可否 unknown)
// と、通常打牌 selection が既にダマ打点を評価済みの経路では組み立てないので `None`。act() は
// 診断を作らないのでそのまま捨てる。
pub(crate) struct ReachDecision {
    pub(crate) diagnostic: ReachDecisionDiagnostic,
    pub(crate) hands: Option<TenpaiCompletedHands>,
}

// リーチ判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 判断材料は通常打牌 selection が選んだ打牌の評価だけで、リーチ専用に手牌から向聴・受け入れを
// 計算し直さない。現在聴牌の比較で待ち・ダマ打点を計算済みならその結果を共有し、未計算の経路
// だけ選択済み1候補分を既存 pure helper から求める。全合法候補分の診断や2手先探索は構築しない。
//
// ダマでロンできる通常のケースでは、その打牌後の全ての生きた待ち・赤黒 variant についてダマの
// 確定打点を評価し、その結論だけでリーチ / ダマを決める。待ち枚数の threshold
// (REACH_MIN_REMAINING) をダマ打点より先に適用してリーチを抑制しない。したがって1～2枚待ちでも
// ダマが安ければリーチする。
//
// リーチ / ダマの条件は decide_reach_reason() が source of truth で、押し引きが攻撃打点を求める
// ときの攻撃モードもそこを共有する。ここでは合法 Reach と打牌後テンパイを確認して結論を載せる
// だけにする。
//
// フリテン・ロン可否 unknown・ダマ打点を確定できない場合は、非フリテンだともダマ打点が十分だとも
// 推測せず、待ち枚数だけを見る既存判断を維持する。
// TODO: フリテン専用のリーチ policy を決める。
pub(crate) fn decide_reach(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    selection: &DiscardActionSelection,
    open_hand_threats: &[OpenHandThreatAssessment; 4],
) -> ReachDecision {
    let mut diagnostic = ReachDecisionDiagnostic {
        selected_discard: selection.action.clone(),
        shanten_after_discard: selection
            .evaluation
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard),
        tenpai_wait: None,
        damaten_value: None,
        selected: None,
        reason: ReachDecisionReason::NoLegalReach,
        timing: None,
    };

    // 合法手にリーチが無ければ待ちもフリテンも求めない。
    let Some(reach) = legal_actions
        .iter()
        .find(|action| matches!(action, LegalAction::Reach))
        .cloned()
    else {
        return ReachDecision {
            diagnostic,
            hands: None,
        };
    };

    // DiscardActionSelection の不変条件より、evaluation が Some なら action も Some。
    let Some(evaluation) = selection.evaluation.as_ref() else {
        diagnostic.reason = ReachDecisionReason::NoSelectedDiscard;
        return ReachDecision {
            diagnostic,
            hands: None,
        };
    };

    if evaluation.min_shanten_after_discard() != REACH_TENPAI_SHANTEN {
        diagnostic.reason = ReachDecisionReason::NotTenpai;
        return ReachDecision {
            diagnostic,
            hands: None,
        };
    }

    let Some(tenpai_wait) = selection
        .tenpai_wait
        .clone()
        .or_else(|| selected_discard_tenpai_wait_availability(ctx, evaluation))
    else {
        diagnostic.reason = ReachDecisionReason::NotTenpai;
        return ReachDecision {
            diagnostic,
            hands: None,
        };
    };

    // ダマでロンできると確定した場合だけダマ打点を評価する。フリテンとロン可否 unknown では
    // 評価そのものを行わず、既存判断へ委ねる。現在聴牌比較が評価済みならその結論をそのまま使う。
    //
    // 完成手を組み立てるのは、この経路で実際にダマ打点を評価する場合だけ。組み立てた集合は
    // 統合診断のリーチ Ron baseline へそのまま渡せるよう返すが、そのために組み立てる条件を
    // 増やさない。
    let can_ron = tenpai_wait.can_ron() == Some(true);
    let hands = (can_ron && selection.damaten_value.is_none())
        .then(|| tenpai_completed_hands_after_discard(ctx, evaluation, &tenpai_wait))
        .flatten();
    let damaten_value = can_ron
        .then(|| {
            selection.damaten_value.clone().or_else(|| {
                hands
                    .as_ref()
                    .map(|hands| damaten_value_from_hands(ctx, hands))
            })
        })
        .flatten();

    // 待ち枚数は既存受け入れそのもので、visible tiles の反映も打牌評価の時点で済んでいる。
    // リーチ / ダマの条件そのものは押し引きの攻撃打点と共有し、ここで書き直さない。
    //
    // 恒常フリテンの named 役満だけは、ダマ打点 threshold とは別の categorical rule でダマに
    // する。ロンできない聴牌なのでダマ打点は評価しておらず、材料は既存 scoring の Tsumo 評価
    // だけになる。
    let reason = if selects_named_yakuman_damaten(
        tenpai_wait.permanent_furiten(),
        tenpai_wait.tsumo_remaining,
        tsumo_named_yakuman(ctx, evaluation, &tenpai_wait).is_established(),
    ) {
        ReachDecisionReason::NamedYakumanDamaten
    } else {
        decide_reach_reason(
            true,
            damaten_value.as_ref().map(|value| value.verdict),
            tenpai_wait.tsumo_remaining,
        )
    };

    // timing はここまでの base policy を上書きしない。base policy がリーチを選んだ場合だけ、
    // そのリーチを今回宣言するかどうかを決める層として後ろに続く。
    let timing = reason
        .selects_reach()
        .then(|| reach_timing(ctx, selection, evaluation, &tenpai_wait, open_hand_threats));

    diagnostic.reason = reason;
    diagnostic.selected = timing
        .filter(|timing| !timing.defers_reach())
        .map(|_| reach);
    diagnostic.timing = timing;
    diagnostic.damaten_value = damaten_value;
    diagnostic.tenpai_wait = Some(tenpai_wait);
    ReachDecision { diagnostic, hands }
}

// 打牌後テンパイのツモ和了が named 役満と確定するか。
//
// 役満判定は既存 scoring の結論そのもので、牌姿からも点数 threshold からも役満を推測しない。
// 評価するモードは、リーチを宣言しない場合の Tsumo baseline (Damaten) 1つだけ。categorical rule
// の対象になる恒常フリテン聴牌でしか完成手を組み立てず、それ以外では点数計算も走らせない。
fn tsumo_named_yakuman(
    ctx: &GameContext,
    evaluation: &DiscardEvaluation,
    tenpai_wait: &TenpaiWaitAvailability,
) -> NamedYakumanTsumo {
    if !evaluates_named_yakuman_damaten(
        tenpai_wait.permanent_furiten(),
        tenpai_wait.tsumo_remaining,
    ) {
        return NamedYakumanTsumo::NotEstablished;
    }

    tenpai_completed_hands_after_discard(ctx, evaluation, tenpai_wait)
        .map_or(NamedYakumanTsumo::NotEstablished, |hands| {
            tenpai_tsumo_named_yakuman(ctx, &hands, TenpaiOffenseMode::Damaten)
        })
}

// base policy がリーチを選んだ聴牌の timing 判断。
//
// 恒常フリテンに加え、非フリテンでは「生きた待ち1種・3枚以下・非么九牌・Reach 後に非現物かつ
// 壁なし・無スジ・external threat なし」の暫定 structural gate をすべて満たす場合だけ対象に
// する。これらは Ron probability の代用ではなく、公開 safety evidence 上の安全根拠が無いこと
// だけを見る。
//
// 対象になった場合に評価するのは通常打牌 selection が選んだ1候補だけで、全合法 Dahai 候補の
// 継続枝は production では構築しない。比較する値も既存 self-tsumo 比較そのもので、この経路が
// 確率模型も点数計算も持たない。現在聴牌候補の比較が同じ candidate の継続 timing を評価済みなら、
// その結論をそのまま使い同じ2手先評価を繰り返さない。
fn reach_timing(
    ctx: &GameContext,
    selection: &DiscardActionSelection,
    evaluation: &DiscardEvaluation,
    tenpai_wait: &TenpaiWaitAvailability,
    open_hand_threats: &[OpenHandThreatAssessment; 4],
) -> ReachTimingDiagnostic {
    if evaluates_reach_timing(tenpai_wait.permanent_furiten(), tenpai_wait.tsumo_remaining) {
        // 現在聴牌候補の比較が同じ gate と同じ timing policy で評価済みなら、その結論そのもの。
        if let Some(timing) = selection.tenpai_reach_timing {
            return timing;
        }

        // ここへ来るのは合法手にリーチがあった経路だけ。現在局面のリーチ可否は決め直さない。
        let comparison = selected_tenpai_self_tsumo_comparison(ctx, evaluation, true);
        return decide_permanent_furiten_reach_timing(
            comparison.and_then(|comparison| comparison.reach_now),
            comparison.and_then(|comparison| comparison.defer_forced_reach()),
        );
    }

    if tenpai_wait.permanent_furiten() != PermanentFuriten::No {
        return ReachTimingDiagnostic::not_evaluated();
    }

    // public-state projection より前に、既存の打牌後待ちと threat classification だけで判定できる
    // gate を落とす。対象外では selected candidate continuation も public safety も評価しない。
    if tenpai_wait.can_ron() != Some(true)
        || tenpai_wait.live_waits.len() != 1
        || tenpai_wait.tsumo_remaining == 0
        || tenpai_wait.tsumo_remaining > 3
    {
        return ReachTimingDiagnostic::non_furiten_heuristic_not_evaluated();
    }

    let wait_tile = tenpai_wait.live_waits[0];
    let wait_is_non_yaochu = !wait_tile.is_yaochu();
    if !wait_is_non_yaochu {
        return ReachTimingDiagnostic::non_furiten_heuristic_not_evaluated();
    }

    let reached_opponents = ctx.reached_opponents();
    let high_open_hand_targets = high_open_hand_threat_players(open_hand_threats);
    if !reached_opponents.is_empty() || !high_open_hand_targets.is_empty() {
        return ReachTimingDiagnostic::non_furiten_heuristic_not_evaluated();
    }

    let safety = selection.action.as_ref().and_then(|selected_discard| {
        reach_public_safety_after_discard(ctx, selected_discard, wait_tile)
    });
    let facts = NonFuritenBadWaitTimingFacts {
        permanent_furiten: tenpai_wait.permanent_furiten(),
        can_ron: tenpai_wait.can_ron(),
        live_wait_type_count: tenpai_wait.live_waits.len(),
        live_copies: tenpai_wait.tsumo_remaining,
        wait_is_non_yaochu,
        reach_genbutsu: safety.map(|safety| safety.genbutsu),
        wall_rank: safety.and_then(|safety| safety.suited.map(|suited| suited.wall_rank)),
        suji_rank: safety.and_then(|safety| safety.suited.map(|suited| suited.suji_rank)),
        reached_opponent_count: reached_opponents.len(),
        high_open_hand_target_count: high_open_hand_targets.len(),
    };
    if !evaluates_non_furiten_bad_wait_reach_timing(facts) {
        return ReachTimingDiagnostic::non_furiten_heuristic_not_evaluated();
    }

    let comparison = selected_tenpai_self_tsumo_comparison(ctx, evaluation, true);
    decide_non_furiten_bad_wait_reach_timing(
        comparison.and_then(|comparison| comparison.reach_now),
        comparison.and_then(|comparison| comparison.defer_forced_reach()),
    )
}
