use bot_core::{
    CallCandidateDiagnostic, CallDecisionDiagnostic, CallDecisionReason, CallIishantenComparison,
    CallThreeShantenPassEvaluation, CallTwoShantenPassEvaluation, CombinedDefenseCategory,
    DamatenValue, DamatenValueDiagnostic, DamatenValueVerdict, DefenseDecisionDiagnostic,
    DefenseFallbackKind, GameContext, LegalAction, OpenHandDefenseCategory, PushPullMode,
    PushPullReason, ReachDecisionDiagnostic, ReachDecisionReason, ReachTimingReason,
    RyukyokuDecisionDiagnostic, RyukyokuVerdict, ShantenDecisionDiagnostic,
    StrongTenpaiRequirement, TenpaiOffenseValue,
};
use bot_logic::{PermanentFuriten, TileId, TileType};

use crate::ranked_choice::{AnalysisOpponentHonorValue, RankedChoice, rank_choices};

/// 1局面の production 判断を consumer 向けに投影した構造化結果。
///
/// `GameContext` と合法手、そしてその局面で既に得ている primary [`ShantenDecisionDiagnostic`]
/// から作る。手入力 scenario か replay かといった局面の出所には依存しない。
/// 診断そのものを公開せず、consumer が必要とする値だけを薄く写す。表示用の文字列は作らず、
/// 既存の enum と数値をそのまま保持するので、CLI formatter と Web が同じ結果を読める。
///
/// この投影のために判断をやり直さない。choice 2 以降だけ [`rank_choices`] が既存 production
/// 再診断を行い、それ以外の section は primary 診断が持つ値の転記に留める。
///
/// 欠けている値は理由ごとに形を変える。section 自体が無い (`None`)、評価していない、評価した
/// が選択が無い、値を確定できないをそれぞれ別の状態として持ち、consumer が欠落の理由を推測
/// しなくて済むようにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisResult {
    /// 上位から順に並べた選択肢。`choice_limit` 件までで、先頭が primary 診断そのもの。
    pub choices: Vec<RankedChoice>,
    /// 九種九牌が合法だった局面の判断。合法でなければ `None`。
    pub ryukyoku: Option<AnalysisRyukyoku>,
    /// 押し引きまで進んだ局面の判断。進まなかった場合は `None`。
    pub push_pull: Option<AnalysisPushPull>,
    /// リーチ判断。評価していない状態と判断そのものが無い状態を区別する。
    pub reach: AnalysisReach,
    /// 鳴きを検討した局面の判断。合法な Chi / Pon が無ければ `None`。
    pub call: Option<AnalysisCall>,
    /// 採用経路まで含めた防御判断。どの防御も評価していない場合は `None`。
    pub defense: Option<AnalysisDefense>,
}

impl AnalysisResult {
    /// 局面と primary 診断から解析結果を投影する。
    ///
    /// `choice_limit` は [`choices`](Self::choices) に並べる件数の上限で、consumer が決める。
    /// primary 診断はここで取り直さず、渡されたものをそのまま choice 1 として使う。
    pub fn from_decision(
        context: &GameContext,
        legal_actions: &[LegalAction],
        diagnostic: &ShantenDecisionDiagnostic,
        choice_limit: usize,
    ) -> Self {
        Self {
            choices: rank_choices(context, legal_actions, diagnostic, choice_limit),
            ryukyoku: diagnostic.ryukyoku.as_ref().map(ryukyoku),
            push_pull: push_pull(diagnostic),
            reach: reach(diagnostic),
            call: diagnostic.call.as_ref().map(call),
            defense: defense(diagnostic),
        }
    }
}

/// 九種九牌を宣言するか続行するかの判断。
///
/// 向聴数は判断に使った値そのもので、手牌を評価できなかった軸は `None` のままにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRyukyoku {
    pub verdict: RyukyokuVerdict,
    pub standard_shanten: Option<i8>,
    pub chiitoitsu_shanten: Option<i8>,
    pub kokushi_shanten: Option<i8>,
}

/// 押し引きの結論と、打牌後テンパイの攻撃材料。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisPushPull {
    pub mode: PushPullMode,
    pub reason: PushPullReason,
    /// 打牌後がテンパイになる場合の攻撃材料。テンパイにならない局面と、攻撃側を評価して
    /// いない局面では `None`。
    pub tenpai_offense: Option<AnalysisTenpaiOffense>,
}

/// 押し引きが見た打牌後テンパイの待ちと攻撃打点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisTenpaiOffense {
    /// ツモ和了できる牌の残枚数。
    pub live_wait_remaining: u8,
    /// ツモ和了できる牌の種類数。
    pub live_wait_type_count: usize,
    pub permanent_furiten: PermanentFuriten,
    /// 恒常フリテンの観点でロンできるか。判断できない場合は `None`。
    pub can_ron: Option<bool>,
    /// 攻撃を継続した場合の攻撃モードと確定打点。評価していない場合は `None` で、評価しても
    /// 打点を確定できなかった場合は `Some` のまま
    /// [`OffenseValue::Unknown`](bot_core::OffenseValue::Unknown) になる。
    pub value: Option<TenpaiOffenseValue>,
    /// 押すために要求する条件。恒常フリテンが unknown で条件そのものが決まらない場合は `None`。
    ///
    /// production input から一度だけ求めた値で、consumer 側で判定し直さない。
    pub strong_tenpai_requirement: Option<StrongTenpaiRequirement>,
}

/// リーチ判断の状態。
///
/// 「判断まで進まなかった」「進んだが評価していない」「評価した」を潰さずに区別する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisReach {
    /// 押し引きより手前で決着し、リーチ判断そのものが無い。
    Absent,
    /// 押し引きは評価したが、リーチを検討する局面ではなかった。
    NotEvaluated,
    Evaluated(AnalysisReachDecision),
}

