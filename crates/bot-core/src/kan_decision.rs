//! カン判断 policy 層。Chi / Pon の鳴き判断 ([`crate::call_decision`]) とは別の責務として持つ。
//!
//! Chi / Pon は他家打牌への reaction で「鳴く → 直後に打牌する」を1つの評価単位にできるが、
//! カンはそうならない。
//!
//! | 種別 | 契機 | 直後の手番 |
//! | --- | --- | --- |
//! | Ankan | 自分のツモ番 | 嶺上牌を引いてから打牌 |
//! | Kakan | 自分のツモ番 | 嶺上牌を引いてから打牌 (搶槓あり) |
//! | Daiminkan | 他家打牌への reaction | 嶺上牌を引いてから打牌 |
//!
//! どの種別も「カン → 未知の嶺上牌 → 打牌」なので、Chi / Pon の
//! `Call → post-call discard` 評価モデルをそのまま当てはめられない。そのため鳴き判断へ混ぜず、
//! この層を分ける。
//!
//! # 今回 production へ接続する範囲
//!
//! production で選べるのは [`KanKind::Ankan`] と [`KanKind::Kakan`] である。
//! [`KanKind::Daiminkan`] は候補として診断には並ぶが、理由
//! ([`KanDecisionReason::DaiminkanNotConnected`]) を付けて必ず選ばない。
//!
//! 加槓は暗槓と違って搶槓される可能性があるので、暗槓の成立条件に加えて「加槓牌で搶槓ロン
//! されない」ことを hard fact で確定できる局面だけへ限定する。詳細は
//! 「加槓 (Kakan) v1」の節に置く。
//!
//! # 自己リーチ前と自己リーチ後で policy を分ける
//!
//! 暗槓の判断は、自分が既にリーチしているかどうかで**別の policy** になる。分かれ目は
//! [`GameContext::own_reached`] だけで、`reached` の index を推測しない。
//!
//! | `own_reached()` | policy |
//! | --- | --- |
//! | `Some(false)` | 暗槓前後を既存評価で比較し、悪化しないと確認できた場合だけ暗槓する |
//! | `Some(true)` | 合法な暗槓を原則そのまま採用する暫定 policy |
//! | `None` | 自席を特定できない。リーチ済みだともしていないとも推測せず [`KanDecisionReason::OwnReachUnknown`] |
//!
//! 分ける理由は、自己リーチ後には**比較対象が違う**ためである。リーチ後は合法な打牌が現在の
//! ツモ牌1枚に限られるので、暗槓しない場合の選択肢は自由な通常打牌ではなく強制ツモ切りになる。
//! したがって暗槓前の「通常打牌をどう選ぶか」という比較そのものが成り立たない。
//!
//! この分岐は暗槓だけのものである。加槓は元になる Pon があるので通常は自己リーチと両立しない
//! が、server / context が矛盾した値を持っても推測で処理せず、`Some(true)` は
//! [`KanDecisionReason::KakanAfterOwnReach`]、`None` は [`KanDecisionReason::OwnReachUnknown`]
//! として加槓しない。server が加槓を提示していることだけを理由に自己リーチ状態を `false` と
//! 推測しない。
//!
//! # source of truth
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | カンの合法性 | 入力の `legal_actions` (`possible_actions` 由来) |
//! | 自分がリーチ済みか | [`GameContext::own_reached`] |
//! | 面子の形の検証 | [`Meld::shape`] |
//! | 加槓が置換する既存 Pon | 自分の副露 ([`GameContext::own_melds`]) の [`Meld::shape`] |
//! | 加槓牌で搶槓ロンされないこと | [`is_discarded_by_player`] と [`CompressedStructuralTenpaiHiddenHandStates`] |
//! | 副露済み面子数 | [`GameContext::own_fixed_meld_count`] / [`FixedMeldCount`] |
//! | カン後の向聴数 | [`calculate_shanten_with_fixed_melds`] |
//! | カン後の受け入れ | [`calculate_acceptance_with_fixed_melds`] / [`calculate_acceptance_with_fixed_melds_and_visible_tiles`] |
//! | カン後テンパイの待ちとロン可否 | [`tenpai_wait_availability`] |
//! | カン後テンパイの完成手 | [`tenpai_completed_hands`] |
//! | カン後のリーチ合法性 | [`future_reach_legal`] (= 共有条件 [`is_reach_legal`](crate::reach_policy::is_reach_legal)) |
//! | テンパイの攻撃打点と攻撃モード | [`evaluate_tenpai_offense_value`] / [`evaluate_tenpai_offense_with_reach_legality`] |
//! | 暗槓しない場合の打牌 | production の通常打牌選択が選んだ [`DiscardEvaluation`] |
//! | 押し引き | [`decide_push_pull`](crate::push_pull::decide_push_pull) の結論 |
//!
//! 向聴・受け入れ・待ち・フリテン・役・点数をこの層で計算し直さない。打点は押し引きとリーチ
//! 判断が使うのと同じ [`TenpaiOffenseValue`] をそのまま読む。
//!
//! リーチ後に暗槓できるか (待ちが変わらないか) も含めて、合法性の判定はこの層に持たない。
//! `legal_actions` に [`LegalAction::Ankan`] が並んでいることだけを合法の根拠にする。
//!
//! # 判断する位置
//!
//! `ShantenAgent` は
//!
//! ```text
//! Hora → 九種九牌 → Chi / Pon → 押し引き → (Push なら Reach) → Kan → 通常打牌 / 防御 fallback
//! ```
//!
//! の順で action を決める。和了できる局面では Hora が先に決まるので、カンが和了より先に選ばれる
//! ことはない。Push mode ではリーチを採用しなかった後にだけカンを検討するので、既存の Reach
//! 優先順位も変わらない。
//!
//! カン判断そのものは押し引きの結論にかかわらず通る。自己リーチ後は降りようがないので、
//! 押し引きが Fold と判断した局面でもカンを検討する必要があるためである。押し引きを見るのは
//! 自己リーチ前の policy だけで、その gate はこの層が持つ ([`KanDecisionReason::NotPush`])。
//!
//! # 自己リーチ後の暫定 policy
//!
//! ```text
//! own_reached() == Some(true)
//! AND legal_actions に Ankan がある
//! AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
//! AND 自分の副露済み面子数が分かり、暗槓後も上限内
//! → Ankan ([`KanDecisionReason::EligibleAnkanAfterOwnReach`])
//! ```
//!
//! 向聴・受け入れ・攻撃打点・押し引き・他家リーチはどれも採用条件にしない。リーチ後は暗槓
//! しなければ現在のツモ牌を強制ツモ切りするしかないので、比較は
//!
//! ```text
//! 暗槓 vs 現在のツモ牌の強制ツモ切り
//! ```
//!
//! になる。どちらにも既存評価では同じ尺度に載らない損得があり、
//!
//! | 暗槓する側のリスク | 暗槓しない側のリスク |
//! | --- | --- |
//! | 新しい槓ドラで他家の打点が上がる | 強制ツモ切りした牌で放銃する |
//! | 山とツモ順が変わる | 嶺上牌という追加のツモ機会を失う |
//! | | 槓ドラ・槓裏による自分の打点上昇機会を失う |
//!
//! を1つの EV として比べる基盤が今の nodocchi には無い。中途半端な係数や heuristic を置かない
//! ため、今回は「server が合法とした暗槓は原則行う」という暫定 policy にする。
//!
//! TODO: 自己リーチ後も「暗槓 vs 現在のツモ牌をそのまま切る」を比較できるようにする。少なくとも
//! 次を同じ尺度で評価できるようになった時点で、この暫定 policy を見直す。
//!
//! - 強制ツモ切り牌の ron risk
//! - 新しい槓ドラによる opponent threat / 打点変化
//! - 嶺上牌という追加のツモ機会
//! - 自分の槓ドラ / 槓裏による打点変化
//! - 点棒 / 順位状況
//! - 終盤・流局条件
//!
//! # 自己リーチ前の暗槓の成立条件
//!
//! ```text
//! own_reached() == Some(false)
//! AND 合法な Ankan がある
//! AND 他家にリーチ者がいない
//! AND 既存 Push/Pull policy が Push と判定している
//! AND 自分のツモを経たと確認できる (drawn_tile がある)
//! AND 自分の副露済み面子数が分かり、暗槓後も上限内
//! AND consumed 4枚を手牌 + ツモ牌から取り除いてカンの形になる
//! AND 暗槓しない場合に選ぶ通常打牌の評価がある
//! AND 暗槓後の向聴数 == その通常打牌後の向聴数
//! AND 暗槓後の受け入れ残枚数 >= その通常打牌後の受け入れ残枚数
//! AND 暗槓後の受け入れ牌種数 >= その通常打牌後の受け入れ牌種数
//! AND 両側の攻撃打点を既存評価で確定でき、攻撃モードも一致する
//! AND 暗槓後の攻撃打点 >= その通常打牌後の攻撃打点
//! AND 成立した暗槓候補がちょうど1件
//! ```
//!
//! ## 比較する2つの state
//!
//! 暗槓する場合としない場合を、どちらも「13枚相当で次のツモを待つ state」に揃えて比べる。
//!
//! ```text
//! 暗槓しない: 14枚 → 通常打牌 → 13枚 (副露 N)        → 次のツモを待つ
//! 暗槓する  : 14枚 → 暗槓     → 10枚 (副露 N+1)      → 嶺上牌を待つ
//! ```
//!
//! `10枚 + 副露 N+1` と `13枚 + 副露 N` はどちらも `13 - 3 * 副露数` 枚の同じ大きさの手牌なので、
//! 既存の向聴・受け入れ・打点をそのまま同じ尺度で比べられる。比較の基準に使う打牌は production
//! の通常打牌選択が実際に選んだ1件そのもので、この層で打牌を選び直さない。
//!
//! ## 向聴の比較
//!
//! [`Acceptance`](bot_logic::Acceptance) は「その牌を1枚加えると**現在の向聴数**が下がる牌」な
//! ので、向聴段階が違う state の受け入れ枚数・牌種数は同じ意味の値ではない。1向聴の受け入れ8枚
//! とテンパイの待ち4枚を `4 < 8` として比べない。したがって向聴の比較は3通りに分ける。
//!
//! | 暗槓後 vs 通常打牌後 | 扱い |
//! | --- | --- |
//! | 悪化 | [`KanDecisionReason::ShantenRegresses`] |
//! | 改善 | 受け入れも打点も同じ尺度で比べられないので [`KanDecisionReason::ShantenImprovedNotComparable`] |
//! | 同じ | 受け入れと打点の比較へ進む |
//!
//! 向聴が改善するのは、暗槓しない側の合法打牌が制限されていて4枚目を切れない局面などに限られる。
//! 改善そのものを暗槓の根拠にはしない。向聴が進んだ分の価値を既存 primitive で確定できないため、
//! 「向聴が改善したから暗槓する」という結論もここでは作らない。
//!
//! ## 打点の比較
//!
//! 速度が悪化しないことだけでは、暗槓によって役・待ち構成・確定打点が落ちる局面を弾けない。
//! そのため速度の比較を通った候補には、押し引き・リーチ判断が使うのと同じ攻撃打点
//! ([`TenpaiOffenseValue`]) の比較を必ず要求する。
//!
//! | 側 | 手牌 | 打点 |
//! | --- | --- | --- |
//! | 暗槓しない | 通常打牌後13枚 + 既存副露 | [`evaluate_tenpai_offense_value`] |
//! | 暗槓する | 暗槓後10枚 + 既存副露 + 今回の暗槓 | [`evaluate_tenpai_offense_with_reach_legality`] |
//!
//! どちらも同じ hypothetical baseline (リーチ手なら [`current_reach_baseline_context`]、ダマ手
//! なら [`damaten_baseline_context`]) と同じ既知のドラ表示牌で評価し、生きた和了牌 variant の
//! 残枚数で加重した合計 ([`OffenseValue::weighted_total`](crate::offense_value::OffenseValue::weighted_total))
//! を比べる。押し引きが threshold 判定に使うのと同じ値で、カン専用の打点評価も集約規則も
//! 持たない。
//!
//! 攻撃モード (リーチ手かダマ手か) が違う2つの値は同じ尺度ではないので、モードが一致しない
//! 場合は比較しない。暗槓後のモードを決める「リーチが合法か」は、現在局面の `legal_actions` を
//! そのまま流用せず、共有条件 ([`is_reach_legal`](crate::reach_policy::is_reach_legal)) を
//! 暗槓後の手牌の事実へ適用した [`future_reach_legal`] で求める。
//!
//! ### 暗槓後の山の残りツモ可能枚数
//!
//! リーチ宣言には `remaining_tiles >= REACH_MIN_REMAINING_TILES` が要る。暗槓後は嶺上牌を1枚
//! 引くので、その時点でツモできる枚数は現在より1枚少ない。2手先評価の枝のように「何巡先か」が
//! 確定しない未来とは違い、暗槓後は現在の枚数さえ分かればこの1枚分を既知 fact として導ける。
//!
//! | 現在の `remaining_tiles` | 暗槓後 | remaining tiles 条件 |
//! | --- | --- | --- |
//! | `Some(5)` | `Some(4)` | 満たす |
//! | `Some(4)` | `Some(3)` | 満たさない (暗槓後はリーチできない) |
//! | `Some(0)` | `Some(0)` | 満たさない。unknown へ倒してリーチ可能側にしない |
//! | `None` | `None` | 推測しない。共有条件の unknown 規則へ委ねる |
//!
//! 導出は [`post_kan_remaining_tiles`] だけが持ち、リーチ合法性の条件そのものは
//! [`is_reach_legal`](crate::reach_policy::is_reach_legal) のままにする。
//!
//! ## 評価不能として暗槓しない局面
//!
//! 次のどれかに当たる候補は [`KanDecisionReason::ValueNotEvaluable`] にして暗槓せず、通常打牌を
//! そのまま維持する。速度非劣化だけを根拠に暗槓へ倒すことはしない。
//!
//! - どちらかの side がテンパイでない (テンパイ以外の打点を既存 primitive で比較できない)
//! - どちらかの side の攻撃モードが [`TenpaiOffenseMode::Unknown`]、またはモードが食い違う
//! - どちらかの side の攻撃打点が
//!   [`OffenseValue::Unknown`](crate::offense_value::OffenseValue::Unknown) (役なし・ロン不可・
//!   点数計算の入力不足・裏ドラ未確定)
//!
//! テンパイ以外を対象外にするのは、1向聴以降の価値尺度が
//! [`ExpectedSelfTsumoValue`](bot_logic::ForwardMetrics) 系になるためである。暗槓後の state は
//! 嶺上牌ぶん1回多くツモれて、しかも残り自摸機会 ([`own_future_draws`]) の元になる山の残枚数も
//! 変わるので、暗槓しない側と同じ horizon の値にならない。差を埋める補正を推測で置かない限り
//! 比較にならないため、今回は接続しない。テンパイの攻撃打点はロン和了1回分の確定打点で、
//! 残り自摸機会に依存しないので、この非対称性を持たない。
//!
//! ## 今回評価に含めないもの
//!
//! 暗槓には既存評価だけでは値を確定できない要素があり、係数や推定値を置かずに「評価に含め
//! ない」ままにする。含めていないものは次のとおりで、いずれも TODO として残す。
//!
//! - **新ドラ**: カンで増えるドラ表示牌の中身は未知なので、自分の打点にも他家の打点にも
//!   加算しない。他家リーチ中に暗槓しない ([`KanDecisionReason::OpponentReached`]) のは、
//!   この未知のドラがリーチ者の打点をどれだけ押し上げるかを既存評価で測れないためである。
//!   暗槓側の打点を過小評価する方向なので、比較は暗槓に不利な側へ倒れる。
//! - **嶺上牌**: 引く牌は未知なので、特定の牌を引いた後の state として評価しない。暗槓が1回
//!   分多くツモれることも評価へ足さない。
//!
//! 暗刻が暗槓になることで増える符は、既存 scoring が暗槓を含む固定面子から求めた値がそのまま
//! 打点比較へ入る。この層で符を数え直さない。
//!
//! # 加槓 (Kakan) v1
//!
//! 加槓は暗槓と同じ自摸番の action だが、追加する4枚目を他家に**搶槓**される可能性がある点が
//! 決定的に違う。v1 では搶槓リスクを推定せず、搶槓ロンが起こり得ないと hard fact で確定できる
//! 局面だけへ限定する。
//!
//! ## 加槓の成立条件
//!
//! ```text
//! legal_actions に Kakan がある
//! AND own_reached() == Some(false)
//! AND 自分のツモを経たと確認できる (drawn_tile がある)
//! AND 他家にリーチ者がいない
//! AND 既存 Push/Pull policy が Push と判定している
//! AND 加槓の形が成り立ち、対応する既存 Pon を特定できる
//! AND Pon → Kakan の post-state を組み立てられる
//! AND 全他家について、自身の河に加槓牌があるか structural completion が0であることを確定できる
//! AND 加槓しない場合に選ぶ通常打牌の評価がある
//! AND 加槓後の向聴数 == その通常打牌後の向聴数
//! AND 加槓後の受け入れ (残枚数・牌種数) が悪化しない
//! AND 両側の攻撃打点を既存評価で確定でき、攻撃モードも一致する
//! AND 加槓後の攻撃打点 >= その通常打牌後の攻撃打点
//! AND 成立したカン候補がちょうど1件
//! ```
//!
//! 1つでも満たせない場合は加槓しない。
//!
//! ## Pon → Kakan の post-state
//!
//! 加槓は固定面子の**追加**ではなく**置換**である。
//!
//! ```text
//! 既存 Pon (3枚) + 追加牌1枚 → 同じ位置の Kakan (4枚)
//! ```
//!
//! | 項目 | 加槓後 |
//! | --- | --- |
//! | 副露済み面子数 | 前後で変わらない |
//! | 元の Pon | 副露 list から消える |
//! | Kakan | 元の Pon と同じ位置を置き換える |
//! | Kakan の tiles | 元 Pon の3枚 + 追加牌1枚 |
//! | Kakan の `called_tile` | 元 Pon の `called_tile` をそのまま保持する |
//! | concealed hand | 手牌 + ツモ牌から追加牌1枚だけを取り除く |
//!
//! [`LegalAction::Kakan`] の `tile` は追加する4枚目であって、Kakan 面子の `called_tile` では
//! ない。`called_tile` は元の Pon で鳴いた牌のままで、符計算や公開情報の扱いが変わらないように
//! 置換後も保持する。
//!
//! 比較する2つの state は暗槓と同じく「13枚相当で次のツモを待つ state」へ揃う。
//!
//! ```text
//! 加槓しない: 14枚 → 通常打牌 → 13枚 (副露 N) → 次のツモを待つ
//! 加槓する  : 14枚 → 加槓     → 13枚 (副露 N) → 嶺上牌を待つ
//! ```
//!
//! 加槓は副露数を変えないので、concealed hand の枚数も `13 - 3 * 副露数` のまま変わらない。
//!
//! ## 物理牌 (TileId) の扱い
//!
//! RiichiLab の mjai → [`TileId`] 変換は黒牌の物理 copy ID を復元できず、同じ牌種の黒牌は
//! すべて同じ代表 ID へ潰れる (`temporary_tile_id_from_mjai_pai`)。したがって `consumed` の
//! [`TileId`] が既存 Pon の物理牌と完全一致することを validation 条件にしない。確かめるのは
//! 牌種 semantics だけにする。
//!
//! ```text
//! consumed.len() == 3
//! consumed 3枚が同一 TileType
//! 追加牌も同一 TileType
//! 対応する既存 Pon が同一 TileType の Pon
//! 追加牌が現在の concealed hand + ツモ牌に存在する
//! ```
//!
//! 一方、追加牌を実際に手牌から取り除くときは赤5と黒5を区別する。牌種だけで一致させると赤5を
//! 誤って槓へ持っていき、手牌に残る赤ドラを取り違えるためである。取り除く牌は牌種と赤牌かどうか
//! の両方が一致する1枚に限る。
//!
//! ## 搶槓 hard-safe
//!
//! v1 でもっとも重要な条件である。加槓牌は他家から搶槓される可能性があり、しかも通常の打牌と
//! 危険度が違う。既存の hidden-hand model が持つ通常ロン評価は `chankan = false` を前提にした
//! 箇所があるので、
//!
//! ```text
//! 通常の打牌では役なしでロンできない
//! 加槓では搶槓 (Chankan) が役として付いてロンできる
//! ```
//!
//! という手を取りこぼす。そのため通常打牌用の exact ron-risk をそのまま流用しない。
//!
//! v1 では搶槓 risk を**推定しない**。他家リーチ中は加槓しないので残る3家は非リーチであり、
//! その全員について次のどちらかを確定できた場合だけ加槓する。
//!
//! ```text
//! A. その player 自身の河に加槓牌がある
//!    → 恒常フリテンでロンできない
//!    → hard-safe ([`KakanChankanSafety::RiverFuriten`])
//!
//! OR
//!
//! B. structural hidden-hand model で
//!    target_completion_state_weight(加槓牌) == 0
//!    → 公開情報と整合する hidden hand の中に、その牌で Standard の和了形を完成できる
//!      state 自体が存在しない
//!    → hard-safe ([`KakanChankanSafety::NoStructuralCompletion`])
//! ```
//!
//! 1人でもどちらも確定できなければ [`KanDecisionReason::KakanChankanNotHardSafe`] で加槓しない。
//!
//! ### structural completion を使う理由
//!
//! [`CompressedStructuralTenpaiHiddenHandStates::target_completion_state_weight`] は、通常ロンの
//! 「役があるか」を判定する `R` ではなく、**対象牌を加えたときに Standard の構造的和了形が完成
//! する hidden state の重み**を exact に数える。したがって
//!
//! ```text
//! target_completion_state_weight(加槓牌) == 0
//! ```
//!
//! なら、通常ロンで役があるかにも、搶槓で Chankan が役として追加されるかにも関係なく、そもそも
//! その牌で和了形になれないため搶槓ロン不能と確定できる。heuristic ではなく hard fact として
//! 使える。
//!
//! | model の結果 | v1 の扱い |
//! | --- | --- |
//! | `completion == 0` | hard-safe |
//! | `completion > 0` | 搶槓されるとは断定しないが hard-safe とも断定できないので reject |
//! | model unavailable / unsupported | unknown として reject |
//!
//! `completion > 0` を確率へ変換したり threshold を置いたりしない。
//!
//! 判定順は A が先で、自身の河に加槓牌がある player には exact counting を行わない
//! ([`is_discarded_by_player`] が source of truth)。A で確定しない player についてだけ
//! [`CompressedStructuralTenpaiHiddenHandStates::new`] を試し、その成功・失敗をそのまま model の
//! 対応範囲とする。「副露があるように見えるから使えるはず」といった条件をこの層で複製しない。
//!
//! ### hard-safe の根拠に使わないもの
//!
//! | 使わない evidence | 理由 |
//! | --- | --- |
//! | `temporary_passed_tiles` | 「一時フリテンで今はロンできない」だけで、搶槓で新しく役が付く手を排除できない |
//! | `same_hand_passed_tiles` | 手牌不変の見逃し観測であって hard fact ではない |
//! | Suji | 河由来の推測で、ロン不能を確定しない |
//! | Wall / OneChance | 見え枚数由来の推測で、ロン不能を確定しない |
//! | Honor safety rank | 同上 |
//! | 通常 Dahai 用 exact `R/T` (`ron_risk_evidence` / `ron_capable_state_weight`) | 役判定を含み、`chankan = false` 前提の経路がある |
//! | 「Push だから大丈夫」 | 押し引きの結論は放銃可否の事実ではない |
//!
//! 特に `temporary_passed_tiles` は通常ロンに対する安全 evidence としては有効でも、搶槓に
//! よって役が新しく付くケースを排除できないため、加槓の safety へ流用しない。
//!
//! ## 他家リーチ中
//!
//! ```text
//! ctx.any_opponent_reached() == true
//! → 加槓しない ([`KanDecisionReason::OpponentReached`])
//! ```
//!
//! 加槓には一発を消す利点があるが、
//!
//! - 新しい槓ドラによる相手の打点上昇
//! - 一発消去
//! - 搶槓 risk
//! - 嶺上牌
//!
//! を同じ尺度で比較できる基盤がまだ無い。今回はその比較モデルを作らない。
//!
//! ## 加槓後の攻撃打点
//!
//! 加槓後も手は開いたままである。元が Pon なので門前には戻らず、誤って門前手やリーチ手として
//! 評価しない。門前かどうかは置換後の副露 list から既存 [`is_menzen`] で求める。新しい槓ドラの
//! 中身は未知なので、加槓後の攻撃打点へ加算しない。嶺上牌も暗槓と同じく評価へ含めない。
//!
//! ## TODO: 搶槓 exact model
//!
//! TODO: [`WinningContext`](bot_logic::WinningContext) の `chankan: true` を使った opponent
//! hidden-hand model を整備し、リーチ者・公開副露者・門前非リーチ者のすべてについて搶槓の
//! `R/T` を評価できるようにする。その時点で「structural completion が1つでもあれば加槓しない」
//! 「model を使えない player がいれば加槓しない」という v1 の保守的な制限を緩和する。今回この
//! defense model 拡張は行わない。
//!
//! # 複数のカン候補
//!
//! 同じ局面で2件以上のカン (暗槓・加槓を問わない) が成立した場合、production では**どれも
//! 選ばない** ([`KanDecisionReason::MultipleEligibleCandidates`])。自己リーチの前後どちらでも
//! 同じ扱いにする。合法 action の列挙順は server が決めるものなので、AI の tie-break に使わない。
//! 候補間を妥当に比較できる既存 comparator がまだ無いので、将来のためだけの独自 ranking も
//! 作らない。候補ごとの判断内訳は診断へそのまま残す。
//!
//! # 判断にかかるコスト
//!
//! 自己リーチ後は structural validation だけで結論が出るので、向聴も受け入れも打点も評価しない。
//! 自己リーチ前は安価な事前判定 (種別・他家リーチ・押し引き・副露数・向聴・受け入れ) を通った
//! 候補だけが打点比較へ進む。打点比較は押し引きが threat ありのテンパイで払うのと同じ1回分の
//! evaluation を両 side に対して行うもので、前方探索は通らない。合法なカンが1件も無い局面では
//! 候補の列挙で終わるので、通常の打牌局面へ載るコストは無い。
//!
//! TODO: 暗槓前後を ExpectedSelfTsumoValue で比較できるようにして、自己リーチ前のテンパイ以外の
//! 暗槓も production へ接続する。嶺上牌ぶんの追加ツモと山の残枚数の差をどう揃えるかが未解決。
//!
//! TODO: 複数のカン候補を既存評価で比較できるようにする。
//!
//! TODO: 搶槓 exact model を整備して、加槓の hard-safe 制限を緩和する
//! (「TODO: 搶槓 exact model」の節)。
//!
//! TODO: Daiminkan の reaction モデルを評価できるようにしてから、残り1種別を production へ
//! 接続する。

use std::cmp::Ordering;

use bot_logic::{
    DiscardEvaluation, EffectiveAcceptance, FixedMeldCount, Meld, MeldKind, MeldShape, OwnDiscards,
    TileCounts, TileId, TileType, calculate_acceptance_with_fixed_melds,
    calculate_acceptance_with_fixed_melds_and_visible_tiles, calculate_shanten_with_fixed_melds,
    is_menzen, structural_acceptance_tile_types_with_fixed_melds, tenpai_completed_hands,
    tenpai_wait_availability,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::defense::{CompressedStructuralTenpaiHiddenHandStates, is_discarded_by_player};
use crate::discard_selection::selected_discard_tenpai_wait_availability;
use crate::offense_value::{
    TenpaiOffenseMode, TenpaiOffenseValue, evaluate_tenpai_offense_value,
    evaluate_tenpai_offense_with_reach_legality,
};
use crate::prospective_value::future_reach_legal;
use crate::push_pull::PushPullMode;

/// 暗槓が消費する物理牌の枚数。
const ANKAN_CONSUMED_TILE_COUNT: usize = 4;

/// 加槓が消費する物理牌の枚数。追加する4枚目は `consumed` ではなく `tile` が持つ。
const KAKAN_CONSUMED_TILE_COUNT: usize = 3;

/// 評価対象のカン種別。
///
/// 今回 production で選べるのは [`Self::Ankan`] と [`Self::Kakan`] で、[`Self::Daiminkan`] は
/// 候補として診断に並ぶだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanKind {
    Ankan,
    Kakan,
    Daiminkan,
}