/// 評価済みリーチ判断のうち consumer が見る値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReachDecision {
    pub verdict: AnalysisReachVerdict,
    /// base policy の理由。timing で見送った場合もこの理由は上書きしない。
    pub base_reason: ReachDecisionReason,
    /// 今回の宣言を見送った理由。[`AnalysisReachVerdict::Deferred`] のときだけ `Some`。
    pub timing_reason: Option<ReachTimingReason>,
    /// 打牌後テンパイの待ち。テンパイにならない場合は `None`。
    pub tenpai_wait: Option<AnalysisReachTenpaiWait>,
    /// ダマ打点。評価しなかった場合は `None`。
    pub damaten: Option<AnalysisDamaten>,
}

/// 今回リーチを宣言するかどうか。
///
/// 表すのは採否だけで、宣言しない理由は持たない。リーチが合法でない・テンパイでない・base
/// policy がダマを選んだのどれも [`NoReach`](Self::NoReach) で、その区別は
/// [`AnalysisReachDecision::base_reason`] が source of truth。ダマ打点を評価したかどうかと
/// その結論は [`AnalysisReachDecision::damaten`] が持つ。
///
/// base policy がリーチを選んだうえで timing が今回の宣言を見送った局面だけは、宣言しない
/// 他の局面と区別して [`Deferred`](Self::Deferred) にする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisReachVerdict {
    /// 今回リーチを宣言する。
    Reach,
    /// 今回リーチを宣言しない。
    NoReach,
    /// base policy はリーチを選んだが、timing が今回の宣言を見送った。
    Deferred,
}

/// リーチ判断が見た打牌後テンパイの待ち。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisReachTenpaiWait {
    pub live_wait_remaining: u8,
    pub live_wait_type_count: usize,
    /// 恒常フリテンと打牌後の履歴依存フリテンを合わせたロン可否。判断できない場合は `None`。
    pub can_ron: Option<bool>,
}

/// ダマ打点の結論と、和了牌の物理牌ごとの打点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisDamaten {
    pub verdict: DamatenValueVerdict,
    /// 和了牌の物理牌ごとのダマ打点。生きた待ちが無ければ空。
    pub winning_tiles: Vec<AnalysisDamatenWinningTile>,
}

/// 和了牌の物理牌1つ分のダマ打点。赤5と黒5は別の要素として並ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisDamatenWinningTile {
    pub winning_tile: TileId,
    pub value: DamatenValue,
}

/// 鳴き判断。`selected` が `None` なら鳴かない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCall {
    pub selected: Option<LegalAction>,
    pub reason: CallDecisionReason,
    /// `reason` がどの候補に由来するか。
    pub reason_source: AnalysisCallReasonSource,
    /// consumer へ見せる比較対象の候補。採用候補があればその候補、無ければ self-tsumo 比較を
    /// 持つ最初の候補で、どちらも比較を持たなければ `None`。
    pub compared_candidate: Option<AnalysisCallCandidate>,
}

/// [`AnalysisCall::reason`] の由来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisCallReasonSource {
    /// 採用した候補が成立した理由。
    SelectedCandidate,
    /// 採用が無く、最初の候補が落ちた理由。
    FirstCandidate,
    /// 候補が1件も無く、由来を特定できない。
    NoCandidate,
}

/// 比較対象として見せる鳴き候補。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCallCandidate {
    pub action: LegalAction,
    /// この候補自身の理由。
    pub reason: CallDecisionReason,
    /// この候補が [`AnalysisCall::reason`] の由来と同じか。
    pub is_reason_source: bool,
    /// 鳴いた直後の最良打牌。打牌候補を評価しなかった場合は `None`。
    pub post_call_discard: Option<AnalysisDiscardTile>,
    /// production が実際に使った Call / Pass の self-tsumo 比較。
    pub self_tsumo: AnalysisCallSelfTsumo,
}

/// 打牌の物理牌。赤5と黒5を潰さずに持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisDiscardTile {
    pub tile: TileType,
    pub red_five: bool,
}

/// 鳴き候補の self-tsumo 比較。起点の向聴数ごとに尺度も Pass 側の評価も違うので混ぜない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisCallSelfTsumo {
    /// 鳴いても1向聴のままの候補の比較。
    Iishanten(AnalysisCallSelfTsumoComparison),
    /// 現在2向聴から鳴き後の最良打牌で1向聴になる候補の比較。
    TwoShanten {
        pass_evaluation: CallTwoShantenPassEvaluation,
        comparison: AnalysisCallSelfTsumoComparison,
    },
    /// 現在3向聴から鳴き後の最良打牌で2向聴になる候補の比較。
    ThreeShanten {
        pass_evaluation: CallThreeShantenPassEvaluation,
        comparison: AnalysisCallSelfTsumoComparison,
    },
}

/// Call / Pass の ExpectedSelfTsumoValue [[`bot_logic::SELF_TSUMO_VALUE_SCALE`]] と結論。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisCallSelfTsumoComparison {
    pub pass_expected_self_tsumo_value: Option<u64>,
    pub call_expected_self_tsumo_value: Option<u64>,
    pub verdict: CallIishantenComparison,
}

/// 採用した防御 fallback の経路。
///
/// リーチ者向け防御・複合 threat 防御・非リーチ相手向け防御の優先順で1つだけ選ぶ。
/// リーチ者向け防御だけは「評価したが選択が無い」を別の状態として残す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisDefense {
    /// リーチ者向け防御を評価したが、採用した action が無い。
    ReachThreatWithoutSelection,
    ReachThreat(AnalysisReachThreatDefense),
    CombinedThreat {
        action: LegalAction,
        category: CombinedDefenseCategory,
    },
    OpenHand {
        action: LegalAction,
        category: OpenHandDefenseCategory,
    },
}

/// リーチ者向け防御 fallback の選択結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReachThreatDefense {
    pub action: LegalAction,
    pub kind: DefenseFallbackKind,
    /// [`DefenseFallbackKind::HonorSafety`] を選んだ場合の、相手にとっての役牌価値。
    /// 他の種別では `None`。
    pub opponent_honor_value: Option<AnalysisOpponentHonorValue>,
}