impl KanKind {
    /// 対応する既存の副露種別。
    pub fn meld_kind(self) -> MeldKind {
        match self {
            Self::Ankan => MeldKind::Ankan,
            Self::Kakan => MeldKind::Kakan,
            Self::Daiminkan => MeldKind::Daiminkan,
        }
    }

    /// 今回 production で選択できる種別か。
    pub fn is_production_connected(self) -> bool {
        matches!(self, Self::Ankan | Self::Kakan)
    }

    // 今回 production 接続していない種別の理由。
    fn not_connected_reason(self) -> Option<KanDecisionReason> {
        match self {
            Self::Ankan | Self::Kakan => None,
            Self::Daiminkan => Some(KanDecisionReason::DaiminkanNotConnected),
        }
    }
}

/// カンを採用した / しなかった理由。
///
/// カンを採用した理由は3つあり、[`Self::is_eligible`] がその3つを表す。
///
/// | 採用した理由 | policy |
/// | --- | --- |
/// | [`Self::EligibleAnkanNoRegression`] | 自己リーチ前の暗槓。暗槓前後を既存評価で比較して悪化しない |
/// | [`Self::EligibleAnkanAfterOwnReach`] | 自己リーチ後の暗槓の暫定 policy |
/// | [`Self::EligibleKakanNoRegression`] | 自己リーチ前の加槓。搶槓 hard-safe で、加槓前後を既存評価で比較して悪化しない |
///
/// 残りはすべて「今回はカンしない」理由であり、最初に落ちた条件を1つだけ表す。判定順は
/// [`KanCandidateDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanDecisionReason {
    /// 自己リーチ前の暗槓で、既存評価で測れる向聴・受け入れ・攻撃打点のどれも悪化しない。
    EligibleAnkanNoRegression,
    /// 自己リーチ後の暫定 policy で暗槓する。
    ///
    /// リーチ後は暗槓しなければ現在のツモ牌を強制ツモ切りするしかなく、その比較を既存評価で
    /// 同じ尺度に載せられない。そのため合法性と structural validation だけを条件にする。
    /// 向聴・受け入れ・攻撃打点・押し引き・他家リーチは採用条件にしない。
    EligibleAnkanAfterOwnReach,
    /// 自己リーチ前の加槓で、加槓牌の搶槓ロン不能を hard fact で確定でき、既存評価で測れる
    /// 向聴・受け入れ・攻撃打点のどれも悪化しない。
    EligibleKakanNoRegression,
    /// 自分の席を特定できず、リーチ済みかどうかを判断できない。
    ///
    /// リーチ済みだともしていないとも推測しないので、どちらの policy へも進めない。
    OwnReachUnknown,
    /// 大明槓はまだ production へ接続していない。reaction としての評価モデルが無い。
    DaiminkanNotConnected,
    /// 自己リーチ後の加槓。
    ///
    /// 元になる Pon があるので通常は自己リーチと両立しないが、server / context が矛盾した値を
    /// 持つ場合に自己リーチ状態を推測で `false` へ倒さず、加槓しない。
    KakanAfterOwnReach,
    /// 加槓が置換する既存 Pon を自分の副露から特定できない。
    KakanWithoutMatchingPon,
    /// 加槓の形が成り立たない。
    ///
    /// consumed が3枚でない、consumed と追加牌の牌種が揃わない、追加牌が手牌とツモ牌に無い、
    /// 置換後の面子が槓の形にならないのいずれか。物理 [`TileId`] の完全一致は条件にしない。
    InvalidKakanShape,
    /// 加槓牌について、全他家からの搶槓ロン不能を hard fact で確定できない。
    ///
    /// v1 の根拠は「その player 自身の河に加槓牌と同じ牌種がある」ことだけで、搶槓 risk の推定は
    /// 行わない。
    KakanChankanNotHardSafe,
    /// 自己リーチ前で、他家にリーチ者がいる。新ドラがリーチ者の打点へ与える影響を既存評価で
    /// 測れない。自己リーチ後の暫定 policy ではこの理由で落とさない。
    OpponentReached,
    /// 自己リーチ前で、既存 Push/Pull policy が Push と判定していない。
    ///
    /// 自己リーチ後は降りようがないので、この理由で落とさない。
    NotPush,
    /// 自分のツモを経たと確認できない。暗槓はツモ番の action なので、この局面では判断しない。
    NotAfterOwnDraw,
    /// 自分の副露済み面子数が不明。0副露と推測しない。
    FixedMeldCountUnknown,
    /// 暗槓後の副露済み面子数が上限を超える。
    FixedMeldCountOverflow,
    /// consumed が4枚でない・手牌に無い・カンの形にならない。
    InvalidConsumed,
    /// 比較の基準になる通常打牌評価が無く、暗槓しない場合と比べられない。
    NormalDiscardUnavailable,
    /// 暗槓後の向聴数が、暗槓しない場合の通常打牌後より悪くなる。
    ShantenRegresses,
    /// 暗槓後の向聴数が通常打牌後より進む。
    ///
    /// 受け入れも打点も向聴段階が違えば同じ意味の値ではないので、枚数の単純比較で結論しない。
    /// 向聴が進んだ分の価値も既存 primitive では確定できないため、改善そのものを暗槓の根拠にも
    /// しない。
    ShantenImprovedNotComparable,
    /// 向聴数は同じだが、暗槓後の受け入れが通常打牌後より減る。
    AcceptanceRegresses,
    /// 暗槓前後の攻撃打点を既存評価で比較できない。
    ///
    /// どちらかがテンパイでない、攻撃モードが [`TenpaiOffenseMode::Unknown`] または食い違う、
    /// どちらかの攻撃打点が [`OffenseValue::Unknown`](crate::offense_value::OffenseValue::Unknown) のいずれか。速度が悪化しないことだけを
    /// 根拠に暗槓せず、通常打牌を維持する。
    ValueNotEvaluable,
    /// 暗槓後の攻撃打点が、暗槓しない場合の通常打牌後より下がる。
    ValueRegresses,
    /// 成立した暗槓候補が2件以上ある。
    ///
    /// 合法 action の列挙順を tie-break にしないため、候補間を比較できる既存評価が無いうちは
    /// どれも選ばない。候補ごとの判断内訳は診断へそのまま残す。
    MultipleEligibleCandidates,
}

impl KanDecisionReason {
    /// カンを採用した理由か。
    pub fn is_eligible(self) -> bool {
        matches!(
            self,
            Self::EligibleAnkanNoRegression
                | Self::EligibleAnkanAfterOwnReach
                | Self::EligibleKakanNoRegression
        )
    }
}

/// 比較に使った13枚相当 state 1つ分の既存評価。
///
/// 値はすべて既存 layer が求めたもので、診断のために向聴も受け入れも打点も計算し直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KanHandDiagnostic {
    /// 副露済み面子数を含めた effective shanten。
    pub shanten: i8,
    /// 受け入れ残枚数 [枚]。`shanten` が違う state 同士では同じ意味の値にならない。
    pub acceptance_remaining: u8,
    /// 受け入れ牌種数。`acceptance_remaining` と同じく向聴段階に依存する。
    pub acceptance_type_count: usize,
    /// テンパイの場合の攻撃モードと確定打点。押し引き・リーチ判断が使うのと同じ値。
    ///
    /// テンパイでない state と、待ちを組み立てられない state では `None`。
    pub offense: Option<TenpaiOffenseValue>,
}

impl KanHandDiagnostic {
    /// 生きた待ちの支払い合計の残枚数加重合計 [点]。確定しない場合は `None`。
    pub fn weighted_total(&self) -> Option<u64> {
        self.offense?.value.weighted_total()
    }