fn ryukyoku(ryukyoku: &RyukyokuDecisionDiagnostic) -> AnalysisRyukyoku {
    AnalysisRyukyoku {
        verdict: ryukyoku.verdict,
        standard_shanten: ryukyoku.standard_shanten(),
        chiitoitsu_shanten: ryukyoku.chiitoitsu_shanten(),
        kokushi_shanten: ryukyoku.kokushi_shanten(),
    }
}

fn push_pull(diagnostic: &ShantenDecisionDiagnostic) -> Option<AnalysisPushPull> {
    let decision = diagnostic.push_pull_decision?;
    Some(AnalysisPushPull {
        mode: decision.mode,
        reason: decision.reason,
        tenpai_offense: tenpai_offense(diagnostic),
    })
}

fn tenpai_offense(diagnostic: &ShantenDecisionDiagnostic) -> Option<AnalysisTenpaiOffense> {
    let inputs = diagnostic.push_pull_inputs.as_ref()?;
    let offense = inputs.offense?;
    let wait = offense.tenpai_wait_after_discard?;
    Some(AnalysisTenpaiOffense {
        live_wait_remaining: wait.tsumo_remaining,
        live_wait_type_count: wait.tsumo_type_count,
        permanent_furiten: wait.permanent_furiten,
        can_ron: wait.can_ron,
        value: offense.tenpai_offense_value_after_discard,
        strong_tenpai_requirement: offense.strong_tenpai_requirement(inputs.dealer_reacher),
    })
}

fn reach(diagnostic: &ShantenDecisionDiagnostic) -> AnalysisReach {
    let Some(reach) = diagnostic.reach.as_ref() else {
        return match diagnostic.push_pull_decision {
            Some(_) => AnalysisReach::NotEvaluated,
            None => AnalysisReach::Absent,
        };
    };
    AnalysisReach::Evaluated(reach_decision(reach))
}

fn reach_decision(reach: &ReachDecisionDiagnostic) -> AnalysisReachDecision {
    let deferred = reach.defers_reach();
    AnalysisReachDecision {
        verdict: match (deferred, reach.should_reach()) {
            (true, _) => AnalysisReachVerdict::Deferred,
            (false, true) => AnalysisReachVerdict::Reach,
            (false, false) => AnalysisReachVerdict::NoReach,
        },
        base_reason: reach.reason,
        timing_reason: deferred
            .then(|| reach.timing.as_ref().map(|timing| timing.reason))
            .flatten(),
        tenpai_wait: reach
            .tenpai_wait
            .as_ref()
            .map(|wait| AnalysisReachTenpaiWait {
                live_wait_remaining: wait.tsumo_remaining,
                live_wait_type_count: wait.tsumo_type_count,
                can_ron: wait.can_ron(),
            }),
        damaten: reach.damaten_value.as_ref().map(damaten),
    }
}

fn damaten(damaten: &DamatenValueDiagnostic) -> AnalysisDamaten {
    AnalysisDamaten {
        verdict: damaten.verdict,
        winning_tiles: damaten
            .winning_tile_values()
            .map(|winning_tile| AnalysisDamatenWinningTile {
                winning_tile: winning_tile.winning_tile,
                value: winning_tile.value,
            })
            .collect(),
    }
}

// 鳴き判断のうち consumer が見る候補を1件だけ選んで投影する。候補集合そのものは公開しない。
fn call(call: &CallDecisionDiagnostic) -> AnalysisCall {
    let selected_index = selected_call_candidate(call);
    let reason_index = selected_index.or_else(|| (!call.candidates.is_empty()).then_some(0));
    let compared_index = selected_index.or_else(|| {
        call.candidates
            .iter()
            .position(|candidate| call_self_tsumo(candidate).is_some())
    });

    AnalysisCall {
        selected: call.selected.clone(),
        reason: call.reason,
        reason_source: match (selected_index, reason_index) {
            (Some(_), _) => AnalysisCallReasonSource::SelectedCandidate,
            (None, Some(_)) => AnalysisCallReasonSource::FirstCandidate,
            (None, None) => AnalysisCallReasonSource::NoCandidate,
        },
        compared_candidate: compared_index
            .and_then(|index| call_candidate(&call.candidates[index], reason_index == Some(index))),
    }
}

fn selected_call_candidate(call: &CallDecisionDiagnostic) -> Option<usize> {
    call.candidates
        .iter()
        .position(|candidate| candidate.selected)
}

fn call_candidate(
    candidate: &CallCandidateDiagnostic,
    is_reason_source: bool,
) -> Option<AnalysisCallCandidate> {
    Some(AnalysisCallCandidate {
        action: candidate.action.clone(),
        reason: candidate.reason,
        is_reason_source,
        post_call_discard: candidate.post_call_discard.as_ref().map(|discard| {
            AnalysisDiscardTile {
                tile: discard.discard,
                red_five: discard.discards_red_five,
            }
        }),
        self_tsumo: call_self_tsumo(candidate)?,
    })
}

// 起点の向聴数ごとに1つだけ持つ比較。production が実際に使った比較そのものを写す。
fn call_self_tsumo(candidate: &CallCandidateDiagnostic) -> Option<AnalysisCallSelfTsumo> {
    if let Some(compared) = candidate.iishanten_self_tsumo.as_ref() {
        return Some(AnalysisCallSelfTsumo::Iishanten(
            AnalysisCallSelfTsumoComparison {
                pass_expected_self_tsumo_value: compared.pass_expected_self_tsumo_value,
                call_expected_self_tsumo_value: compared.call_expected_self_tsumo_value,
                verdict: compared.comparison,
            },
        ));
    }
    if let Some(compared) = candidate.two_shanten_self_tsumo.as_ref() {
        return Some(AnalysisCallSelfTsumo::TwoShanten {
            pass_evaluation: compared.pass_evaluation,
            comparison: AnalysisCallSelfTsumoComparison {
                pass_expected_self_tsumo_value: compared.pass_expected_self_tsumo_value,
                call_expected_self_tsumo_value: compared.call_expected_self_tsumo_value,
                verdict: compared.comparison,
            },
        });
    }
    let compared = candidate.three_shanten_self_tsumo.as_ref()?;
    Some(AnalysisCallSelfTsumo::ThreeShanten {
        pass_evaluation: compared.pass_evaluation,
        comparison: AnalysisCallSelfTsumoComparison {
            pass_expected_self_tsumo_value: compared.pass_expected_self_tsumo_value,
            call_expected_self_tsumo_value: compared.call_expected_self_tsumo_value,
            verdict: compared.comparison,
        },
    })
}