    /// 攻撃モード。テンパイでない場合は `None`。
    pub fn offense_mode(&self) -> Option<TenpaiOffenseMode> {
        Some(self.offense?.mode)
    }
}

/// 加槓牌の搶槓 hard-safe 判定の内訳。
///
/// 他家1人ごとの根拠は [`KakanChankanSafety`] が表す。`temporary_passed_tiles` /
/// `same_hand_passed_tiles` / Suji / Wall / OneChance / 通常 Dahai 用 exact `R/T` は根拠に
/// 使わない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KakanChankanDiagnostic {
    /// 加槓で追加する4枚目の牌種。
    pub tile: TileType,
    /// 自分以外の3家の判定内訳。player id の昇順。
    pub opponents: Vec<KakanChankanOpponent>,
    /// 全他家について搶槓ロン不能を確定できたか。
    ///
    /// 3家すべてが個別に hard-safe の場合だけ `true`。production の判断はこの値をそのまま読み、
    /// 集約規則を別に持たない。
    pub hard_safe: bool,
}

/// 他家1人分の搶槓 hard-safe の根拠、または確定できなかった理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KakanChankanSafety {
    /// その player 自身の河に加槓牌と同じ牌種がある。恒常フリテンでロンできない。
    RiverFuriten,
    /// structural hidden-hand model で、加槓牌を加えて Standard の和了形が完成する hidden
    /// state が1つも無い (`target_completion_state_weight == 0`)。
    NoStructuralCompletion,
    /// 加槓牌を加えると和了形になる hidden state が存在する。
    ///
    /// 実際に搶槓されると断定する意味ではないが、hard-safe とも確定できない。
    StructuralCompletionPossible,
    /// structural hidden-hand model を構築できない (門前非リーチなど)。hard-safe unknown。
    StructuralModelUnavailable,
}

impl KakanChankanSafety {
    /// 搶槓ロン不能を hard fact で確定できた根拠か。
    pub fn is_hard_safe(self) -> bool {
        matches!(self, Self::RiverFuriten | Self::NoStructuralCompletion)
    }
}

/// 他家1人分の搶槓 hard-safe 判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KakanChankanOpponent {
    pub player: usize,
    /// その player 自身の河に加槓牌と同じ牌種があるか ([`is_discarded_by_player`])。
    pub river_furiten: bool,
    /// structural hidden-hand model を評価した場合の target completion state weight [重み]。
    ///
    /// 河フリテンで確定した player と、model を構築できない player では評価しないので `None`。
    pub structural_completion_weight: Option<u128>,
    /// hard-safe の根拠、または確定できなかった理由。
    pub safety: KakanChankanSafety,
}

impl KakanChankanOpponent {
    /// この player について搶槓ロン不能を確定できたか。
    pub fn hard_safe(&self) -> bool {
        self.safety.is_hard_safe()
    }
}

/// 合法な `LegalAction::Ankan` / `LegalAction::Kakan` / `LegalAction::Daiminkan` 1件ごとの判断内訳。
///
/// 各フィールドは判定が実際にそこまで進んだ場合だけ `Some` になり、進まなかった判定は推測せず
/// `None` のままにする。評価不能で落ちた候補は `reason` がその理由を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanCandidateDiagnostic {
    pub action: LegalAction,
    pub kind: KanKind,
    /// カンの対象牌種。Ankan は consumed の牌種、Kakan は追加する4枚目、Daiminkan は対象の打牌。
    pub tile: Option<TileType>,
    /// 加槓が置換する既存の Pon。加槓以外と、元 Pon を特定できなかった場合は `None`。
    pub matching_pon: Option<Meld>,
    /// 加槓牌の搶槓 hard-safe 判定。加槓以外と、判定まで進まなかった場合は `None`。
    pub chankan: Option<KakanChankanDiagnostic>,
    pub current_fixed_meld_count: Option<FixedMeldCount>,
    /// カン後の副露済み面子数。暗槓・大明槓は1増え、加槓は Pon の置換なので変わらない。
    pub post_kan_fixed_meld_count: Option<FixedMeldCount>,
    /// カンしない場合に採用する通常打牌と、その打牌後13枚の既存評価。
    ///
    /// production の通常打牌選択が選んだ1件そのもので、この層で選び直さない。
    pub baseline_discard: Option<TileType>,
    pub baseline: Option<KanHandDiagnostic>,
    /// 暗槓後の13枚相当 state の既存評価。
    pub post_kan: Option<KanHandDiagnostic>,
    pub eligible: bool,
    pub selected: bool,
    pub reason: KanDecisionReason,
}

impl KanCandidateDiagnostic {
    /// 暗槓後 - 暗槓しない場合の向聴数差。負なら暗槓後の方が良い。両方を評価した場合だけ `Some`。
    pub fn shanten_delta(&self) -> Option<i8> {
        Some(self.post_kan?.shanten - self.baseline?.shanten)
    }

    /// 暗槓後 - 暗槓しない場合の受け入れ残枚数差 [枚]。符号付き。
    ///
    /// 向聴段階が違う state 同士では同じ意味の値にならないので、production の判断は
    /// `shanten_delta() == Some(0)` の場合しかこの差を読まない。
    pub fn acceptance_remaining_delta(&self) -> Option<i16> {
        Some(
            i16::from(self.post_kan?.acceptance_remaining)
                - i16::from(self.baseline?.acceptance_remaining),
        )
    }

    /// 暗槓後 - 暗槓しない場合の受け入れ牌種数差。符号付き。
    ///
    /// 読み方の制約は [`Self::acceptance_remaining_delta`] と同じ。
    pub fn acceptance_type_delta(&self) -> Option<isize> {
        Some(
            self.post_kan?.acceptance_type_count as isize
                - self.baseline?.acceptance_type_count as isize,
        )
    }

    /// 暗槓後 - 暗槓しない場合の攻撃打点差 [点]。符号付き。
    ///
    /// 両側の打点を確定できた場合だけ `Some`。攻撃モードが食い違う組み合わせでも値は返るが、
    /// production の判断はモードが一致する場合しか読まない。
    pub fn weighted_total_delta(&self) -> Option<i128> {
        Some(
            i128::from(self.post_kan?.weighted_total()?)
                - i128::from(self.baseline?.weighted_total()?),
        )
    }
}

/// カン判断の構造化診断。
///
/// `selected` は `ShantenAgent::act()` が実際に採用したカンそのもので、診断用の別判断ロジック
/// は持たない。採用が無い場合の `reason` は最初の候補が落ちた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanDecisionDiagnostic {
    pub selected: Option<LegalAction>,
    pub reason: KanDecisionReason,
    pub candidates: Vec<KanCandidateDiagnostic>,
}

// カン判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 合法なカンが1件も無ければ検討自体を行わず None。1件以上ある場合は候補ごとに独立して条件を
// 評価し、成立した候補がちょうど1件の場合だけそれを選ぶ。2件以上成立した場合は合法 action の
// 列挙順を tie-break にせず、どれも選ばずに理由だけを残す。
//
// `normal_discard` は暗槓しない場合に採用する通常打牌の評価。production の通常打牌選択が選んだ
// ものをそのまま受け取り、この層で打牌を選び直さない。
pub(crate) fn evaluate_kan_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
) -> Option<KanDecisionDiagnostic> {
    let mut candidates: Vec<KanCandidateDiagnostic> = Vec::new();
    for action in legal_actions {
        let Some((kind, called_tile, consumed)) = normalize_kan(action) else {
            continue;
        };
        let mut candidate = new_kan_candidate(action, kind, called_tile, consumed);
        let reason = evaluate_kan_candidate(
            ctx,
            legal_actions,
            kind,
            called_tile,
            consumed,
            mode,
            normal_discard,
            &mut candidate,
        );
        candidate.eligible = reason.is_eligible();
        candidate.reason = reason;
        candidates.push(candidate);
    }

    if candidates.is_empty() {
        return None;
    }

    let eligible: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.eligible)
        .map(|(index, _)| index)
        .collect();

    // 採用が無い場合の理由は最初の候補が落ちた理由。ちょうど1件成立した場合だけ採用し、2件
    // 以上成立した場合は候補固有の理由ではなく複数成立そのものを理由にする。
    let (selected_index, reason) = match eligible.as_slice() {
        [] => (None, candidates[0].reason),
        [index] => (Some(*index), candidates[*index].reason),
        _ => (None, KanDecisionReason::MultipleEligibleCandidates),
    };

    if let Some(index) = selected_index {
        candidates[index].selected = true;
    }
    let selected = selected_index.map(|index| candidates[index].action.clone());

    Some(KanDecisionDiagnostic {
        selected,
        reason,
        candidates,
    })
}

// 合法 action をカンの共通表現へ正規化する。それ以外の action は対象外。
fn normalize_kan(action: &LegalAction) -> Option<(KanKind, Option<TileId>, &[TileId])> {
    match action {
        LegalAction::Ankan { consumed } => Some((KanKind::Ankan, None, consumed)),
        LegalAction::Kakan { tile, consumed } => Some((KanKind::Kakan, Some(*tile), consumed)),
        LegalAction::Daiminkan { tile, consumed } => {
            Some((KanKind::Daiminkan, Some(*tile), consumed))
        }
        _ => None,
    }
}

fn new_kan_candidate(
    action: &LegalAction,
    kind: KanKind,
    called_tile: Option<TileId>,
    consumed: &[TileId],
) -> KanCandidateDiagnostic {
    KanCandidateDiagnostic {
        action: action.clone(),
        kind,
        tile: called_tile
            .or_else(|| consumed.first().copied())
            .map(TileId::tile_type),
        matching_pon: None,
        chankan: None,
        current_fixed_meld_count: None,
        post_kan_fixed_meld_count: None,
        baseline_discard: None,
        baseline: None,
        post_kan: None,
        eligible: false,
        selected: false,
        reason: KanDecisionReason::EligibleAnkanNoRegression,
    }
}

// 候補1件の条件を上から順に評価し、最初に落ちた理由を返す。評価が進んだ範囲の値だけを
// candidate へ書き込み、評価しなかった項目は None のままにする。
//
// 暗槓は自己リーチ済みかどうかで policy そのものが分かれる。自己リーチ後は structural
// validation だけ、自己リーチ前は既存評価による暗槓前後の比較を通る。加槓は自己リーチ前だけを
// 対象にし、自己リーチ状態が分からない場合も矛盾している場合も推測で処理しない。
#[allow(clippy::too_many_arguments)]
fn evaluate_kan_candidate(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    kind: KanKind,
    called_tile: Option<TileId>,
    consumed: &[TileId],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
    candidate: &mut KanCandidateDiagnostic,
) -> KanDecisionReason {
    if let Some(reason) = kind.not_connected_reason() {
        return reason;
    }

    match (kind, ctx.own_reached()) {
        // 自席を特定できない。リーチ済みだともしていないとも推測しない。
        (_, None) => KanDecisionReason::OwnReachUnknown,
        (KanKind::Ankan, Some(true)) => evaluate_ankan_after_own_reach(ctx, consumed, candidate),
        (KanKind::Ankan, Some(false)) => evaluate_ankan_before_own_reach(
            ctx,
            legal_actions,
            consumed,
            mode,
            normal_discard,
            candidate,
        ),
        (KanKind::Kakan, Some(true)) => KanDecisionReason::KakanAfterOwnReach,
        (KanKind::Kakan, Some(false)) => evaluate_kakan_before_own_reach(
            ctx,
            legal_actions,
            called_tile,
            consumed,
            mode,
            normal_discard,
            candidate,
        ),
        // 接続していない種別はここへ来る前に落ちる。
        (KanKind::Daiminkan, _) => KanDecisionReason::DaiminkanNotConnected,
    }
}

// 自己リーチ後の暫定 policy。server が合法とした暗槓を原則そのまま採用する。
//
// 合法性 (待ちが変わらないかを含む) は `legal_actions` が source of truth なので再判定しない。
// 確かめるのは、その合法手を実際に手牌から組み立てられるかという structural validation だけで、
// 向聴・受け入れ・攻撃打点・押し引き・他家リーチはどれも採用条件にしない。
fn evaluate_ankan_after_own_reach(
    ctx: &GameContext,
    consumed: &[TileId],
    candidate: &mut KanCandidateDiagnostic,
) -> KanDecisionReason {
    match validate_ankan_structure(ctx, consumed, candidate) {
        Ok(_) => KanDecisionReason::EligibleAnkanAfterOwnReach,
        Err(reason) => reason,
    }
}

// 自己リーチ前の暗槓。暗槓前後を既存評価で比べ、悪化しないと確認できた場合だけ採用する。
//
// 判定順は安価な fact (他家リーチ・押し引き・副露数・カンの形) から始め、向聴と受け入れを
// 通った候補だけが打点比較へ進む。
fn evaluate_ankan_before_own_reach(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    consumed: &[TileId],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
    candidate: &mut KanCandidateDiagnostic,
) -> KanDecisionReason {
    if ctx.any_opponent_reached() {
        return KanDecisionReason::OpponentReached;
    }

    if mode != PushPullMode::Push {
        return KanDecisionReason::NotPush;
    }

    if !ctx.is_after_own_draw() {
        return KanDecisionReason::NotAfterOwnDraw;
    }

    let (meld, post_kan_tiles, post_kan_fixed_meld_count) =
        match validate_ankan_structure(ctx, consumed, candidate) {
            Ok(validated) => validated,
            Err(reason) => return reason,
        };

    let Some(normal_discard) = normal_discard else {
        return KanDecisionReason::NormalDiscardUnavailable;
    };
    candidate.baseline_discard = Some(normal_discard.discard);
    let baseline = evaluate_baseline_hand(ctx, legal_actions, normal_discard);
    candidate.baseline = Some(baseline);

    let mut post_kan_melds = ctx.own_melds().unwrap_or_default().to_vec();
    post_kan_melds.push(meld);
    let post_kan = evaluate_post_kan_hand(
        ctx,
        &post_kan_tiles,
        &post_kan_melds,
        post_kan_fixed_meld_count,
    );
    candidate.post_kan = Some(post_kan);

    match compare_post_kan_hand(baseline, post_kan) {
        Ok(()) => KanDecisionReason::EligibleAnkanNoRegression,
        Err(reason) => reason,
    }
}

// 自己リーチ前の加槓。搶槓ロン不能を hard fact で確定できた候補だけを、暗槓と同じ既存評価で
// 比較する。
//
// 判定順は暗槓と揃え、安価な fact (他家リーチ・押し引き・自摸・加槓の形と元 Pon・搶槓
// hard-safe) を通った候補だけが向聴・受け入れ・打点の比較へ進む。搶槓 hard-safe は打点比較より
// 前に置く。v1 では搶槓 risk を推定しないので、hard-safe でない候補は速度や打点を比べる前に
// 落ちる。
#[allow(clippy::too_many_arguments)]
fn evaluate_kakan_before_own_reach(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    called_tile: Option<TileId>,
    consumed: &[TileId],
    mode: PushPullMode,
    normal_discard: Option<&DiscardEvaluation>,
    candidate: &mut KanCandidateDiagnostic,
) -> KanDecisionReason {
    if ctx.any_opponent_reached() {
        return KanDecisionReason::OpponentReached;
    }

    if mode != PushPullMode::Push {
        return KanDecisionReason::NotPush;
    }

    if !ctx.is_after_own_draw() {
        return KanDecisionReason::NotAfterOwnDraw;
    }

    let (added_tile, post_kan_melds, post_kan_tiles, fixed_meld_count) =
        match validate_kakan_structure(ctx, called_tile, consumed, candidate) {
            Ok(validated) => validated,
            Err(reason) => return reason,
        };

    let Some(chankan) = chankan_hard_safety(ctx, added_tile.tile_type()) else {
        return KanDecisionReason::KakanChankanNotHardSafe;
    };
    let hard_safe = chankan.hard_safe;
    candidate.chankan = Some(chankan);
    if !hard_safe {
        return KanDecisionReason::KakanChankanNotHardSafe;
    }

    let Some(normal_discard) = normal_discard else {
        return KanDecisionReason::NormalDiscardUnavailable;
    };
    candidate.baseline_discard = Some(normal_discard.discard);
    let baseline = evaluate_baseline_hand(ctx, legal_actions, normal_discard);
    candidate.baseline = Some(baseline);

    let post_kan = evaluate_post_kan_hand(ctx, &post_kan_tiles, &post_kan_melds, fixed_meld_count);
    candidate.post_kan = Some(post_kan);

    match compare_post_kan_hand(baseline, post_kan) {
        Ok(()) => KanDecisionReason::EligibleKakanNoRegression,
        Err(reason) => reason,
    }
}