// 防御の source は「リーチ者向け → 複合 threat → 非リーチ相手」の優先順で1つだけ選ぶ。
// リーチ者向け防御を評価した局面では、採用が無くてもその事実を残し、他の source へ落とさない。
fn defense(diagnostic: &ShantenDecisionDiagnostic) -> Option<AnalysisDefense> {
    if let Some(defense) = diagnostic.defense.as_ref() {
        return Some(match reach_threat_defense(defense) {
            Some(selected) => AnalysisDefense::ReachThreat(selected),
            None => AnalysisDefense::ReachThreatWithoutSelection,
        });
    }

    if let Some(selected) = diagnostic.combined_defense.selected.as_ref() {
        return Some(AnalysisDefense::CombinedThreat {
            action: selected.selected_action.clone(),
            category: selected.selected_category,
        });
    }

    let selected = diagnostic.open_hand_defense.selected.as_ref()?;
    Some(AnalysisDefense::OpenHand {
        action: selected.selected_action.clone(),
        category: selected.selected_category,
    })
}

// 選択結果の物理牌は、選択済み候補が持つ合法 action をそのまま使う。
fn reach_threat_defense(defense: &DefenseDecisionDiagnostic) -> Option<AnalysisReachThreatDefense> {
    let selected = defense.selected.as_ref()?;
    let action = defense
        .candidates
        .iter()
        .find(|candidate| candidate.selected)?
        .action
        .clone();

    Some(AnalysisReachThreatDefense {
        action,
        kind: selected.selected_kind,
        opponent_honor_value: matches!(selected.selected_kind, DefenseFallbackKind::HonorSafety(_))
            .then(|| match selected.selected_opponent_honor_value {
                Some(value) => AnalysisOpponentHonorValue::Known(value),
                None => AnalysisOpponentHonorValue::Unknown,
            }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Scenario, ScenarioSpec};
    use bot_core::{
        CallKind, CallTwoShantenSelfTsumoDiagnostic, CallTwoShantenSpeedDiagnostic,
        CombinedDefenseCategory, OpenHandDefenseCategory, OpponentHonorValue, ShantenAgent,
        TenpaiOffenseMode,
    };
    use bot_logic::{DiscardEvaluation, TileCounts, select_best_discard};

    const NORMAL_SCENARIO: &str = r#"{
        "hand": "234m455p789s1123z",
        "draw": "N",
        "dora_indicators": "3p",
        "round_wind": "E",
        "seat_wind": "S",
        "player_id": 0,
        "oya": 3
    }"#;

    // 打 北 で 2p / 5p の両面テンパイになり、リーチを選ぶ局面。
    const REACH_SCENARIO: &str = r#"{
        "hand": "123456789m34p55s",
        "draw": "N"
    }"#;

    // 単独の子リーチに対する2向聴。押し引きは降りを選び、リーチ判断まで進まない。
    const DEFENSE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "dora_indicators": "3p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 1,
        "reached": [false, true, false, false],
        "discards": ["", "1m 4m 7p E", "", ""]
    }"#;

    // 単独の子リーチに対する 6s 待ち3枚のテンパイ。ダマ 7700 なので打点を理由に押す。
    const HIGH_VALUE_TENPAI_UNDER_REACH_SCENARIO: &str = r#"{
        "hand": "234678m 22p 34455s",
        "draw": "N",
        "dora_indicators": "1p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 2,
        "reached": [false, true, false, false],
        "extra_visible_tiles": "333s 6s",
        "history_furiten": { "same_turn": false, "riichi_missed_win": false }
    }"#;

    // 場風・自風が不明で攻撃打点を確定できない、単独の子リーチに対するテンパイ。
    const TENPAI_UNDER_REACH_SCENARIO: &str = r#"{
        "hand": "234m 567m 88m 345p 67p",
        "draw": "N",
        "player_id": 0,
        "oya": 2,
        "reached": [false, true, false, false],
        "discards": ["1s", "9m", "", ""]
    }"#;

    // 打 北 で 3s / 6s の両面テンパイになる平和 + 断幺。ドラだけを変えて、ダマ打点が
    // threshold 以上 / 未満の対照ケースにする。
    const DAMATEN_HIGH_VALUE_SCENARIO: &str = r#"{
        "hand": "234678m 22p 34455s",
        "draw": "N",
        "dora_indicators": "1p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 3,
        "extra_visible_tiles": "333s",
        "history_furiten": { "same_turn": false, "riichi_missed_win": false }
    }"#;

    const DAMATEN_LOW_VALUE_SCENARIO: &str = r#"{
        "hand": "234678m 22p 34455s",
        "draw": "N",
        "dora_indicators": "1m",
        "round_wind": "E",
        "player_id": 0,
        "oya": 3,
        "extra_visible_tiles": "333s",
        "history_furiten": { "same_turn": false, "riichi_missed_win": false }
    }"#;

    // 九種九牌が成立する么九牌9種・対子なしの手牌。国士4向聴で通常手・七対子も遠い。
    const RYUKYOKU_DECLARE_SCENARIO: &str = r#"{
        "hand": "158m158p5s123456z",
        "draw": "7z",
        "allow_ryukyoku": true
    }"#;

    // 自摸牌が分からない局面。自摸後手牌を復元できないので向聴数は unknown のままにする。
    const RYUKYOKU_UNKNOWN_HAND_SCENARIO: &str = r#"{
        "hand": "158m158p5s123456z",
        "allow_ryukyoku": true
    }"#;

    // 役牌 P を Pon してテンパイになる局面。
    const PON_REACTION_SCENARIO: &str = include_str!("../scenarios/pon_reaction.json");

    // 234567m 68p 24s E FF の一向聴。FF を Pon して E を切っても一向聴のまま。
    const IISHANTEN_PON_REACTION_SCENARIO: &str = r#"{
        "hand": "234567m68p24s166z",
        "round_wind": "E",
        "player_id": 0,
        "oya": 0,
        "discards": ["", "F", "", ""],
        "history_furiten": { "same_turn": false, "riichi_missed_win": false },
        "legal_dahai": "",
        "legal_pon": [{ "from_player": 1, "tile": "F", "consumed": "F F" }],
        "allow_none": true
    }"#;

    // 既存2副露 + CC 55p E S W の2向聴。C を Pon した後、最良打牌で1向聴になる。
    const TWO_SHANTEN_PON_CALL_SCENARIO: &str = r#"{
        "hand": "55p12377z",
        "round_wind": "E",
        "seat_wind": "E",
        "player_id": 0,
        "oya": 0,
        "discards": ["", "P F C", "", ""],
        "melds": [
            [
                { "kind": "pon", "tiles": "P P P", "called_tile": "P" },
                { "kind": "pon", "tiles": "F F F", "called_tile": "F" }
            ],
            [],
            [],
            []
        ],
        "remaining_tiles": 32,
        "history_furiten": { "same_turn": false, "riichi_missed_win": false },
        "legal_dahai": "",
        "legal_pon": [{ "from_player": 1, "tile": "C", "consumed": "C C" }],
        "allow_none": true
    }"#;

    // 4p は 1p だけ河にある片スジ、7s は 4s でスジ。単独リーチには exact R/T で 4p を選ぶ。
    const HALF_SUJI_SCENARIO: &str = r#"{
        "hand": "444p147m258p123s7s",
        "draw": "9m",
        "player_id": 0,
        "oya": 3,
        "reached": [false, true, false, false],
        "discards": ["", "1p 4s", "", ""],
        "legal_dahai": "4p 7s"
    }"#;

    // player 3 の副露で exact model を使えなくし、字牌 safety の役牌価値 tie-break を通す。
    const HONOR_GUEST_VS_VALUE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 3,
        "reached": [false, true, false, true],
        "discards": ["", "1m 4m 7p", "", ""],
        "melds": [[], [], [], [{"kind": "pon", "tiles": "111m", "called_tile": "1m"}]],
        "legal_dahai": "C N"
    }"#;

    // 合法な打牌が無いまま押し引きが降りを選ぶ局面。防御は評価するが選べる候補が無い。
    const NO_DEFENSE_CANDIDATE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 1,
        "reached": [false, true, false, false],
        "discards": ["", "1m 4m 7p E", "", ""],
        "legal_dahai": "",
        "allow_none": true
    }"#;

    // player 3 が3副露の High。リーチ者はいないので OpenHand 防御が担当する。
    const OPEN_HAND_DEFENSE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 1,
        "discards": ["", "", "", "1p 4s 7p 2m 9p 1s 4s F 5m 1p 7p 2m"],
        "melds": [
            [],
            [],
            [],
            [
                { "kind": "chi", "tiles": "1s 2s 3s", "called_tile": "1s" },
                { "kind": "chi", "tiles": "4s 5s 6s", "called_tile": "4s" },
                { "kind": "pon", "tiles": "F F F", "called_tile": "F" }
            ]
        ],
        "legal_dahai": "1m 9m 1p 9p"
    }"#;

    // 同じ局面に player 1 のリーチを足した複合 threat。
    const COMBINED_THREAT_DEFENSE_SCENARIO: &str = r#"{
        "hand": "19m19p1478s23467z",
        "draw": "4p",
        "round_wind": "E",
        "player_id": 0,
        "oya": 1,
        "reached": [false, true, false, false],
        "discards": ["", "1m 4m 7p E", "", "1p 4s 7p 2m 9p 1s 4s F 5m 1p 7p 2m"],
        "melds": [
            [],
            [],
            [],
            [
                { "kind": "chi", "tiles": "1s 2s 3s", "called_tile": "1s" },
                { "kind": "chi", "tiles": "4s 5s 6s", "called_tile": "4s" },
                { "kind": "pon", "tiles": "F F F", "called_tile": "F" }
            ]
        ],
        "legal_dahai": "1m 9m 1p 9p"
    }"#;

    const CHOICE_LIMIT: usize = 3;

    fn scenario_from_json(json: &str) -> Scenario {
        let spec: ScenarioSpec = serde_json::from_str(json).unwrap();
        Scenario::resolve(&spec).unwrap()
    }

    fn analyzed(json: &str) -> (Scenario, ShantenDecisionDiagnostic, AnalysisResult) {
        let scenario = scenario_from_json(json);
        let diagnostic = ShantenAgent::diagnose(&scenario.context, &scenario.legal_actions);
        let result = AnalysisResult::from_decision(
            &scenario.context,
            &scenario.legal_actions,
            &diagnostic,
            CHOICE_LIMIT,
        );
        (scenario, diagnostic, result)
    }

    fn evaluated_reach(result: &AnalysisResult) -> &AnalysisReachDecision {
        match &result.reach {
            AnalysisReach::Evaluated(decision) => decision,
            other => panic!("リーチを評価している: {other:?}"),
        }
    }

    fn tile_label(action: &LegalAction) -> String {
        match action {
            LegalAction::Dahai { tile } => tile.to_mjai_string(),
            other => panic!("打牌を選んでいる: {other:?}"),
        }
    }

    #[test]
    fn choices_are_the_ranked_choices_of_the_same_diagnostic() {
        let (scenario, diagnostic, result) = analyzed(NORMAL_SCENARIO);
        assert_eq!(
            result.choices,
            rank_choices(
                &scenario.context,
                &scenario.legal_actions,
                &diagnostic,
                CHOICE_LIMIT
            )
        );
        assert_eq!(
            result.choices[0].selected_action,
            diagnostic.selected_action
        );
    }

    #[test]
    fn a_legal_ryukyoku_keeps_its_verdict_and_three_shanten_values() {
        let (_, _, result) = analyzed(RYUKYOKU_DECLARE_SCENARIO);
        assert_eq!(
            result.ryukyoku,
            Some(AnalysisRyukyoku {
                verdict: RyukyokuVerdict::Declare,
                standard_shanten: Some(8),
                chiitoitsu_shanten: Some(6),
                kokushi_shanten: Some(4),
            })
        );
    }

    #[test]
    fn an_unevaluable_hand_keeps_the_shanten_values_unknown() {
        let (_, _, result) = analyzed(RYUKYOKU_UNKNOWN_HAND_SCENARIO);
        let ryukyoku = result.ryukyoku.expect("九種九牌が合法");
        assert_eq!(ryukyoku.verdict, RyukyokuVerdict::Declare);
        assert_eq!(ryukyoku.standard_shanten, None);
        assert_eq!(ryukyoku.chiitoitsu_shanten, None);
        assert_eq!(ryukyoku.kokushi_shanten, None);
    }

    #[test]
    fn a_normal_scenario_has_no_ryukyoku_section() {
        let (_, _, result) = analyzed(NORMAL_SCENARIO);
        assert_eq!(result.ryukyoku, None);
    }

    #[test]
    fn push_pull_keeps_its_mode_and_reason() {
        let (_, diagnostic, result) = analyzed(DEFENSE_SCENARIO);
        let decision = diagnostic
            .push_pull_decision
            .expect("押し引きまで進んでいる");
        let push_pull = result.push_pull.expect("押し引きまで進んでいる");

        assert_eq!(push_pull.mode, decision.mode);
        assert_eq!(push_pull.reason, decision.reason);
        assert_eq!(push_pull.mode, PushPullMode::Fold);
        assert_eq!(
            push_pull.reason,
            PushPullReason::TwoOrMoreShantenAgainstReach
        );
        // 打牌後がテンパイでない局面では攻撃材料を持たない。
        assert_eq!(push_pull.tenpai_offense, None);
    }

    #[test]
    fn a_tenpai_push_keeps_the_offense_facts_behind_it() {
        let (_, _, result) = analyzed(HIGH_VALUE_TENPAI_UNDER_REACH_SCENARIO);
        let push_pull = result.push_pull.expect("押し引きまで進んでいる");
        let offense = push_pull.tenpai_offense.expect("打牌後がテンパイ");

        assert_eq!(push_pull.mode, PushPullMode::Push);
        assert_eq!(push_pull.reason, PushPullReason::StrongTenpaiAgainstReach);
        assert_eq!(offense.live_wait_remaining, 3);
        assert_eq!(offense.live_wait_type_count, 1);
        assert_eq!(offense.permanent_furiten, PermanentFuriten::No);
        assert_eq!(offense.can_ron, Some(true));

        let value = offense.value.expect("攻撃打点を評価している");
        assert_eq!(value.mode, TenpaiOffenseMode::Damaten);
        assert_eq!(value.value.average_total(), Some(7_700));
        assert_eq!(
            offense.strong_tenpai_requirement,
            Some(StrongTenpaiRequirement::WeightedTotal(15_600))
        );
    }

    #[test]
    fn an_unscored_tenpai_keeps_the_live_wait_requirement() {
        let (_, _, result) = analyzed(TENPAI_UNDER_REACH_SCENARIO);
        let offense = result
            .push_pull
            .expect("押し引きまで進んでいる")
            .tenpai_offense
            .expect("打牌後がテンパイ");

        // 打点は評価済みで確定しなかった。評価していない `None` とは別の状態。
        let value = offense.value.expect("攻撃打点を評価している");
        assert_eq!(value.value.average_total(), None);
        assert_eq!(
            offense.strong_tenpai_requirement,
            Some(StrongTenpaiRequirement::LiveWait(6))
        );
    }

    #[test]
    fn a_selected_reach_keeps_its_verdict_and_wait() {
        let (_, diagnostic, result) = analyzed(REACH_SCENARIO);
        let reach = evaluated_reach(&result);

        assert_eq!(diagnostic.selected_action, LegalAction::Reach);
        assert_eq!(reach.verdict, AnalysisReachVerdict::Reach);
        assert_eq!(reach.timing_reason, None);

        let wait = reach.tenpai_wait.expect("打牌後がテンパイ");
        assert_eq!(wait.live_wait_remaining, 8);
        assert_eq!(wait.live_wait_type_count, 2);
        assert_eq!(wait.can_ron, None);
    }

    #[test]
    fn a_high_value_damaten_is_not_a_reach() {
        let (_, diagnostic, result) = analyzed(DAMATEN_HIGH_VALUE_SCENARIO);
        let reach = evaluated_reach(&result);
        let damaten = reach.damaten.as_ref().expect("ダマ打点を評価している");
        let diagnosed = diagnostic.reach.as_ref().expect("リーチを検討している");

        // リーチしない理由は verdict ではなく base reason が持つ。
        assert_eq!(reach.verdict, AnalysisReachVerdict::NoReach);
        assert_eq!(reach.base_reason, diagnosed.reason);
        assert_eq!(reach.base_reason, ReachDecisionReason::HighValueDamaten);
        assert_eq!(damaten.verdict, DamatenValueVerdict::AboveThreshold);
        assert!(!damaten.winning_tiles.is_empty());
        assert!(
            damaten
                .winning_tiles
                .iter()
                .all(|winning_tile| matches!(winning_tile.value, DamatenValue::Known { .. }))
        );
    }

    #[test]
    fn a_hand_without_a_legal_reach_is_no_reach() {
        // リーチが合法でない局面もリーチしない結果になるが、ダマを選んだわけではない。
        let (_, diagnostic, result) = analyzed(NORMAL_SCENARIO);
        let reach = evaluated_reach(&result);
        let diagnosed = diagnostic.reach.as_ref().expect("リーチを検討している");

        assert_eq!(reach.verdict, AnalysisReachVerdict::NoReach);
        assert_eq!(reach.base_reason, diagnosed.reason);
        assert_eq!(reach.base_reason, ReachDecisionReason::NoLegalReach);
        assert_eq!(reach.damaten, None);
    }

    #[test]
    fn the_verdict_does_not_name_the_reason_for_not_reaching() {
        // リーチしない理由が何であっても verdict は NoReach で、理由は base reason だけが持つ。
        for reason in [
            ReachDecisionReason::NoLegalReach,
            ReachDecisionReason::NoSelectedDiscard,
            ReachDecisionReason::NotTenpai,
            ReachDecisionReason::NoLiveWait,
            ReachDecisionReason::InsufficientLiveWait,
            ReachDecisionReason::HighValueDamaten,
            ReachDecisionReason::NamedYakumanDamaten,
        ] {
            let projected = reach_decision(&not_reached_diagnostic(reason));

            assert_eq!(
                projected.verdict,
                AnalysisReachVerdict::NoReach,
                "{reason:?}"
            );
            assert_eq!(projected.base_reason, reason);
            assert_eq!(projected.timing_reason, None);
        }
    }

    #[test]
    fn a_low_value_damaten_keeps_its_winning_tile_values() {
        let (_, diagnostic, result) = analyzed(DAMATEN_LOW_VALUE_SCENARIO);
        let reach = evaluated_reach(&result);
        let damaten = reach.damaten.as_ref().expect("ダマ打点を評価している");
        let expected: Vec<_> = diagnostic
            .reach
            .as_ref()
            .and_then(|reach| reach.damaten_value.as_ref())
            .expect("ダマ打点を評価している")
            .winning_tile_values()
            .map(|winning_tile| (winning_tile.winning_tile, winning_tile.value))
            .collect();

        assert_eq!(reach.verdict, AnalysisReachVerdict::Reach);
        assert_eq!(damaten.verdict, DamatenValueVerdict::BelowThreshold);
        assert_eq!(
            damaten
                .winning_tiles
                .iter()
                .map(|winning_tile| (winning_tile.winning_tile, winning_tile.value))
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn a_fold_marks_the_reach_as_not_evaluated() {
        let (_, diagnostic, result) = analyzed(DEFENSE_SCENARIO);
        assert!(diagnostic.reach.is_none());
        assert_eq!(result.reach, AnalysisReach::NotEvaluated);
    }

    #[test]
    fn a_decision_before_push_pull_has_no_reach_section() {
        let (_, diagnostic, result) = analyzed(PON_REACTION_SCENARIO);
        assert!(diagnostic.push_pull_decision.is_none());
        assert_eq!(result.reach, AnalysisReach::Absent);
    }

    #[test]
    fn a_selected_call_keeps_its_action_and_reason() {
        let (_, diagnostic, result) = analyzed(PON_REACTION_SCENARIO);
        let call = result.call.expect("鳴きを検討している");

        assert_eq!(call.selected, Some(diagnostic.selected_action.clone()));
        assert!(matches!(call.selected, Some(LegalAction::Pon { .. })));
        assert_eq!(call.reason, CallDecisionReason::EligibleTenpai);
        assert_eq!(
            call.reason_source,
            AnalysisCallReasonSource::SelectedCandidate
        );
        // 即テンパイの候補は self-tsumo 比較を持たないので比較対象も出さない。
        assert_eq!(call.compared_candidate, None);
    }

    #[test]
    fn a_rejected_call_keeps_the_first_candidate_reason_and_its_comparison() {
        let (_, _, result) = analyzed(IISHANTEN_PON_REACTION_SCENARIO);
        let call = result.call.expect("鳴きを検討している");
        let candidate = call
            .compared_candidate
            .as_ref()
            .expect("self-tsumo 比較を持つ候補");

        assert_eq!(call.selected, None);
        assert_eq!(call.reason_source, AnalysisCallReasonSource::FirstCandidate);
        assert!(candidate.is_reason_source);
        assert!(matches!(
            candidate.self_tsumo,
            AnalysisCallSelfTsumo::Iishanten(_)
        ));
        assert_eq!(
            candidate.post_call_discard,
            Some(AnalysisDiscardTile {
                tile: TileType::from_mjai_type_str("E").unwrap(),
                red_five: false,
            })
        );
    }

    #[test]
    fn a_two_shanten_call_keeps_its_pass_evaluation() {
        let (_, _, result) = analyzed(TWO_SHANTEN_PON_CALL_SCENARIO);
        let call = result.call.expect("鳴きを検討している");
        let candidate = call.compared_candidate.expect("採用した候補");

        assert!(matches!(call.selected, Some(LegalAction::Pon { .. })));
        assert_eq!(
            call.reason_source,
            AnalysisCallReasonSource::SelectedCandidate
        );
        assert!(candidate.is_reason_source);
        let AnalysisCallSelfTsumo::TwoShanten {
            pass_evaluation,
            comparison,
        } = candidate.self_tsumo
        else {
            panic!("2向聴の比較: {:?}", candidate.self_tsumo);
        };
        assert_eq!(pass_evaluation, CallTwoShantenPassEvaluation::Full);
        assert_eq!(comparison.verdict, CallIishantenComparison::CallHigher);
        assert!(comparison.pass_expected_self_tsumo_value.is_some());
        assert!(comparison.call_expected_self_tsumo_value.is_some());
    }

    #[test]
    fn the_compared_call_candidate_can_differ_from_the_reason_source() {
        // 採用が無く、最初の候補が self-tsumo 比較を持たない局面。比較は次の候補から取り、
        // 理由の由来は最初の候補のままにする。
        let post_call_discard = best_discard(NORMAL_SCENARIO);
        let first =
            call_candidate_diagnostic(0, CallDecisionReason::PostCallNotIishanten, None, None);
        let second = CallCandidateDiagnostic {
            post_call_discard: Some(post_call_discard.clone()),
            ..call_candidate_diagnostic(
                16,
                CallDecisionReason::PassSelfTsumoNotLower,
                Some(CallTwoShantenSelfTsumoDiagnostic {
                    reaction_source_player: Some(2),
                    pass_evaluation: CallTwoShantenPassEvaluation::Full,
                    pass_expected_self_tsumo_value: Some(378_060_000),
                    call_expected_self_tsumo_value: Some(41_069_000),
                    comparison: CallIishantenComparison::PassNotLower,
                    speed: CallTwoShantenSpeedDiagnostic {
                        own_future_draws: Some(8),
                        han: None,
                        overrides_pass: false,
                    },
                }),
                None,
            )
        };
        let diagnostic = CallDecisionDiagnostic {
            selected: None,
            reason: first.reason,
            candidates: vec![first, second.clone()],
        };

        let projected = call(&diagnostic);
        let candidate = projected.compared_candidate.expect("比較を持つ候補");

        assert_eq!(projected.selected, None);
        assert_eq!(
            projected.reason_source,
            AnalysisCallReasonSource::FirstCandidate
        );
        assert_eq!(candidate.action, second.action);
        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert!(!candidate.is_reason_source);
        assert!(matches!(
            candidate.self_tsumo,
            AnalysisCallSelfTsumo::TwoShanten { .. }
        ));
        // 比較対象の候補の値は、同じ候補の打牌と組で出す。
        assert_eq!(
            candidate.post_call_discard,
            Some(AnalysisDiscardTile {
                tile: post_call_discard.discard,
                red_five: post_call_discard.discards_red_five,
            })
        );
    }

    #[test]
    fn a_reach_threat_defense_keeps_its_action_and_kind() {
        let (_, diagnostic, result) = analyzed(HALF_SUJI_SCENARIO);
        let Some(AnalysisDefense::ReachThreat(defense)) = result.defense else {
            panic!("リーチ者向け防御を採用している: {:?}", result.defense);
        };

        assert_eq!(defense.action, diagnostic.selected_action);
        assert_eq!(tile_label(&defense.action), "4p");
        assert_eq!(defense.kind, DefenseFallbackKind::ExactRonRisk);
        // HonorSafety 以外では役牌価値を持たない。
        assert_eq!(defense.opponent_honor_value, None);
    }

    #[test]
    fn an_honor_safety_defense_keeps_the_opponent_honor_value() {
        let (_, _, result) = analyzed(HONOR_GUEST_VS_VALUE_SCENARIO);
        let Some(AnalysisDefense::ReachThreat(defense)) = result.defense else {
            panic!("リーチ者向け防御を採用している: {:?}", result.defense);
        };

        assert_eq!(tile_label(&defense.action), "N");
        assert!(matches!(defense.kind, DefenseFallbackKind::HonorSafety(_)));
        assert_eq!(
            defense.opponent_honor_value,
            Some(AnalysisOpponentHonorValue::Known(
                OpponentHonorValue::GuestWind
            ))
        );
    }

    #[test]
    fn an_evaluated_defense_without_a_selection_keeps_that_state() {
        let (_, diagnostic, result) = analyzed(NO_DEFENSE_CANDIDATE_SCENARIO);
        assert!(
            diagnostic
                .defense
                .as_ref()
                .is_some_and(|defense| defense.selected.is_none())
        );
        assert_eq!(
            result.defense,
            Some(AnalysisDefense::ReachThreatWithoutSelection)
        );
    }

    #[test]
    fn an_open_hand_defense_keeps_its_category() {
        let (_, diagnostic, result) = analyzed(OPEN_HAND_DEFENSE_SCENARIO);
        let Some(AnalysisDefense::OpenHand { action, category }) = result.defense else {
            panic!("OpenHand 防御を採用している: {:?}", result.defense);
        };

        assert_eq!(action, diagnostic.selected_action);
        assert_eq!(category, OpenHandDefenseCategory::SafeAgainstAllTargets);
    }

    #[test]
    fn a_combined_threat_defense_keeps_its_category() {
        let (_, diagnostic, result) = analyzed(COMBINED_THREAT_DEFENSE_SCENARIO);
        let Some(AnalysisDefense::CombinedThreat { action, category }) = result.defense else {
            panic!("複合 threat 防御を採用している: {:?}", result.defense);
        };

        assert_eq!(action, diagnostic.selected_action);
        assert!(matches!(category, CombinedDefenseCategory::SuitedSafety(_)));
    }

    #[test]
    fn a_normal_discard_has_no_defense_section() {
        let (_, _, result) = analyzed(NORMAL_SCENARIO);
        assert_eq!(result.defense, None);
    }

    fn not_reached_diagnostic(reason: ReachDecisionReason) -> ReachDecisionDiagnostic {
        ReachDecisionDiagnostic {
            selected_discard: None,
            shanten_after_discard: None,
            tenpai_wait: None,
            damaten_value: None,
            selected: None,
            reason,
            timing: None,
        }
    }

    fn best_discard(json: &str) -> DiscardEvaluation {
        let scenario = scenario_from_json(json);
        let counts = TileCounts::from_tiles(
            scenario
                .context
                .hand_tiles()
                .iter()
                .copied()
                .chain(scenario.context.drawn_tile()),
        );
        select_best_discard(&counts).expect("best discard")
    }

    fn call_candidate_diagnostic(
        tile: u8,
        reason: CallDecisionReason,
        two_shanten_self_tsumo: Option<CallTwoShantenSelfTsumoDiagnostic>,
        post_call_discard: Option<DiscardEvaluation>,
    ) -> CallCandidateDiagnostic {
        CallCandidateDiagnostic {
            action: LegalAction::Pon {
                tile: TileId::new(tile).unwrap(),
                consumed: vec![
                    TileId::new(tile + 1).unwrap(),
                    TileId::new(tile + 2).unwrap(),
                ],
            },
            kind: CallKind::Pon,
            current_fixed_meld_count: None,
            current_shanten: None,
            post_call_fixed_meld_count: None,
            post_call_forbidden_discards: None,
            post_call_discard,
            post_call_wait: None,
            post_call_wait_yaku: None,
            post_call_push_pull: None,
            iishanten_acceptance: None,
            iishanten_self_tsumo: None,
            two_shanten_self_tsumo,
            three_shanten_self_tsumo: None,
            eligible: false,
            selected: false,
            reason,
        }
    }
}