// 暗槓の structural validation。自己リーチの前後で共有する。
//
// 合法性そのものは `legal_actions` が source of truth なので、ここで確かめるのは「その暗槓を
// 実際に手牌から組み立てられるか」だけになる。副露済み面子数は診断へ書き込む。
fn validate_ankan_structure(
    ctx: &GameContext,
    consumed: &[TileId],
    candidate: &mut KanCandidateDiagnostic,
) -> Result<(Meld, Vec<TileId>, FixedMeldCount), KanDecisionReason> {
    let Some(current_fixed_meld_count) = ctx.own_fixed_meld_count() else {
        return Err(KanDecisionReason::FixedMeldCountUnknown);
    };
    candidate.current_fixed_meld_count = Some(current_fixed_meld_count);

    let Some(post_kan_fixed_meld_count) = FixedMeldCount::new(current_fixed_meld_count.get() + 1)
    else {
        return Err(KanDecisionReason::FixedMeldCountOverflow);
    };
    candidate.post_kan_fixed_meld_count = Some(post_kan_fixed_meld_count);

    let Some((meld, post_kan_tiles)) = ankan_meld_and_concealed_tiles(ctx, consumed) else {
        return Err(KanDecisionReason::InvalidConsumed);
    };

    Ok((meld, post_kan_tiles, post_kan_fixed_meld_count))
}

// カン後13枚相当 state を、カンしない場合の通常打牌後と比べる。悪化しなければ `Ok(())` で、
// 採用理由は種別ごとに呼び出し側が決める。
//
// 向聴・受け入れ・打点の semantics は暗槓と加槓で同じものを共有し、種別ごとに規則を複製しない。
// 受け入れも打点も「現在の向聴数」に紐づく値なので、向聴段階が同じ場合だけ比較する。
fn compare_post_kan_hand(
    baseline: KanHandDiagnostic,
    post_kan: KanHandDiagnostic,
) -> Result<(), KanDecisionReason> {
    match post_kan.shanten.cmp(&baseline.shanten) {
        Ordering::Greater => return Err(KanDecisionReason::ShantenRegresses),
        Ordering::Less => return Err(KanDecisionReason::ShantenImprovedNotComparable),
        Ordering::Equal => {}
    }

    if post_kan.acceptance_remaining < baseline.acceptance_remaining
        || post_kan.acceptance_type_count < baseline.acceptance_type_count
    {
        return Err(KanDecisionReason::AcceptanceRegresses);
    }

    compare_offense_value(baseline, post_kan)
}

// 速度が悪化しない候補について、カン前後の攻撃打点を比べる。悪化しなければ `Ok(())` で、
// 採用理由は種別ごとに呼び出し側が決める。
//
// 同じ尺度の値を確定できない組み合わせはすべて [`KanDecisionReason::ValueNotEvaluable`] にし、
// 速度非劣化だけを根拠にカンへ倒さない。暗槓と加槓でこの規則を分けない。
fn compare_offense_value(
    baseline: KanHandDiagnostic,
    post_kan: KanHandDiagnostic,
) -> Result<(), KanDecisionReason> {
    let (Some(baseline_offense), Some(post_kan_offense)) = (baseline.offense, post_kan.offense)
    else {
        return Err(KanDecisionReason::ValueNotEvaluable);
    };

    // リーチ手とダマ手の打点は別 baseline の値なので同じ尺度で比べない。
    if baseline_offense.mode != post_kan_offense.mode
        || baseline_offense.mode == TenpaiOffenseMode::Unknown
    {
        return Err(KanDecisionReason::ValueNotEvaluable);
    }

    let (Some(baseline_total), Some(post_kan_total)) = (
        baseline_offense.value.weighted_total(),
        post_kan_offense.value.weighted_total(),
    ) else {
        return Err(KanDecisionReason::ValueNotEvaluable);
    };

    if post_kan_total < baseline_total {
        return Err(KanDecisionReason::ValueRegresses);
    }

    Ok(())
}

// 加槓の structural validation と Pon → Kakan の post-state 構築。
//
// 合法性そのものは `legal_actions` が source of truth なので、ここで確かめるのは「その加槓を
// 実際に自分の副露と手牌から組み立てられるか」だけになる。RiichiLab の mjai → TileId 変換は
// 黒牌の物理 copy ID を復元できないため、consumed と既存 Pon の物理牌が完全一致することは
// 要求せず、牌種 semantics だけを確かめる。
//
// 加槓は固定面子の追加ではなく置換なので、副露済み面子数は前後で変わらない。返す副露 list は
// 元 Pon と同じ位置を Kakan で置き換えたもので、`called_tile` は元 Pon のものを保持する。
fn validate_kakan_structure(
    ctx: &GameContext,
    called_tile: Option<TileId>,
    consumed: &[TileId],
    candidate: &mut KanCandidateDiagnostic,
) -> Result<(TileId, Vec<Meld>, Vec<TileId>, FixedMeldCount), KanDecisionReason> {
    let Some(added_tile) = called_tile else {
        return Err(KanDecisionReason::InvalidKakanShape);
    };
    let tile_type = added_tile.tile_type();
    if consumed.len() != KAKAN_CONSUMED_TILE_COUNT
        || consumed.iter().any(|tile| tile.tile_type() != tile_type)
    {
        return Err(KanDecisionReason::InvalidKakanShape);
    }

    let (Some(fixed_meld_count), Some(melds)) = (ctx.own_fixed_meld_count(), ctx.own_melds())
    else {
        return Err(KanDecisionReason::FixedMeldCountUnknown);
    };
    candidate.current_fixed_meld_count = Some(fixed_meld_count);
    // 加槓は Pon を置き換えるので、副露済み面子数は変わらない。
    candidate.post_kan_fixed_meld_count = Some(fixed_meld_count);

    let Some(index) = matching_pon_index(melds, tile_type) else {
        return Err(KanDecisionReason::KakanWithoutMatchingPon);
    };
    let pon = &melds[index];
    candidate.matching_pon = Some(pon.clone());

    let Some(post_kan_tiles) = concealed_tiles_without_added_tile(ctx, added_tile) else {
        return Err(KanDecisionReason::InvalidKakanShape);
    };

    let mut kakan_tiles = pon.tiles().to_vec();
    kakan_tiles.push(added_tile);
    let kakan = Meld::new(MeldKind::Kakan, kakan_tiles, pon.called_tile());
    if kakan.shape().is_none() {
        return Err(KanDecisionReason::InvalidKakanShape);
    }

    let mut post_kan_melds = melds.to_vec();
    post_kan_melds[index] = kakan;

    Ok((added_tile, post_kan_melds, post_kan_tiles, fixed_meld_count))
}

// 加槓が置換する既存 Pon の位置。面子の形は既存 [`Meld::shape`] が source of truth で、
// 物理 [`TileId`] の一致は見ない。同じ牌種の Pon は1局面に1つしか存在しない。
fn matching_pon_index(melds: &[Meld], tile_type: TileType) -> Option<usize> {
    melds.iter().position(|meld| {
        meld.kind() == MeldKind::Pon && meld.shape() == Some(MeldShape::Triplet { tile: tile_type })
    })
}

// 加槓で追加する1枚だけを手牌 + ツモ牌から取り除いた concealed hand。
//
// 取り除く牌は牌種と赤牌かどうかの両方が一致する1枚に限る。牌種だけで一致させると赤5を誤って
// 槓へ持っていき、手牌に残る赤ドラを取り違えるためである。
fn concealed_tiles_without_added_tile(
    ctx: &GameContext,
    added_tile: TileId,
) -> Option<Vec<TileId>> {
    let mut remaining: Vec<TileId> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();
    let position = remaining.iter().position(|held| {
        held.tile_type() == added_tile.tile_type() && held.is_red() == added_tile.is_red()
    })?;
    remaining.remove(position);
    Some(remaining)
}

// 加槓牌の搶槓 hard-safe 判定。自席を特定できない場合は player 0 などを推測せず `None`。
//
// 自分以外の3家それぞれを独立に判定し、全員について搶槓ロン不能を確定できた場合だけ
// hard-safe にする。production の判断と診断はこの1本を共有し、集約規則を別に持たない。
fn chankan_hard_safety(ctx: &GameContext, tile: TileType) -> Option<KakanChankanDiagnostic> {
    let own_seat = usize::from(ctx.player_id()?);
    let opponents: Vec<KakanChankanOpponent> = (0..ctx.discards().len())
        .filter(|&player| player != own_seat)
        .map(|player| chankan_opponent_safety(ctx, tile, player))
        .collect();

    Some(KakanChankanDiagnostic {
        tile,
        hard_safe: !opponents.is_empty() && opponents.iter().all(KakanChankanOpponent::hard_safe),
        opponents,
    })
}

// 他家1人について、加槓牌で搶槓ロンされ得ないと確定できるか。
//
// 根拠は2つあり、どちらも公開情報から確定する hard fact である。確率や閾値は持たない。
//
// | 根拠 | 意味 |
// | --- | --- |
// | [`KakanChankanSafety::RiverFuriten`] | その player 自身の河に加槓牌がある。恒常フリテンでロンできない |
// | [`KakanChankanSafety::NoStructuralCompletion`] | 公開情報と整合する hidden hand の中に、加槓牌で Standard の和了形が完成する state が1つも無い |
//
// structural completion は役を見ないので、通常ロンで役があるかにも搶槓で Chankan が付くかにも
// 依存しない。「そもそもその牌で和了形にならない」という構造の事実なので、`chankan = true` の
// 局面でもそのまま安全根拠に使える。通常 Dahai 用の exact `R/T`
// ([`ron_risk_evidence`](CompressedStructuralTenpaiHiddenHandStates::ron_risk_evidence) /
// [`ron_capable_state_weight`](CompressedStructuralTenpaiHiddenHandStates::ron_capable_state_weight))
// は役判定を含み `chankan = false` 前提の経路があるので、ここでは使わない。
//
// completion が1つでもある場合は「搶槓される」と断定せず、hard-safe を証明できない候補として
// 扱う。model を構築できない player (門前非リーチなど) も同じく unknown にし、Kan 側で hidden-hand
// model の対応範囲を推測で広げない。
//
// 判定順は河フリテンが先で、確定した player には exact counting を行わない。
fn chankan_opponent_safety(
    ctx: &GameContext,
    tile: TileType,
    player: usize,
) -> KakanChankanOpponent {
    if is_discarded_by_player(tile, player, ctx) {
        return KakanChankanOpponent {
            player,
            river_furiten: true,
            structural_completion_weight: None,
            safety: KakanChankanSafety::RiverFuriten,
        };
    }

    let Ok(mut states) = CompressedStructuralTenpaiHiddenHandStates::new(player, ctx) else {
        return KakanChankanOpponent {
            player,
            river_furiten: false,
            structural_completion_weight: None,
            safety: KakanChankanSafety::StructuralModelUnavailable,
        };
    };

    let weight = states.target_completion_state_weight(tile).weight;
    KakanChankanOpponent {
        player,
        river_furiten: false,
        structural_completion_weight: Some(weight),
        safety: if weight == 0 {
            KakanChankanSafety::NoStructuralCompletion
        } else {
            KakanChankanSafety::StructuralCompletionPossible
        },
    }
}

/// カンして嶺上牌を1枚引いた後の、山の残りツモ可能枚数 [枚]。暗槓と加槓で共有する。
///
/// 嶺上牌は王牌から引くが、その分だけ王牌が山から補充されるので、暗槓後にツモできる枚数は
/// 現在より1枚少なくなる
/// ([`remaining_tiles`](crate::context::TableStateFacts::remaining_tiles) は live wall の残り
/// ツモ可能枚数)。現在の枚数が分かっている局面ではこの既知 fact から導き、分からない局面だけ
/// `None` にして推測しない。
///
/// 残り0枚からは引けないので `Some(0)` はそもそもカンが成立しない値だが、unknown へ倒すと
/// [`is_reach_legal`](crate::reach_policy::is_reach_legal) の unknown 規則でリーチ可能側へ
/// 倒れてしまう。飽和減算で `Some(0)` を維持し、リーチ不可のまま残す。
fn post_kan_remaining_tiles(ctx: &GameContext) -> Option<u32> {
    Some(ctx.remaining_tiles()?.saturating_sub(1))
}

// 暗槓で consumed 4枚を取り除いた後の副露と concealed hand。
//
// 手牌 + ツモ牌から consumed の物理牌をちょうど1枚ずつ取り除き、取り除いた4枚がカンの形に
// なることを既存 [`Meld::shape`] で確かめる。形の規則をこの層で持たない。
fn ankan_meld_and_concealed_tiles(
    ctx: &GameContext,
    consumed: &[TileId],
) -> Option<(Meld, Vec<TileId>)> {
    if consumed.len() != ANKAN_CONSUMED_TILE_COUNT {
        return None;
    }

    let mut remaining: Vec<TileId> = ctx
        .hand_tiles()
        .iter()
        .copied()
        .chain(ctx.drawn_tile())
        .collect();
    for consumed_tile in consumed {
        let position = remaining.iter().position(|held| held == consumed_tile)?;
        remaining.remove(position);
    }

    let meld = Meld::new(MeldKind::Ankan, consumed.to_vec(), None);
    meld.shape()?;
    Some((meld, remaining))
}

// カンしない場合に採用する通常打牌の、打牌後13枚相当の既存評価。暗槓と加槓で共有する。
//
// 向聴・受け入れは production の通常打牌選択が求めた [`DiscardEvaluation`] そのもので、打点も
// 押し引き・リーチ判断と同じ helper を通す。この層で求め直す評価は持たない。
fn evaluate_baseline_hand(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    evaluation: &DiscardEvaluation,
) -> KanHandDiagnostic {
    let offense = selected_discard_tenpai_wait_availability(ctx, evaluation)
        .map(|wait| evaluate_tenpai_offense_value(ctx, evaluation, &wait, legal_actions));

    KanHandDiagnostic {
        shanten: evaluation.min_shanten_after_discard(),
        acceptance_remaining: evaluation.acceptance_total_remaining(),
        acceptance_type_count: evaluation.acceptance_type_count(),
        offense,
    }
}

// カン後13枚相当 state の既存評価。見え牌の有無による経路分岐は通常打牌評価と揃える。
//
// 暗槓は `10枚 + 副露 N+1`、加槓は `13枚 + 副露 N` で、どちらも `13 - 3 * 副露数` 枚の同じ
// 大きさの手牌になる。評価そのものは種別で分けない。
fn evaluate_post_kan_hand(
    ctx: &GameContext,
    tiles: &[TileId],
    melds: &[Meld],
    fixed_meld_count: FixedMeldCount,
) -> KanHandDiagnostic {
    let counts = TileCounts::from_tiles(tiles.iter().copied());
    let shanten = calculate_shanten_with_fixed_melds(&counts, fixed_meld_count).min();
    let acceptance = if ctx.visible_tiles().is_empty() {
        calculate_acceptance_with_fixed_melds(&counts, fixed_meld_count)
    } else {
        calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &counts,
            fixed_meld_count,
            ctx.visible_tiles(),
        )
    };
    let offense =
        post_kan_tenpai_offense(ctx, tiles, melds, &counts, fixed_meld_count, &acceptance);

    KanHandDiagnostic {
        shanten,
        acceptance_remaining: acceptance.total_remaining(),
        acceptance_type_count: acceptance.tiles.len(),
        offense,
    }
}

// カン後13枚相当 state がテンパイの場合の攻撃打点。テンパイでない場合と、待ちや完成手を
// 組み立てられない場合は `None`。
//
// 待ちは既存のフリテン基盤、完成手は既存の [`tenpai_completed_hands`]、打点と攻撃モードは
// 押し引き・リーチ判断が使う [`evaluate_tenpai_offense_with_reach_legality`] をそのまま通す。
// カンは評価対象の副露として渡すので、符も役も既存 scoring がカン込みで求めた値になる。
//
// 攻撃モードを決める「リーチが合法か」は、現在局面の `legal_actions` を未来へ流用せず、共有
// 条件をカン後の手牌の事実へ適用した [`future_reach_legal`] で求める。門前かどうかはカンを
// 含む評価対象副露から、山の残りツモ可能枚数は [`post_kan_remaining_tiles`] から取る。加槓後は
// 元 Pon が Kakan になるだけなので門前へは戻らず、既存 [`is_menzen`] がそのまま開いた手と
// 判定する。
//
// 自分の河はカンで変わらない。履歴依存フリテンは、カンの前に自分のツモを経ている事実を
// `ctx` 側の既存補正 ([`GameContext::history_furiten_after_own_discard`]) から取る。カンで
// 増えるドラ表示牌は未知なので、`ctx` の既知のドラ表示牌だけで評価する。
fn post_kan_tenpai_offense(
    ctx: &GameContext,
    tiles: &[TileId],
    melds: &[Meld],
    counts: &TileCounts,
    fixed_meld_count: FixedMeldCount,
    acceptance: &EffectiveAcceptance,
) -> Option<TenpaiOffenseValue> {
    let wait = tenpai_wait_availability(
        acceptance,
        &structural_acceptance_tile_types_with_fixed_melds(counts, fixed_meld_count),
        &OwnDiscards::from_optional_river(ctx.own_discards()),
        ctx.history_furiten_after_own_discard(),
    )?;
    let hands =
        tenpai_completed_hands(tiles, melds, acceptance, Some(&wait), ctx.visible_tiles()).ok()?;
    let reach_legal =
        future_reach_legal(ctx, Some(is_menzen(melds)), post_kan_remaining_tiles(ctx));

    Some(evaluate_tenpai_offense_with_reach_legality(ctx, &wait, reach_legal, Some(&hands)).offense)
}

#[cfg(test)]
mod tests;
