//! Chi / Pon の鳴き判断 policy 層。
//!
//! 既存の production 対象は
//!
//! ```text
//! 現在1向聴 → Chi / Pon → 打牌 → テンパイ
//! ```
//!
//! で、これを満たさなかった候補のうち
//!
//! ```text
//! 現在1向聴 → Chi / Pon → 打牌 → 1向聴
//! 現在2向聴 → Chi / Pon → 打牌 → 1向聴
//! 現在3向聴 → Chi / Pon → 打牌 → 2向聴
//! ```
//!
//! だけを Pass と同じ self-tsumo continuation 尺度で比較する。Chi と Pon は同じ評価 path を
//! 通り、鳴き種別ごとの専用 rule や牌種による gating は持たない。
//!
//! # source of truth
//!
//! この層は「どの条件で鳴くか」だけを持ち、判断材料は既存 layer の結果をそのまま使う。
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | 面子の形の検証 | [`Meld::shape`] |
//! | 喰い替え禁止牌 | [`forbidden_discards_after_call`] |
//! | 副露込みの向聴数 | [`calculate_shanten_with_fixed_melds`] |
//! | 鳴き後の打牌選択 | [`select_discard_action_with_evaluation`] |
//! | 2向聴 / 3向聴 Call の打牌候補 | [`post_call_discard_evaluations`] |
//! | 2向聴 Call の鳴き後打牌比較 | [`select_best_iishanten_post_call_discard`] |
//! | 3向聴 Call の鳴き後打牌比較 | [`select_best_two_shanten_post_call_discard`] |
//! | Pass の継続評価 | [`awaiting_draw_expected_self_tsumo_value`] |
//! | 2向聴 Pass の継続評価 | [`awaiting_draw_two_shanten_expected_self_tsumo_value`] |
//! | 3向聴 Pass の継続評価 | [`awaiting_draw_three_shanten_progress_only_self_tsumo_value`] |
//! | 待ちと残枚数 | [`DiscardEvaluation::acceptance_after_discard`] / [`TenpaiWaitAvailability`] |
//! | ロン可否 | [`TenpaiWaitAvailability::can_ron`] |
//! | 役の有無 | [`evaluate_tenpai_hand_value`] |
//! | threat に対する押し引き | [`decide_push_pull`] |
//!
//! 向聴・受け入れ・待ち・フリテン・役・点数をこの層で計算し直さない。
//!
//! # 同じ鳴きになる合法 action
//!
//! 同じ牌種の物理牌を複数持つ手牌では、消費する物理牌の組み合わせだけが違う Chi / Pon が複数の
//! 合法 action として並ぶ。鳴き後の判断が読むのは [`CallEvaluationKey`] が示す物理牌 semantics
//! だけなので、key が一致する候補は高コストな鳴き後の打牌評価を1回だけ行い、結果を各 action へ
//! 配る。候補の件数・順序・`action` と、同値時に先頭の合法 action を採る tie-break は変えない。
//!
//! # 鳴き後の打牌
//!
//! 鳴いた直後に切れない牌 (喰い替え) は戦術ではなく合法手の制約なので、鳴き後の仮想合法
//! `Dahai` から先に取り除き、残った候補を通常打牌の production selector へ渡す。したがって
//! 「喰い替え禁止牌を切ればテンパイする」を理由に鳴くことはなく、鳴き専用の比較順も持たない。
//!
//! # 成立条件
//!
//! ```text
//! 他家リーチなし
//! AND 現在の effective shanten == 1
//! AND 合法な Chi または Pon
//! AND 鳴いた後の最良打牌で effective shanten == 0
//! AND can_ron == Some(true)
//! AND 生きた待ちの残枚数合計 >= CALL_MIN_LIVE_WAIT_REMAINING
//! AND 残枚数 > 0 の全ての和了牌 variant に役がある
//! AND 鳴き後の最良打牌を既存 Push/Pull policy が Push と判定する
//! ```
//!
//! 現在2向聴の候補は鳴き後1向聴の比較だけを条件にする。
//!
//! ```text
//! 他家リーチなし
//! AND 現在の effective shanten == 2
//! AND 合法な Chi または Pon
//! AND 鳴いた後の最良打牌で effective shanten == 1
//! AND 反応元の席が分かる
//! AND Call 後1向聴の ExpectedSelfTsumoValue > Pass の2向聴 ExpectedSelfTsumoValue
//! AND 鳴き後の選択打牌を既存 Push/Pull policy が Push と判定する
//! ```
//!
//! 現在3向聴の候補は鳴き後2向聴の比較だけを条件にする。
//!
//! ```text
//! 他家リーチなし
//! AND 現在の effective shanten == 3
//! AND 合法な Chi または Pon
//! AND 鳴いた後の最良打牌で effective shanten == 2
//! AND 反応元の席が分かる
//! AND Call 後2向聴の Progress-only value > Pass の3向聴 Progress-only value
//! AND 鳴き後の選択打牌を既存 Push/Pull policy が Push と判定する
//! ```
//!
//! 鳴き後も1向聴のままの候補も、Call / Pass 比較の後に同じ Push/Pull 条件を持つ。どの候補でも
//! Push/Pull は Call / Pass 比較 (速度優先 policy を含む) で成立した後にだけ判定する
//! ([鳴き後の押し引き](#鳴き後の押し引き))。
//!
//! 現在2向聴の候補だけ、値比較で Pass になる結論を速度優先 policy が上書きする。
//!
//! ```text
//! 現在の effective shanten == 2
//! AND 鳴き後の最良打牌で effective shanten == 1
//! AND 鳴き後1向聴の continuation が評価した全テンパイの確定翻数が
//!     CALL_TWO_SHANTEN_SPEED_MIN_HAN 以上
//! AND Call 後の自分の残り自摸機会 >= CALL_TWO_SHANTEN_SPEED_MIN_DRAWS
//! AND 値比較の結論が Pass (`PassSelfTsumoNotLower`)
//! ```
//!
//! 現在3向聴の候補も同じ形の速度優先 policy を持ち、閾値だけが違う。
//!
//! ```text
//! 現在の effective shanten == 3
//! AND 鳴き後の最良打牌で effective shanten == 2
//! AND 鳴き後2向聴の Progress-only 評価が scoring した全テンパイの確定翻数が
//!     CALL_THREE_SHANTEN_SPEED_MIN_HAN 以上
//! AND Call 後の自分の残り自摸機会 >= CALL_THREE_SHANTEN_SPEED_MIN_DRAWS
//! AND 値比較の結論が Pass (`PassSelfTsumoNotLower`)
//! ```
//!
//! 即テンパイ候補は従来どおり最優先する。それが無い場合だけ Call 後1向聴の ExpectedSelfTsumoValue
//! と Pass を同じ流局 horizon で比較し、Call が厳密に高い場合だけ鳴く。同値・unknown は鳴かない。
//! 現在2向聴から1向聴になる鳴きも同じ比較に従い、上の速度優先 policy を満たす場合だけ同値・
//! Call が低い結論を上書きする。他家にリーチ者がいる局面の鳴きは押し引きへ通さず、リーチ者が
//! いる局面をこの速度優先 policy で上書きすることもない。
//!
//! 比較する2つの値は同じ1向聴 continuation の設定で求める。Call 側は鳴いた後の打牌候補比較
//! ([`select_discard_action_with_evaluation`] / [`select_best_iishanten_post_call_discard`]) が、
//! Pass 側は継続評価が、どちらも [`with_production_iishanten_continuation`] と同じ production の
//! 深度・探索内 memo を通る。片側だけ深い評価にして、比較が尺度の違いを拾うことがないように
//! する。
//!
//! # 片和了
//!
//! 役の有無は牌種単位ではなく、和了牌の物理牌 (赤5 / 黒5) ごとの variant 単位で見る。残枚数が
//! 0 の variant は現在ロンできないので判定対象にせず、残枚数 > 0 の variant に1つでも役なしが
//! あれば鳴かない。役の有無を確定できない variant がある場合も、役ありだと推測せず鳴かない。
//!
//! # 1向聴のまま鳴く候補
//!
//! ```text
//! 現在1向聴 → 鳴く → 最良打牌 → 1向聴のまま
//! ```
//!
//! は [`ForwardMetrics::expected_self_tsumo_value`](bot_logic::ForwardMetrics) と同じ Progress /
//! SameShanten 探索・terminal scoring・確率模型で評価する。Pass は架空の現在打牌を作らず、既に
//! action が終わり次の自摸を待つ state 用の共有入口から同じ探索へ入る。
//!
//! raw acceptance と固定面子の役保証は [`CallIishantenAcceptanceDiagnostic`] に観測用として残すが、
//! policy は読まない。diagnostics の有無で action は変わらず、production が使った self-tsumo 値を
//! [`CallIishantenSelfTsumoDiagnostic`] へそのまま保持する。
//!
//! # 2向聴から1向聴になる鳴き
//!
//! 現在2向聴から Chi / Pon 後の最良打牌で1向聴になる候補だけ、Call と Pass の self-tsumo value
//! を比較する。Call は既存の1向聴 post-call selector が返した値、Pass は次の自摸を待つ2向聴
//! state の Full 値を使う。Progress-only は2向聴を維持する枝を含まず、Call 側の完全な1向聴
//! continuation と比較すると Call に有利なため、この比較には使わない。
//!
//! 比較の semantics は1向聴からの鳴きと同じで、Call が厳密に高い場合だけ鳴く。同値・どちらかが
//! 確定できない場合・反応元の席が分からない場合は鳴かない。鳴き後も2向聴のままの候補と Kan は
//! 対象外で、順位条件や守備力による例外も持たない。
//!
//! Pass の2向聴 Full 評価は、鳴き後1向聴になる候補が1件以上ある場合に1回だけ行う。鳴き後も
//! 2向聴のままの候補しかない局面や、そもそも Chi / Pon の合法手が無い局面では評価しない。
//! diagnostics の有無で評価する値も選ぶ action も変わらない。
//!
//! # 速度優先で鳴く高打点の2向聴
//!
//! 序盤で既に打点が確定している手では、門前維持より向聴数を進めたい。そのため
//! `現在2向聴 → 鳴き後1向聴` の候補に限り、値比較が Pass と結論した場合でも
//!
//! ```text
//! 鳴き後1向聴の continuation が評価した全テンパイの確定翻数 >= CALL_TWO_SHANTEN_SPEED_MIN_HAN
//! AND Call 後の自分の残り自摸機会 >= CALL_TWO_SHANTEN_SPEED_MIN_DRAWS
//! ```
//!
//! を満たせば Call する。ExpectedSelfTsumoValue へ係数を掛けるのではなく、この2条件を満たす
//! かどうかだけの明示的な policy にする。上書きするのは値比較の結論
//! ([`CallDecisionReason::PassSelfTsumoNotLower`]) だけで、他家リーチ・喰い替え・向聴数・
//! 反応元不明・値 unknown といった既存の rejection 条件はそのまま残す。
//!
//! | 材料 | source of truth |
//! | --- | --- |
//! | Call 後の残り自摸機会 | [`own_future_draws`] |
//! | continuation が評価したテンパイの確定翻数 | [`continuation_han_verdict`] |
//!
//! 残り自摸機会は巡目や河の枚数から推測せず、鳴き後の打牌選択と self-tsumo continuation が使う
//! 既存の計算をそのまま読む。確定できない局面ではこの policy を適用せず、従来の Call / Pass
//! 比較へ戻す。
//!
//! ## 打点条件が見る範囲
//!
//! 翻数は現在手牌のドラ枚数から推測せず、鳴き後の実際の手牌 state から到達するテンパイを既存
//! prospective scoring で評価した結果だけを読む。ドラ・赤ドラは既存 scoring のとおり含み、鳴いた
//! ことで消えるリーチ・門前清自摸和は production のリーチ判断が選んだ baseline がダマになるので
//! 加算されない。向聴・役・ドラ・点数計算をこの層で持たない。
//!
//! 見る範囲は **Call 側 ExpectedSelfTsumoValue がその候補について評価した terminal テンパイ
//! 全体** で、次の Progress ツモで直接テンパイになる枝だけでなく、SameShanten 手変わりを経由して
//! から到達するテンパイも production の continuation depth に収まる範囲はすべて含む。つまりこの
//! 条件は
//!
//! ```text
//! Call EV が評価した全ての terminal・全ての生きた和了牌 variant で
//! CALL_TWO_SHANTEN_SPEED_MIN_HAN 以上
//! ```
//!
//! であり、値比較に使う Call EV と判定の対象範囲が一致する。判定のためだけに SameShanten
//! downstream を追加探索することはなく、production が探索していない枝は対象にならない。範囲の
//! 中では conservative に畳み、1つでも翻数が足りない / 確定できない terminal があれば policy を
//! 適用しない。畳み方は既存 prospective scoring の集約 ([`continuation_han_verdict`]) が持つ。
//!
//! ## 判定にかかるコスト
//!
//! 翻数の判定は鳴き後1向聴の打牌選択が既に行った前方評価から回収する。枝は
//! [`select_best_iishanten_post_call_discard`] が選択に使った探索結果そのもので、各テンパイの
//! 確定打点も探索中の terminal scoring が求めた値をそのまま読むので、判定のために候補を探索し
//! 直すことも同じテンパイを点数計算し直すこともない。
//!
//! 判定を要求するのは安価な残り自摸機会の条件を満たす局面だけで、満たさない局面では何も足さない。
//! 要求した場合だけ確定打点の下限を集める評価器になる
//! ([`ProductionProspectiveValuator::collecting_han_floor`]) ので、この policy が発動しない局面と
//! 通常打牌には下限の集約コストも載らない。`Reused` 候補は先行候補の結果をそのまま複製するので、
//! 同じ鳴き後 state を2回評価しない。
//!
//! # 3向聴から2向聴になる鳴き
//!
//! 現在3向聴から Chi / Pon 後の最良打牌で2向聴になる候補だけ、Call と Pass の self-tsumo value
//! を比較する。鳴き後も3向聴のままの候補と Kan は対象外で、順位条件や守備力による例外も持た
//! ない。現在3向聴の手牌は副露を2件までしか持てないので、この policy の鳴き後は最大3副露に
//! なる。
//!
//! 尺度は3向聴 production の打牌比較と同じ Progress-only で、Call 側も Pass 側も
//!
//! ```text
//! 3向聴 → Progress → 2向聴 → Progress → 1向聴 → Progress → テンパイ
//! ```
//!
//! だけを追う。この policy のために SameShanten の枝を追加探索しない。
//!
//! | 側 | 起点 | 値 |
//! | --- | --- | --- |
//! | Call | 鳴き後の打牌で到達する2向聴 state | [`select_best_two_shanten_post_call_discard`] |
//! | Pass | 次の自摸を待つ現在の3向聴 state | [`awaiting_draw_three_shanten_progress_only_self_tsumo_value`] |
//!
//! Call 側の鳴き後打牌は、鳴き後の合法打牌のうち打牌後2向聴になる候補を既存の2向聴 Progress
//! 評価と既存 comparator ([`best_two_shanten_progress_discard_among`](bot_logic::best_two_shanten_progress_discard_among))
//! で比べて選ぶ。3向聴 production が3→2の到達先で使うのと同じ入口で、2向聴 Full gate は通ら
//! ない。Pass 側は架空の現在打牌を作らず、現在の13枚を「action 済みで次の自摸を待つ state」と
//! して同じ Progress-only の入口へ渡す。したがって枝集合・確率模型・horizon・1向聴到達後の
//! scope はどちらの側でも一致する。
//!
//! 比較の semantics は1向聴・2向聴からの鳴きと同じで、Call が厳密に高い場合だけ鳴く。同値・
//! どちらかが確定できない場合・反応元の席が分からない場合は鳴かない。
//!
//! # 速度優先で鳴く高打点の3向聴
//!
//! 2向聴と同じ理由で、3向聴でも打点が確定している序盤は向聴数を進めたい。そのため
//! `現在3向聴 → 鳴き後2向聴` の候補に限り、値比較が Pass と結論した場合でも
//!
//! ```text
//! 鳴き後2向聴の Progress-only 評価が scoring した全テンパイの確定翻数が
//!   CALL_THREE_SHANTEN_SPEED_MIN_HAN 以上
//! AND Call 後の自分の残り自摸機会 >= CALL_THREE_SHANTEN_SPEED_MIN_DRAWS
//! ```
//!
//! を満たせば Call する。ExpectedSelfTsumoValue へ係数を掛けるのではなく、この2条件を満たす
//! かどうかだけの明示的な policy にする点も2向聴と同じで、上書きするのは値比較の結論
//! ([`CallDecisionReason::PassSelfTsumoNotLower`]) だけになる。他家リーチ・喰い替え・向聴数・
//! 反応元不明・値 unknown といった既存の rejection 条件はそのまま残す。
//!
//! ## 打点条件が見る範囲
//!
//! 翻数は現在手牌のドラ枚数から推測せず、鳴き後の実際の手牌 state を既存 prospective scoring で
//! 評価した結果だけを読む。見る範囲は **最終的に選択された鳴き後打牌の2向聴
//! Progress-only continuation が実際に評価した terminal 全体** である。
//! 候補の Progress 評価ごとに下限を回収し、既存 comparator が選んだ候補の verdict を返す。
//! 未選択候補や後続 metric の terminal は含めない。
//!
//! ## 判定にかかるコスト
//!
//! 判定は鳴き後2向聴の打牌比較が既に行った terminal scoring から回収する。各テンパイの確定
//! 打点は探索中の terminal scoring が求めた値をそのまま読むので、判定のために候補を探索し直す
//! ことも同じテンパイを点数計算し直すこともない。判定を要求するのは安価な残り自摸機会の条件を
//! 満たす局面だけで、満たさない局面では確定打点の下限を畳む処理そのものを通らない。
//!
//! # 鳴き後の押し引き
//!
//! 非テンパイ Call (`1向聴 → 1向聴` / `2向聴 → 1向聴` / `3向聴 → 2向聴`) は、Call / Pass 比較で
//! 成立した後に、鳴き後の打牌選択が選んだ打牌を既存 Push/Pull ([`decide_push_pull`]) へ通す。
//! `Push` 以外なら [`CallDecisionReason::PostCallNotPush`] で鳴かない。鳴いた直後に同じ production
//! の押し引きが降りる手を、攻撃価値の比較だけで鳴かないようにするためで、即テンパイ Call の
//! 成立条件と同じ考え方になる。現在の Push/Pull は `Neutral` を返さない。
//!
//! 鳴き専用の threat 分類・守備 heuristic・safety 判定・threshold・offense 評価は持たない。鳴く
//! 前後で他家の情報は増えないので threat facts は鳴く前の局面から1回だけ作って共有し、鳴いて
//! 変わる自分側の材料だけを鳴き後 state から渡す。
//!
//! | 材料 | 出どころ |
//! | --- | --- |
//! | threat facts | 鳴く前の局面の [`player_threat_facts_from_context`] (他家の facts は鳴き後と同じ) |
//! | 選択打牌と向聴数 | その候補の鳴き後打牌選択が選んだ [`DiscardEvaluation`] |
//! | 1向聴の前方集計値 | 同じ選択が既に求めた [`ForwardMetrics`] (1→1 は [`select_discard_action_with_evaluation`]、2→1 は [`select_best_iishanten_post_call_discard`]) |
//! | 選択打牌の hard-safe | 鳴き後の合法 Dahai を渡した既存入力構築 |
//!
//! 入力の構築は通常の `act()` が打牌選択の結果を押し引きへ渡すのと同じ
//! [`push_pull_inputs_from_threat_facts`] を通す。選択の計算済み値を持たない呼び出し向けの入口
//! (`push_pull_inputs_from_context_with_evaluation`) は使わないので、configured horizon の1向聴前方
//! 集計値はそのまま再利用する。1向聴 Push/Fold の threshold と比較する ExpectedSelfTsumoValue だけは
//! `UNTIL_RYUKYOKU` の尺度で、configured horizon が `UNTIL_RYUKYOKU` なら選択の計算済み値を使い、
//! それ以外では選択済みの1打牌だけを `UNTIL_RYUKYOKU` で評価し直す。全候補は再探索しない。
//! 3向聴からの鳴きは鳴き後2向聴で、Push/Pull が読むのは向聴数と選択打牌の hard-safe だけなので
//! 前方集計値を渡さない。
//!
//! Call / Pass 比較で落ちた候補には Push/Pull を評価せず、理由も上書きしない。比較で成立して
//! いたことは [`CallCandidateDiagnostic::call_pass_eligible_reason`]、鳴き後の判定は
//! [`CallCandidateDiagnostic::post_call_push_pull`] に残る。
//!
//! # Call と Pass の重ね合わせ
//!
//! Call 側の鳴き後打牌選択と Pass 側の継続評価は、入力も探索基盤も共有しない。Call 側は
//! `FixedMeldCount` が1つ増えた鳴き後の手牌 state を評価し、Pass 側は現在の13枚 state を
//! 評価するので、base 評価 memo も探索内 state memo も terminal tenpai memo も key が重ならない。
//! したがって両者はどちらが先でも値が変わらず、別 thread で重ねても結果は bit-exact に
//! 一致する。
//!
//! ```text
//! 安価な事前判定 (向聴・喰い替え・鳴き後の最小向聴数)
//! ↓
//! 鳴き後の最良打牌が1向聴になる候補があり、反応元の席も分かる
//! ↓
//! Call 候補の deep 評価 group ─┐
//!                             ├─ 別 thread
//! Pass の継続評価            ─┘
//! ↓
//! join → 既存の Call > Pass 比較
//! ```
//!
//! 重ねる対象は現在1向聴の Pass 継続評価・現在2向聴の Pass Full 評価・現在3向聴の Pass
//! Progress-only 評価で、どれを評価するかは現在の向聴数が決める。現在の向聴数は手牌と副露数
//! だけで決まり候補ごとに違わないので、1回の鳴き判断で重ねる Pass は必ずこのうち1つになる。
//! 重ねるのは「Pass を1回評価する」という既存条件が安価な事前判定だけで確定する局面に限る。
//! 即テンパイだけの局面や Call policy の前段で落ちる局面や向聴数が進む候補が無い局面へ、
//! 高コストな Pass 継続評価を足すことはない。判定材料は既存の1手評価
//! ([`post_call_discard_evaluations`]) が持つ鳴き後の最小向聴数で、比較順の先頭が向聴数
//! なので本番の鳴き後打牌選択が選ぶ候補の向聴数もこの値になる。
//!
//! 並列度の解決は既存の [`available_parallelism`] をそのまま使い、この層で新しい parallelism
//! policy を持たない。並列度 1 の runtime では必ず従来の逐次経路へ落ちる。Call 側の候補単位
//! 並列評価 ([`crate::discard_selection`]) はそのままで、その外側に足すのは Pass の1 thread
//! だけになる。

use std::time::{Duration, Instant};

use bot_logic::{
    DiscardEvaluation, FixedMeldCount, ForwardMetrics, HandValueError, HandValueOutcome, Meld,
    MeldKind, OwnDiscards, TenpaiWaitAvailability, TileCounts, TileId, TileType,
    awaiting_draw_expected_self_tsumo_value,
    awaiting_draw_three_shanten_progress_only_self_tsumo_value,
    awaiting_draw_two_shanten_expected_self_tsumo_value,
    awaiting_draw_two_shanten_progress_self_tsumo_value, best_discard_selection_index,
    calculate_acceptance_with_fixed_melds_and_visible_tiles, calculate_shanten_with_fixed_melds,
    discard_tenpai_wait_availability, evaluate_tenpai_hand_value, fixed_melds_guarantee_yaku,
    split_discarded_tile, tenpai_completed_hands,
};

use crate::action::LegalAction;
use crate::context::GameContext;
use crate::damaten_value::damaten_baseline_context;
use crate::decision_timing::{CallCandidateElapsed, CallCandidateTimer, CallDecisionTimer};
use crate::discard_selection::{
    DiscardActionSelection, LookaheadDiagnosticScope, available_parallelism,
    lookahead_inputs_with_own_future_draws, own_future_draws, post_call_discard_evaluations,
    select_best_iishanten_post_call_discard, select_best_two_shanten_post_call_discard,
    select_discard_action_with_evaluation, with_production_iishanten_continuation,
};
use crate::kuikae::forbidden_discards_after_call;
use crate::prospective_value::{ProductionProspectiveValuator, ProspectiveHanVerdict};
use crate::push_pull::{
    PushPullDecision, PushPullMode, decide_push_pull, push_pull_inputs_from_selected_tenpai,
    push_pull_inputs_from_threat_facts,
};
use crate::threat::{PlayerThreatFacts, player_threat_facts_from_context};

/// 鳴きを検討する現在の向聴数。
pub const CALL_CURRENT_SHANTEN: i8 = 1;

/// 鳴き後の打牌でテンパイと判断する向聴数。
pub const CALL_TENPAI_SHANTEN: i8 = 0;

/// 鳴くために必要な、鳴き後テンパイの生きた待ちの残枚数合計 [枚]。inclusive。
pub const CALL_MIN_LIVE_WAIT_REMAINING: u8 = 3;

// Chi / Pon の consumed 枚数。
const CALL_CONSUMED_TILE_COUNT: usize = 2;

/// 鳴き後1向聴の比較だけで判断する、もう1つの現在向聴数。
pub const CALL_TWO_SHANTEN_SHANTEN: i8 = 2;

/// `現在2向聴 → Call → 打牌 → 1向聴` を速度優先で鳴くために必要な、鳴き後1向聴の continuation が
/// 評価したテンパイの確定翻数 [翻]。inclusive。
pub const CALL_TWO_SHANTEN_SPEED_MIN_HAN: u8 = 3;

/// 同じ速度優先 policy が必要とする、Call 後に自分へ残っている自摸機会 [回]。inclusive。
pub const CALL_TWO_SHANTEN_SPEED_MIN_DRAWS: u32 = 10;

/// 鳴き後2向聴の比較だけで判断する、もう1つの現在向聴数。
pub const CALL_THREE_SHANTEN_SHANTEN: i8 = 3;

/// `現在3向聴 → Call → 打牌 → 2向聴` を速度優先で鳴くために必要な、鳴き後2向聴の
/// Progress-only 評価が scoring したテンパイの確定翻数 [翻]。inclusive。
pub const CALL_THREE_SHANTEN_SPEED_MIN_HAN: u8 = 4;

/// 同じ速度優先 policy が必要とする、Call 後に自分へ残っている自摸機会 [回]。inclusive。
pub const CALL_THREE_SHANTEN_SPEED_MIN_DRAWS: u32 = 12;

/// 評価対象の鳴き種別。今回の対象は Chi と Pon だけで、Kan は含まない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Chi,
    Pon,
}

impl CallKind {
    /// 対応する既存の副露種別。
    pub fn meld_kind(self) -> MeldKind {
        match self {
            Self::Chi => MeldKind::Chi,
            Self::Pon => MeldKind::Pon,
        }
    }
}

/// 鳴きを採用した / しなかった理由。
///
/// `EligibleTenpai` / `EligibleIishantenSelfTsumo` / `EligibleTwoShantenSelfTsumo` 以外はすべて
/// 「今回は鳴かない」理由であり、最初に落ちた条件を1つだけ表す。判定順は
/// [`CallCandidateDiagnostic`] のフィールドが埋まる順と一致する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDecisionReason {
    /// 全条件を満たし、鳴き後に生きた待ちのテンパイになる。
    EligibleTenpai,
    /// 鳴き後も1向聴だが、同じ horizon の ExpectedSelfTsumoValue が Pass より厳密に高い。
    EligibleIishantenSelfTsumo,
    /// 現在2向聴から鳴き後1向聴になり、ExpectedSelfTsumoValue が Pass より厳密に高い。
    EligibleTwoShantenSelfTsumo,
    /// 現在2向聴から鳴き後1向聴になり、ExpectedSelfTsumoValue は Pass 以下だが、鳴き後1向聴の
    /// continuation が評価したテンパイの確定打点と残り自摸機会が速度優先 policy を満たす。
    EligibleTwoShantenSpeed,
    /// 現在3向聴から鳴き後2向聴になり、Progress-only の self-tsumo 値が Pass より厳密に高い。
    EligibleThreeShantenSelfTsumo,
    /// 現在3向聴から鳴き後2向聴になり、Progress-only の self-tsumo 値は Pass 以下だが、鳴き後
    /// 2向聴の評価が scoring したテンパイの確定打点と残り自摸機会が速度優先 policy を満たす。
    EligibleThreeShantenSpeed,
    /// 他家にリーチ者がいる。今回の鳴きは押し引きへ通さない。
    OpponentReached,
    /// reaction context に `drawn_tile` があり局面として不整合。14枚扱いで判断しない。
    UnexpectedDrawnTile,
    /// consumed が2枚でない・手牌に無い・物理牌が重複している・面子の形にならないなどで
    /// 鳴き後の手牌を組み立てられない。
    InvalidConsumed,
    /// 自分の副露済み面子数が不明。0副露と推測しない。
    FixedMeldCountUnknown,
    /// 鳴き後の副露済み面子数が上限を超える。
    FixedMeldCountOverflow,
    /// 現在の effective shanten が鳴きの検討対象 (1向聴 / 2向聴) ではない。
    CurrentShantenNotCallable,
    /// 現在2向聴で、鳴き後の最良打牌でも1向聴にならない。
    PostCallNotIishanten,
    /// 現在3向聴で、鳴き後の最良打牌でも2向聴にならない。
    PostCallNotTwoShanten,
    /// 鳴き後の手牌に、喰い替え禁止牌を除いた合法な打牌候補が無い。
    NoPostCallDiscard,
    /// 鳴き後の最良打牌でもテンパイにならない。
    PostCallNotTenpai,
    /// 鳴き後1向聴と Pass の ExpectedSelfTsumoValue のどちらかを確定できない。
    IishantenSelfTsumoUnknown,
    /// Pass の正確な horizon に必要な reaction 元 player が不明。
    ReactionSourceUnknown,
    /// Pass の ExpectedSelfTsumoValue が Call 以上。同値でも鳴かない。
    PassSelfTsumoNotLower,
    /// 鳴き後はテンパイだが、待ち牌がすべて見えている。
    NoLiveAcceptance,
    /// 生きた待ちはあるが、残枚数合計が [`CALL_MIN_LIVE_WAIT_REMAINING`] 未満。
    TooFewLiveWaits,
    /// 鳴き後テンパイでロンできない。フリテンとロン可否 unknown のどちらもここに含む。
    CannotRon,
    /// 残枚数 > 0 の和了牌 variant に役なしがある。片和了は許可しない。
    YakuMissing,
    /// 残枚数 > 0 の和了牌 variant に、役の有無を確定できないものがある。
    HandValueUnknown,
    /// 鳴き後の最良打牌を既存 Push/Pull policy が Push と判定しない。
    ///
    /// 即テンパイ Call では成立条件の最後に、非テンパイ Call では Call / Pass 比較で成立した後に
    /// 判定する。後者で Call / Pass 比較が成立していたことは
    /// [`CallCandidateDiagnostic::call_pass_eligible_reason`] で確認できる。
    PostCallNotPush,
    /// 鳴き後の仮想局面を既存の通常打牌・Push/Pull policy へ渡せない。
    PostCallEvaluationUnavailable,
}

/// `Call -> 打牌 -> 1向聴` と Pass の self-tsumo continuation 比較結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallIishantenComparison {
    CallHigher,
    PassNotLower,
    Unknown,
}

/// production が使用した1向聴 Call / Pass の ExpectedSelfTsumoValue。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallIishantenSelfTsumoDiagnostic {
    pub reaction_source_player: Option<u8>,
    pub pass_expected_self_tsumo_value: Option<u64>,
    pub call_expected_self_tsumo_value: Option<u64>,
    pub comparison: CallIishantenComparison,
}

/// 2向聴 Pass 側で使った既存 self-tsumo 評価。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallTwoShantenPassEvaluation {
    /// Progress と、一度だけの SameShanten → Progress を含む Full 値。
    Full,
}

/// production が使用した `現在2向聴 → Call → 打牌 → 1向聴` の速度優先 policy の判断材料。
///
/// 値はどちらも既存 layer の結果そのままで、残り自摸機会は打牌選択と同じ
/// [`own_future_draws`]、翻数は鳴き後1向聴の打牌選択が使った前方評価から回収した
/// [`continuation_han_verdict`] の結論を使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallTwoShantenSpeedDiagnostic {
    /// Call 後に自分へ残っている自摸機会 [回]。山の残枚数が unknown な局面では `None`。
    pub own_future_draws: Option<u32>,
    /// 鳴き後の最良打牌の continuation が評価した全テンパイの確定翻数が
    /// [`CALL_TWO_SHANTEN_SPEED_MIN_HAN`] 以上か。SameShanten を経由してから到達するテンパイも
    /// 含み、対象は Call 側 ExpectedSelfTsumoValue が集計した terminal 集合と一致する。
    ///
    /// 残り自摸機会の条件で先に落ちた候補では判定を要求しないので `None`。
    pub han: Option<ProspectiveHanVerdict>,
    /// 通常の Call / Pass 比較では Pass になる候補を、この policy が Call へ変えたか。
    pub overrides_pass: bool,
}

impl CallTwoShantenSpeedDiagnostic {
    /// 速度優先 policy の条件をすべて満たすか。
    ///
    /// 残り自摸機会を確定できない局面と、翻数を確定できない候補はどちらも満たさない。
    pub fn is_satisfied(&self) -> bool {
        self.own_future_draws
            .is_some_and(|draws| draws >= CALL_TWO_SHANTEN_SPEED_MIN_DRAWS)
            && self.han == Some(ProspectiveHanVerdict::AtLeast)
    }
}

/// production が使用した `現在2向聴 → Call → 打牌 → 1向聴` と Pass の比較。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallTwoShantenSelfTsumoDiagnostic {
    pub reaction_source_player: Option<u8>,
    pub pass_evaluation: CallTwoShantenPassEvaluation,
    pub pass_expected_self_tsumo_value: Option<u64>,
    pub call_expected_self_tsumo_value: Option<u64>,
    pub comparison: CallIishantenComparison,
    /// 同じ候補の速度優先 policy の判断材料。`comparison` が `PassNotLower` の候補だけ、この
    /// policy が成立すれば Call へ変わる。
    pub speed: CallTwoShantenSpeedDiagnostic,
}

/// 3向聴 Pass 側で使った既存 self-tsumo 評価。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallThreeShantenPassEvaluation {
    /// 3→2、2→1、1向聴到達後のどこでも Progress だけを追う値。
    ProgressOnly,
}

/// production が使用した `現在3向聴 → Call → 打牌 → 2向聴` の速度優先 policy の判断材料。
///
/// 値はどちらも既存 layer の結果そのままで、残り自摸機会は打牌選択と同じ
/// [`own_future_draws`]、翻数は鳴き後2向聴の Progress-only 評価が行った terminal scoring から
/// 回収した [`scored_han_verdict`](crate::prospective_value::scored_han_verdict) の結論を使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallThreeShantenSpeedDiagnostic {
    /// Call 後に自分へ残っている自摸機会 [回]。山の残枚数が unknown な局面では `None`。
    pub own_future_draws: Option<u32>,
    /// 鳴き後2向聴の Progress-only 評価が scoring した全テンパイの確定翻数が
    /// [`CALL_THREE_SHANTEN_SPEED_MIN_HAN`] 以上か。
    ///
    /// 残り自摸機会の条件で先に落ちた候補では判定を要求しないので `None`。
    pub han: Option<ProspectiveHanVerdict>,
    /// 通常の Call / Pass 比較では Pass になる候補を、この policy が Call へ変えたか。
    pub overrides_pass: bool,
}

impl CallThreeShantenSpeedDiagnostic {
    /// 速度優先 policy の条件をすべて満たすか。
    ///
    /// 残り自摸機会を確定できない局面と、翻数を確定できない候補はどちらも満たさない。
    pub fn is_satisfied(&self) -> bool {
        self.own_future_draws
            .is_some_and(|draws| draws >= CALL_THREE_SHANTEN_SPEED_MIN_DRAWS)
            && self.han == Some(ProspectiveHanVerdict::AtLeast)
    }
}

/// production が使用した `現在3向聴 → Call → 打牌 → 2向聴` と Pass の比較。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallThreeShantenSelfTsumoDiagnostic {
    pub reaction_source_player: Option<u8>,
    pub pass_evaluation: CallThreeShantenPassEvaluation,
    pub pass_expected_self_tsumo_value: Option<u64>,
    pub call_expected_self_tsumo_value: Option<u64>,
    pub comparison: CallIishantenComparison,
    /// 同じ候補の速度優先 policy の判断材料。`comparison` が `PassNotLower` の候補だけ、この
    /// policy が成立すれば Call へ変わる。
    pub speed: CallThreeShantenSpeedDiagnostic,
}

/// 和了牌の物理牌1つ分の役の有無。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallWaitYaku {
    /// 既存 [`HandValueOutcome::Known`] として役が確定した。
    Present,
    /// 既存 [`HandValueOutcome::NoCandidate`] で役が無いと確定した。
    Absent,
    /// 役の有無を確定できない。点数計算の入力不足や裏ドラ未確定の場合。
    Unknown,
}

/// 鳴き後テンパイの和了牌の物理牌1つ分の役診断。
///
/// 赤5と黒5は別の variant として並ぶ。`remaining` は既存受け入れの残枚数を赤 / 黒へ分けた値で、
/// ここで数え直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallWaitYakuDiagnostic {
    pub winning_tile: TileId,
    /// この variant の残枚数。
    pub remaining: u8,
    pub yaku: CallWaitYaku,
}

impl CallWaitYakuDiagnostic {
    /// 現在まだロンできる variant か。`remaining == 0` の variant は片和了判定の対象外。
    pub fn is_live(&self) -> bool {
        self.remaining > 0
    }

    pub fn is_red(&self) -> bool {
        self.winning_tile.is_red()
    }
}

/// 1向聴のまま鳴く候補についての、鳴かない場合と鳴いた場合の受け入れ比較。
///
/// production の鳴き判断はこの値を読まない。将来
/// 「1向聴 → 鳴いて1向聴だが受け入れが大きく改善する」を policy へ入れるかどうかを実戦局面で
/// 観測するためだけに持つ。閾値も比も置かない。
///
/// | 値 | source of truth |
/// | --- | --- |
/// | 鳴かない場合の受け入れ | [`calculate_acceptance_with_fixed_melds_and_visible_tiles`] |
/// | 鳴いた後の向聴と受け入れ | [`CallCandidateDiagnostic::post_call_discard`] |
/// | 固定面子だけの役保証 | [`fixed_melds_guarantee_yaku`] |
///
/// どれも production 評価が既に求めた値か既存 calculator の結果そのもので、診断のために向聴・
/// 受け入れ・役を計算し直さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallIishantenAcceptanceDiagnostic {
    /// 鳴かずに現在の手牌のまま進めた場合の受け入れ残枚数 [枚]。
    pub pass_acceptance_remaining: u8,
    /// 同じく受け入れ牌種数。
    pub pass_acceptance_type_count: usize,
    /// 鳴き後の最良打牌の向聴数。この診断を持つ候補では常に [`CALL_CURRENT_SHANTEN`]。
    pub post_call_shanten: i8,
    /// 鳴き後の最良打牌の受け入れ残枚数 [枚]。
    pub post_call_acceptance_remaining: u8,
    /// 同じく受け入れ牌種数。
    pub post_call_acceptance_type_count: usize,
    /// 既存副露 + 今回の Chi / Pon の固定面子だけで、将来の完成形に役が保証されるか。
    ///
    /// 場風・自風が不明な場合は既存 semantics のまま `false`。役ありだと推測しない。
    pub fixed_melds_guarantee_yaku: bool,
}

impl CallIishantenAcceptanceDiagnostic {
    /// 鳴いた場合 - 鳴かない場合の受け入れ残枚数差 [枚]。符号付き。
    pub fn acceptance_remaining_delta(&self) -> i16 {
        i16::from(self.post_call_acceptance_remaining) - i16::from(self.pass_acceptance_remaining)
    }

    /// 鳴いた場合 - 鳴かない場合の受け入れ牌種数差。符号付き。
    pub fn acceptance_type_delta(&self) -> isize {
        self.post_call_acceptance_type_count as isize - self.pass_acceptance_type_count as isize
    }
}

/// 合法な `LegalAction::Chi` / `LegalAction::Pon` 1件ごとの判断内訳。
///
/// 各フィールドは判定が実際にそこまで進んだ場合だけ `Some` になり、進まなかった判定は推測せず
/// `None` のままにする。production が使う値も observation-only の値も既存 selector / helper の
/// 結果そのもので、診断専用の向聴・受け入れ・待ち・役・点数計算は持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCandidateDiagnostic {
    pub action: LegalAction,
    pub kind: CallKind,
    pub current_fixed_meld_count: Option<FixedMeldCount>,
    /// `calculate_shanten_with_fixed_melds()` で求めた現在の effective shanten。
    pub current_shanten: Option<i8>,
    pub post_call_fixed_meld_count: Option<FixedMeldCount>,
    /// 鳴いた直後に切れない牌種。打牌候補を評価しなかった場合は `None`。
    ///
    /// 1向聴 / 2向聴どちらの鳴き後打牌選択も、実際に除外へ使った値そのもので、診断表示の
    /// ために求め直さない。
    pub post_call_forbidden_discards: Option<Vec<TileType>>,
    /// 喰い替え禁止牌を除いた合法な打牌候補の中の最良打牌評価。
    pub post_call_discard: Option<DiscardEvaluation>,
    /// 鳴き後の打牌でテンパイになる場合の待ちとロン可否。
    pub post_call_wait: Option<TenpaiWaitAvailability>,
    /// 鳴き後テンパイの和了牌の物理牌ごとの役診断。役を評価しなかった場合は `None`。
    pub post_call_wait_yaku: Option<Vec<CallWaitYakuDiagnostic>>,
    /// 既存 Push/Pull policy による鳴き後の最良打牌の判定。そこまで評価しなかった場合は `None`。
    ///
    /// 即テンパイ Call では成立条件を満たした候補、非テンパイ Call では Call / Pass 比較で成立
    /// した候補だけが持つ。比較で落ちた候補は評価しないので `None`。
    pub post_call_push_pull: Option<PushPullDecision>,
    /// 鳴いても1向聴のままの候補についてだけ求める観測用の受け入れ比較。対象外の候補と、
    /// そこまで評価が進まなかった候補では `None`。
    pub iishanten_acceptance: Option<CallIishantenAcceptanceDiagnostic>,
    /// 鳴き後も1向聴の候補に対して production が実際に使った Call / Pass 比較。
    pub iishanten_self_tsumo: Option<CallIishantenSelfTsumoDiagnostic>,
    /// 現在2向聴から鳴き後の最良打牌で1向聴になる候補に対して production が実際に使った
    /// Call / Pass 比較。
    pub two_shanten_self_tsumo: Option<CallTwoShantenSelfTsumoDiagnostic>,
    /// 現在3向聴から鳴き後の最良打牌で2向聴になる候補に対して production が実際に使った
    /// Call / Pass 比較。
    pub three_shanten_self_tsumo: Option<CallThreeShantenSelfTsumoDiagnostic>,
    pub eligible: bool,
    pub selected: bool,
    pub reason: CallDecisionReason,
}

impl CallCandidateDiagnostic {
    pub fn post_call_shanten(&self) -> Option<i8> {
        self.post_call_discard
            .as_ref()
            .map(DiscardEvaluation::min_shanten_after_discard)
    }

    pub fn post_call_acceptance_total_remaining(&self) -> Option<u8> {
        self.post_call_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_total_remaining)
    }

    pub fn post_call_acceptance_type_count(&self) -> Option<usize> {
        self.post_call_discard
            .as_ref()
            .map(DiscardEvaluation::acceptance_type_count)
    }

    /// 鳴き後テンパイでツモ和了できる待ちの残枚数合計。テンパイにならない場合は `None`。
    pub fn live_wait_remaining(&self) -> Option<u8> {
        self.post_call_wait
            .as_ref()
            .map(|wait| wait.tsumo_remaining)
    }

    /// 鳴き後テンパイの総合ロン可否。テンパイにならない場合と判断できない場合は `None`。
    pub fn can_ron(&self) -> Option<bool> {
        self.post_call_wait
            .as_ref()
            .and_then(TenpaiWaitAvailability::can_ron)
    }

    /// 非テンパイ Call の Call / Pass 比較が成立させた理由。比較が成立しなかった候補と、
    /// 比較の対象外の候補では `None`。
    ///
    /// production が使った比較結果 (`comparison` と速度優先 policy の `overrides_pass`) を読む
    /// だけで、比較をやり直さない。鳴き後 Push/Pull で [`CallDecisionReason::PostCallNotPush`] に
    /// なった候補でも、比較段階で成立していたことをここで確認できる。
    pub fn call_pass_eligible_reason(&self) -> Option<CallDecisionReason> {
        if let Some(diagnostic) = self.iishanten_self_tsumo {
            return (diagnostic.comparison == CallIishantenComparison::CallHigher)
                .then_some(CallDecisionReason::EligibleIishantenSelfTsumo);
        }
        if let Some(diagnostic) = self.two_shanten_self_tsumo {
            return match (diagnostic.comparison, diagnostic.speed.overrides_pass) {
                (CallIishantenComparison::CallHigher, _) => {
                    Some(CallDecisionReason::EligibleTwoShantenSelfTsumo)
                }
                (_, true) => Some(CallDecisionReason::EligibleTwoShantenSpeed),
                _ => None,
            };
        }
        let diagnostic = self.three_shanten_self_tsumo?;
        match (diagnostic.comparison, diagnostic.speed.overrides_pass) {
            (CallIishantenComparison::CallHigher, _) => {
                Some(CallDecisionReason::EligibleThreeShantenSelfTsumo)
            }
            (_, true) => Some(CallDecisionReason::EligibleThreeShantenSpeed),
            _ => None,
        }
    }

    /// 残枚数 > 0 の和了牌 variant すべてで役ありを確定できたか。役を評価しなかった場合は
    /// `None`。
    pub fn live_waits_have_yaku(&self) -> Option<bool> {
        self.post_call_wait_yaku.as_ref().map(|waits| {
            waits
                .iter()
                .filter(|wait| wait.is_live())
                .all(|wait| wait.yaku == CallWaitYaku::Present)
        })
    }
}

/// 鳴き判断の構造化診断。
///
/// `selected` は `ShantenAgent::act()` が実際に採用した鳴きそのもので、診断用の別判断ロジック
/// は持たない。採用が無い場合の `reason` は最初の候補が落ちた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallDecisionDiagnostic {
    pub selected: Option<LegalAction>,
    pub reason: CallDecisionReason,
    pub candidates: Vec<CallCandidateDiagnostic>,
}

// 鳴き判断の本体。act() と構造化診断はこの1本を共有し、診断は結果を載せるだけにする。
//
// 合法な Chi / Pon が1件も無ければ検討自体を行わず None。1件以上ある場合は候補ごとに独立して
// 条件を評価し、成立した候補の中から1件を選ぶ。
//
// `collect_observations` は解析専用の受け入れ比較を集めるかどうかだけを切り替える。判断に使う
// fact の評価と候補の選択は切り替えの影響を受けない。
pub(crate) fn evaluate_call_decision(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    collect_observations: bool,
    timing: &mut CallDecisionTimer,
) -> Option<CallDecisionDiagnostic> {
    evaluate_call_decision_with_order(
        ctx,
        legal_actions,
        collect_observations,
        CallPassEvaluationOrder::PRODUCTION,
        timing,
    )
}

/// Call 側の候補評価と Pass 側の継続評価を並べる順。
///
/// 変わるのは2つの独立した評価を重ねるかどうかだけで、探索の深度も探索内 memo も comparator も
/// 候補の順も tie-break も変わらない。`Sequential` は重ねる前の production そのもので、比較の
/// baseline として残す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallPassEvaluationOrder {
    /// Call 側の候補評価をすべて終えてから Pass 側を評価する。
    Sequential,
    /// Pass 側が必要なことが安価な事前判定で分かる局面では、Call 側と別 thread で重ねる。
    Overlapped,
}

impl CallPassEvaluationOrder {
    /// production の順。
    pub(crate) const PRODUCTION: Self = Self::Overlapped;
}

// 評価順を指定した鳴き判断。production は必ず [`CallPassEvaluationOrder::PRODUCTION`] を通る。
fn evaluate_call_decision_with_order(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    collect_observations: bool,
    order: CallPassEvaluationOrder,
    timing: &mut CallDecisionTimer,
) -> Option<CallDecisionDiagnostic> {
    evaluate_call_decision_with_post_call_states(
        ctx,
        legal_actions,
        collect_observations,
        order,
        timing,
    )
    .map(|(decision, _)| decision)
}

// 鳴き判断の本体。production の結論に加えて、候補ごとに鳴き後の評価へ渡した入力を返す。
//
// 返す入力は判断が実際に使ったものそのままで、観測用に組み立て直さない。production の呼び出しは
// 捨てるだけなので、判断も評価の回数も変わらない。
fn evaluate_call_decision_with_post_call_states(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    collect_observations: bool,
    order: CallPassEvaluationOrder,
    timing: &mut CallDecisionTimer,
) -> Option<(CallDecisionDiagnostic, Vec<PostCallState>)> {
    let mut slots = prepare_call_candidates(ctx, legal_actions, timing);
    if slots.is_empty() {
        return None;
    }

    let pass =
        evaluate_prepared_call_candidates(ctx, &mut slots, collect_observations, order, timing);

    record_call_candidate_timings(&slots, timing);
    let (mut candidates, post_call_states) = into_call_candidates(slots);

    // 重ねて評価した Pass は、その局面の現在向聴数に対応する policy だけが読む。
    let (iishanten_pass, two_shanten_pass, three_shanten_pass) = match pass {
        Some(pass) => match pass.kind {
            PassSelfTsumoContinuationKind::Iishanten => (Some(pass), None, None),
            PassSelfTsumoContinuationKind::TwoShanten => (None, Some(pass), None),
            PassSelfTsumoContinuationKind::ThreeShanten => (None, None, Some(pass)),
        },
        None => (None, None, None),
    };
    apply_iishanten_self_tsumo_policy(ctx, &mut candidates, iishanten_pass, timing);
    apply_two_shanten_self_tsumo_policy(ctx, &mut candidates, two_shanten_pass, timing);
    apply_three_shanten_self_tsumo_policy(ctx, &mut candidates, three_shanten_pass, timing);
    // Call / Pass 比較で成立した非テンパイ Call だけを、鳴き後の既存 Push/Pull で確かめる。
    apply_post_call_push_pull_gate(ctx, &mut candidates, &post_call_states);

    let selected_index = select_eligible_candidate(&candidates);
    if let Some(index) = selected_index {
        candidates[index].selected = true;
    }

    let reason = candidates[selected_index.unwrap_or(0)].reason;
    let selected = selected_index.map(|index| candidates[index].action.clone());

    Some((
        CallDecisionDiagnostic {
            selected,
            reason,
            candidates,
        },
        post_call_states,
    ))
}

// 評価を終えた作業単位を、合法 action の列挙順の候補診断と鳴き後 state へ分ける。
fn into_call_candidates(
    slots: Vec<CallCandidateSlot>,
) -> (Vec<CallCandidateDiagnostic>, Vec<PostCallState>) {
    let mut candidates: Vec<CallCandidateDiagnostic> = Vec::with_capacity(slots.len());
    let mut post_call_states: Vec<PostCallState> = Vec::with_capacity(slots.len());
    for slot in slots {
        let (candidate, post_call_state) = match slot.preparation {
            // 同じ post-call state を作る候補なので、評価結果をそのまま複製して action だけ
            // 元の合法 action に戻す。高コスト評価は行わない。
            CallCandidatePreparation::Reused(source) => {
                let mut candidate = candidates[source].clone();
                candidate.action = slot.candidate.action;
                (candidate, PostCallState::Reused(source))
            }
            preparation => (
                slot.candidate,
                PostCallState::Evaluated {
                    preparation,
                    iishanten_forward_metrics: slot.post_call_iishanten_forward_metrics,
                },
            ),
        };
        candidates.push(candidate);
        post_call_states.push(post_call_state);
    }
    (candidates, post_call_states)
}

/// 鳴き候補1件の評価を、安価な事前判定と高コストな評価に分けて持つ作業単位。
///
/// 高コストな評価へ進む候補が分かってから deep 評価を始めるため、Call 側の deep 評価 group と
/// Pass 側の継続評価を重ねられる。候補の並びも `action` も合法 action の列挙順のままで、
/// 分け方は候補の semantics も選択も変えない。
struct CallCandidateSlot {
    candidate: CallCandidateDiagnostic,
    preparation: CallCandidatePreparation,
    /// この候補が実際に払った実測。事前判定と deep 評価を足し合わせたもので、間に挟まる他候補
    /// の評価も Pass の join 待ちも含まない。
    elapsed: CallCandidateElapsed,
    /// 鳴き後の打牌選択が選んだ1向聴打牌について、その選択が既に求めた前方集計値。
    ///
    /// configured horizon の値で、鳴き後の押し引き ([`apply_post_call_push_pull_gate`]) へそのまま
    /// 渡す。threshold と比較する `UNTIL_RYUKYOKU` の値は configured horizon が `UNTIL_RYUKYOKU` なら
    /// これを再利用し、それ以外では押し引き側が選択済みの1打牌だけを評価し直す。鳴き後の選択打牌が
    /// 1向聴でない候補と、そこまで評価しなかった候補では `None`。
    post_call_iishanten_forward_metrics: Option<ForwardMetrics>,
}

/// 候補1件について、鳴き後の押し引きに渡す材料をどこから読むか。
///
/// 高コストな評価を行った候補は、その評価の入力 (鳴き後の手牌 state と合法打牌の作り方) と、
/// 選択が既に求めた1向聴の前方集計値を持つ。semantic に同一な候補は先行候補の結論をそのまま
/// 使う。
enum PostCallState {
    Evaluated {
        preparation: CallCandidatePreparation,
        iishanten_forward_metrics: Option<ForwardMetrics>,
    },
    Reused(usize),
}

/// 安価な事前判定が確定させた、候補1件の次の一手。
enum CallCandidatePreparation {
    /// 高コストな鳴き後打牌選択へ進む候補。
    PostCall(Box<PostCallInputs>),
    /// 現在2向聴からの鳴きとして、鳴き後の打牌選択へ進む候補。
    TwoShantenCall(Box<TwoShantenCallInputs>),
    /// 現在3向聴からの鳴きとして、鳴き後の打牌選択へ進む候補。
    ThreeShantenCall(Box<ThreeShantenCallInputs>),
    /// 安価な条件だけで理由が確定した候補。高コストな評価は行わない。
    Settled,
    /// semantic に同一な先行候補 (index) の結果をそのまま複製する候補。
    Reused(usize),
}

impl CallCandidatePreparation {
    /// 鳴いた後の最良打牌でも1向聴のままになる候補か。
    ///
    /// 判断材料は既存の1手評価が持つ鳴き後の最小向聴数だけで、深い前方評価は通らない。
    fn stays_iishanten_after_call(&self) -> bool {
        match self {
            Self::PostCall(inputs) => inputs.post_call_min_shanten == Some(CALL_CURRENT_SHANTEN),
            _ => false,
        }
    }

    /// 現在2向聴から鳴いて、最良打牌で1向聴になる候補か。
    ///
    /// 判断材料は1向聴のままの候補と同じ既存の1手評価が持つ鳴き後の最小向聴数だけで、深い
    /// 前方評価は通らない。
    fn reaches_iishanten_after_call(&self) -> bool {
        match self {
            Self::TwoShantenCall(inputs) => {
                inputs.post_call_min_shanten == Some(CALL_CURRENT_SHANTEN)
            }
            _ => false,
        }
    }

    /// 現在3向聴から鳴いて、最良打牌で2向聴になる候補か。
    ///
    /// 判断材料は2向聴からの鳴きと同じ既存の1手評価が持つ鳴き後の最小向聴数だけで、深い
    /// 前方評価は通らない。
    fn reaches_two_shanten_after_call(&self) -> bool {
        match self {
            Self::ThreeShantenCall(inputs) => {
                inputs.post_call_min_shanten == Some(CALL_TWO_SHANTEN_SHANTEN)
            }
            _ => false,
        }
    }
}

/// 重ねて評価する Pass 側の継続評価の種類。
///
/// どちらを評価するかは現在の向聴数だけで決まる。現在の向聴数は手牌と副露数から求めるので
/// 候補ごとに違わず、1回の鳴き判断で必要になる Pass はこのどちらか1つだけになる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassSelfTsumoContinuationKind {
    /// 現在1向聴の Pass 継続評価。
    Iishanten,
    /// 現在2向聴の Pass Full 評価。
    TwoShanten,
    /// 現在3向聴の Pass Progress-only 評価。
    ThreeShanten,
}

/// 別 thread で評価した Pass 側の継続評価。
struct PassSelfTsumoContinuation {
    kind: PassSelfTsumoContinuationKind,
    value: Option<u64>,
    /// 評価そのものの実測。計測しない run では `Duration::ZERO`。
    elapsed: Duration,
}

// 合法 action を候補の作業単位へ落とし、高コストな評価の手前まで進める。
//
// semantic に同一な候補の判定も合法 action の列挙順もここで確定し、deep 評価はまだ行わない。
fn prepare_call_candidates(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
    timing: &mut CallDecisionTimer,
) -> Vec<CallCandidateSlot> {
    let mut slots: Vec<CallCandidateSlot> = Vec::new();
    // 既に評価した semantic key と、その結果を持つ candidate の index。合法な Chi / Pon は
    // 1局面あたり数件なので線形探索で足りる。
    let mut evaluated: Vec<(CallEvaluationKey, usize)> = Vec::new();
    for action in legal_actions {
        let Some((kind, tile, consumed)) = normalize_call(action) else {
            continue;
        };
        timing.start();

        let key = call_meld_and_concealed_tiles(ctx.hand_tiles(), kind, tile, consumed)
            .map(|(meld, post_call_tiles)| CallEvaluationKey::new(&meld, &post_call_tiles));
        if let Some(key) = key.as_ref()
            && let Some(&(_, source)) = evaluated.iter().find(|(known, _)| known == key)
        {
            slots.push(CallCandidateSlot {
                candidate: new_call_candidate(action, kind),
                preparation: CallCandidatePreparation::Reused(source),
                elapsed: CallCandidateElapsed::default(),
                post_call_iishanten_forward_metrics: None,
            });
            continue;
        }

        let mut candidate = new_call_candidate(action, kind);
        let candidate_timing = timing.candidate_timer();
        let preparation = prepare_call_candidate(ctx, kind, tile, consumed, &mut candidate);
        if let Some(key) = key {
            evaluated.push((key, slots.len()));
        }
        slots.push(CallCandidateSlot {
            candidate,
            preparation,
            elapsed: candidate_timing.finish(),
            post_call_iishanten_forward_metrics: None,
        });
    }
    slots
}

// 準備できた候補の高コストな評価をまとめて行い、必要なら Pass 側の継続評価を重ねる。
//
// Pass 側を評価するのは、鳴いた後の最良打牌が1向聴になる候補が1件以上あり、かつ反応元の席が
// 分かっている局面だけ。どちらも安価な事前判定だけで確定するので、即テンパイ Call だけの局面や
// Call policy の前段で落ちる局面へ Pass の継続評価を足すことはない。
//
// 重ねられない runtime (並列度 1) では従来どおり Call → Pass の逐次経路へ落とす。
fn evaluate_prepared_call_candidates(
    ctx: &GameContext,
    slots: &mut [CallCandidateSlot],
    collect_observations: bool,
    order: CallPassEvaluationOrder,
    timing: &mut CallDecisionTimer,
) -> Option<PassSelfTsumoContinuation> {
    let overlaps = order != CallPassEvaluationOrder::Sequential && call_pass_overlap_is_available();
    let Some(kind) = pass_continuation_is_required(ctx, slots).filter(|_| overlaps) else {
        evaluate_call_candidate_group(ctx, slots, collect_observations, timing);
        return None;
    };

    let measured = timing.is_armed();
    let pass = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let since = measured.then(Instant::now);
            let value = match kind {
                PassSelfTsumoContinuationKind::Iishanten => {
                    pass_iishanten_expected_self_tsumo_value(ctx)
                }
                PassSelfTsumoContinuationKind::TwoShanten => {
                    pass_two_shanten_expected_self_tsumo_value(ctx)
                }
                PassSelfTsumoContinuationKind::ThreeShanten => {
                    pass_three_shanten_progress_self_tsumo_value(ctx)
                }
            };
            PassSelfTsumoContinuation {
                kind,
                value,
                elapsed: since.map(|since| since.elapsed()).unwrap_or_default(),
            }
        });
        evaluate_call_candidate_group(ctx, slots, collect_observations, timing);
        worker
            .join()
            .expect("Pass の継続評価 thread は panic しない")
    });
    match pass.kind {
        PassSelfTsumoContinuationKind::Iishanten => {
            timing.record_pass_iishanten_self_tsumo(pass.elapsed)
        }
        PassSelfTsumoContinuationKind::TwoShanten => {
            timing.record_pass_two_shanten_self_tsumo(pass.elapsed)
        }
        PassSelfTsumoContinuationKind::ThreeShanten => {
            timing.record_pass_three_shanten_self_tsumo(pass.elapsed)
        }
    }
    Some(pass)
}

// Call 側の deep 評価 group。候補は合法 action の列挙順で順に評価する。
fn evaluate_call_candidate_group(
    ctx: &GameContext,
    slots: &mut [CallCandidateSlot],
    collect_observations: bool,
    timing: &mut CallDecisionTimer,
) {
    for slot in slots {
        let mut candidate_timing = timing.candidate_timer();
        let reason = evaluate_prepared_call_candidate(
            ctx,
            &slot.preparation,
            collect_observations,
            &mut slot.candidate,
            &mut slot.post_call_iishanten_forward_metrics,
            &mut candidate_timing,
        );
        let Some(reason) = reason else {
            continue;
        };
        slot.elapsed = slot.elapsed.merged(candidate_timing.finish());
        slot.candidate.eligible = reason == CallDecisionReason::EligibleTenpai;
        slot.candidate.reason = reason;
    }
}

// 候補1件ずつの実測を、合法 action の列挙順で計上する。
fn record_call_candidate_timings(slots: &[CallCandidateSlot], timing: &mut CallDecisionTimer) {
    for slot in slots {
        let Some((kind, tile, consumed)) = normalize_call(&slot.candidate.action) else {
            continue;
        };
        if matches!(slot.preparation, CallCandidatePreparation::Reused(_)) {
            timing.record_reused_candidate(kind, tile, consumed);
        } else {
            timing.record_candidate(kind, tile, consumed, slot.elapsed);
        }
    }
}

/// Pass 側の継続評価がこの局面で必要か。必要ならその種類。
///
/// 判断材料は反応元の席が分かるかどうかと、鳴き後の最良打牌が1向聴になる候補があるかどうかだけ
/// で、どちらも [`apply_iishanten_self_tsumo_policy`] / [`apply_two_shanten_self_tsumo_policy`]
/// が Pass を評価する条件そのもの。前者は既存の [`reaction_draw_distance`]、後者は既存の1手
/// 評価が持つ鳴き後の最小向聴数を読む。
fn pass_continuation_is_required(
    ctx: &GameContext,
    slots: &[CallCandidateSlot],
) -> Option<PassSelfTsumoContinuationKind> {
    reaction_draw_distance(ctx)?;
    if slots
        .iter()
        .any(|slot| slot.preparation.stays_iishanten_after_call())
    {
        return Some(PassSelfTsumoContinuationKind::Iishanten);
    }
    if slots
        .iter()
        .any(|slot| slot.preparation.reaches_iishanten_after_call())
    {
        return Some(PassSelfTsumoContinuationKind::TwoShanten);
    }
    if slots
        .iter()
        .any(|slot| slot.preparation.reaches_two_shanten_after_call())
    {
        return Some(PassSelfTsumoContinuationKind::ThreeShanten);
    }
    None
}

/// Call 側の deep 評価と Pass 側の継続評価を別 thread へ重ねられる runtime か。
///
/// 並列度の判断は既存の [`available_parallelism`] をそのまま使い、鳴き判断側で新しい
/// parallelism policy を持たない。分ける相手がいない並列度 1 の環境では従来の逐次経路へ落ちる。
fn call_pass_overlap_is_available() -> bool {
    available_parallelism() > 1
}

fn new_call_candidate(action: &LegalAction, kind: CallKind) -> CallCandidateDiagnostic {
    CallCandidateDiagnostic {
        action: action.clone(),
        kind,
        current_fixed_meld_count: None,
        current_shanten: None,
        post_call_fixed_meld_count: None,
        post_call_forbidden_discards: None,
        post_call_discard: None,
        post_call_wait: None,
        post_call_wait_yaku: None,
        post_call_push_pull: None,
        iishanten_acceptance: None,
        iishanten_self_tsumo: None,
        two_shanten_self_tsumo: None,
        three_shanten_self_tsumo: None,
        eligible: false,
        selected: false,
        reason: CallDecisionReason::EligibleTenpai,
    }
}

// 評価に効く物理牌の属性だけを取り出した表現。
//
// `TileId` は同じ牌種の4枚を別 ID で持つが、向聴・受け入れ・喰い替え・打点のどれも
// `TileId::tile_type()` と `TileId::is_red()` しか読まない (`TileId::copy_index()` は評価経路の
// どこにも現れない)。したがって牌種と赤5かどうかが一致する物理牌は評価上は交換可能で、赤5と
// 黒5は別物として残る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalTile {
    tile_type: TileType,
    red: bool,
}

impl PhysicalTile {
    fn new(tile: TileId) -> Self {
        Self {
            tile_type: tile.tile_type(),
            red: tile.is_red(),
        }
    }

    fn sequence(tiles: &[TileId]) -> Vec<Self> {
        tiles.iter().copied().map(Self::new).collect()
    }
}

/// 鳴き候補1件の評価入力を physical tile semantics へ落とした key。
///
/// `evaluate_call_conditions()` は `call_meld_and_concealed_tiles()` を通した後、判断に使う入力
/// として `GameContext` (全候補で共通) と `meld` / `post_call_tiles` しか読まない。鳴いた牌と
/// consumed もこの2つを組み立てるためだけに使う。したがってこの2つが物理牌 semantics まで一致
/// すれば、
///
/// ```text
/// 鳴き後の concealed hand / Meld / 喰い替え禁止牌 / 鳴き後の副露一覧 / 鳴き後の GameContext
/// / 鳴き後の合法 Dahai / 本番の打牌評価 / 候補の判断結果
/// ```
///
/// はすべて同じになる。
///
/// - 喰い替え禁止牌は [`forbidden_discards_after_call`] が `meld` の種別と牌種だけから決める
/// - 鳴き後の合法 Dahai は concealed hand の物理牌から禁止牌種を除いたもの
/// - 打牌評価と打点は牌種と赤5かどうかだけを読む ([`PhysicalTile`])
///
/// 並び順も含めて比較するため、手牌の並びが違えば別候補として個別に評価する。表示上の
/// `tile` / `consumed` が同じでも、赤5 / 黒5が違えば `red` で別 key になる。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CallEvaluationKey {
    meld_kind: MeldKind,
    meld_called_tile: Option<PhysicalTile>,
    meld_tiles: Vec<PhysicalTile>,
    post_call_concealed: Vec<PhysicalTile>,
}

impl CallEvaluationKey {
    fn new(meld: &Meld, post_call_tiles: &[TileId]) -> Self {
        Self {
            meld_kind: meld.kind(),
            meld_called_tile: meld.called_tile().map(PhysicalTile::new),
            meld_tiles: PhysicalTile::sequence(meld.tiles()),
            post_call_concealed: PhysicalTile::sequence(post_call_tiles),
        }
    }
}

/// `現在2向聴 → Chi / Pon → 打牌 → 2向聴のまま` の候補1件について、production の candidate
/// preparation が組み立てた鳴き後 state。
///
/// observation 専用で、production の判断はこの struct を読まない。値はどれも鳴き判断が実際に
/// 使ったもの、または Push/Pull gate と同じ既存の組み立て ([`post_call_melds`] /
/// [`post_call_legal_dahai_actions`]) の結果で、observation のために向聴・受け入れ・喰い替えを
/// 求め直さない。
#[derive(Debug, Clone)]
pub(crate) struct TwoShantenStayCallState {
    pub(crate) post_call_tiles: Vec<TileId>,
    /// 既存副露に今回の Chi / Pon を加えた副露一覧。
    pub(crate) post_call_melds: Vec<Meld>,
    /// 事前判定が求めた鳴き後の全合法打牌候補の1手評価。喰い替え禁止牌は除いてある。
    pub(crate) post_call_discards: Vec<DiscardEvaluation>,
    pub(crate) forbidden_discards: Vec<TileType>,
    pub(crate) post_call_fixed_meld_count: FixedMeldCount,
    pub(crate) post_call_min_shanten: i8,
    /// 今回の鳴きを反映した鳴き後の局面。自分の席を特定できない場合は `None`。
    pub(crate) post_call_context: Option<GameContext>,
    /// 鳴き後の合法 Dahai。喰い替え禁止牌を除いた物理牌そのもの。
    pub(crate) post_call_legal_actions: Vec<LegalAction>,
}

/// observation 対象の候補が鳴き後 state をどこから読むか。
#[derive(Debug, Clone)]
pub(crate) enum TwoShantenStayCallSource {
    Evaluated(Box<TwoShantenStayCallState>),
    /// 物理牌 semantics まで同じ鳴き後 state を作る先行候補 (候補 index)。
    Reused(usize),
}

/// production の判断で `PostCallNotIishanten` になった `現在2向聴 → 2向聴のまま` の候補。
#[derive(Debug, Clone)]
pub(crate) struct TwoShantenStayCallTarget {
    /// [`CallDecisionDiagnostic::candidates`] の index。合法 action の列挙順。
    pub(crate) candidate_index: usize,
    pub(crate) source: TwoShantenStayCallSource,
}

impl CallCandidateDiagnostic {
    /// production の判断で `現在2向聴 → Call → 打牌 → 2向聴のまま` として止まった候補か。
    ///
    /// production が書き込んだ現在向聴数・鳴き後の最良打牌の向聴数・理由を読むだけで、判定を
    /// やり直さない。
    pub fn stays_two_shanten_after_call(&self) -> bool {
        self.current_shanten == Some(CALL_TWO_SHANTEN_SHANTEN)
            && self.post_call_shanten() == Some(CALL_TWO_SHANTEN_SHANTEN)
            && self.reason == CallDecisionReason::PostCallNotIishanten
    }
}

/// production と同じ鳴き判断を1回行い、結論と `現在2向聴 → 2向聴のまま` の候補の鳴き後 state を
/// 返す。
///
/// 判断は `act()` と同じ入口・同じ評価順で、observation のための追加探索はここでは走らない。
/// 鳴き後 state は判断が捨てる前の candidate preparation から取り出すだけなので、同じ state を
/// 別実装で組み立て直さない。合法な Chi / Pon が無ければ `None`。
pub(crate) fn evaluate_call_decision_with_two_shanten_stay_calls(
    ctx: &GameContext,
    legal_actions: &[LegalAction],
) -> Option<(CallDecisionDiagnostic, Vec<TwoShantenStayCallTarget>)> {
    let (decision, post_call_states) = evaluate_call_decision_with_post_call_states(
        ctx,
        legal_actions,
        false,
        CallPassEvaluationOrder::PRODUCTION,
        &mut CallDecisionTimer::disabled(),
    )?;
    let targets = post_call_states
        .into_iter()
        .enumerate()
        .filter(|(index, _)| decision.candidates[*index].stays_two_shanten_after_call())
        .filter_map(|(index, state)| {
            let source = match state {
                PostCallState::Reused(source) => TwoShantenStayCallSource::Reused(source),
                PostCallState::Evaluated {
                    preparation: CallCandidatePreparation::TwoShantenCall(inputs),
                    ..
                } => {
                    let candidate = &decision.candidates[index];
                    let forbidden_discards = candidate.post_call_forbidden_discards.clone()?;
                    let post_call_melds = post_call_melds(ctx, &inputs.meld);
                    TwoShantenStayCallSource::Evaluated(Box::new(TwoShantenStayCallState {
                        post_call_context: ctx.with_own_hand_state(
                            inputs.post_call_tiles.clone(),
                            post_call_melds.clone(),
                        ),
                        post_call_legal_actions: post_call_legal_dahai_actions(
                            &inputs.post_call_tiles,
                            &forbidden_discards,
                        ),
                        post_call_fixed_meld_count: candidate.post_call_fixed_meld_count?,
                        post_call_min_shanten: inputs.post_call_min_shanten?,
                        post_call_tiles: inputs.post_call_tiles,
                        post_call_melds,
                        post_call_discards: inputs.post_call_discards,
                        forbidden_discards,
                    }))
                }
                PostCallState::Evaluated { .. } => return None,
            };
            Some(TwoShantenStayCallTarget {
                candidate_index: index,
                source,
            })
        })
        .collect();
    Some((decision, targets))
}

// 合法 action を Chi / Pon の共通表現へ正規化する。それ以外の action は対象外。
fn normalize_call(action: &LegalAction) -> Option<(CallKind, TileId, &[TileId])> {
    match action {
        LegalAction::Chi { tile, consumed } => Some((CallKind::Chi, *tile, consumed)),
        LegalAction::Pon { tile, consumed } => Some((CallKind::Pon, *tile, consumed)),
        _ => None,
    }
}

// 成立した候補の中から採用する1件を選ぶ。
//
// 比較軸は鳴き後の最良打牌評価で、通常打牌選択と同じ既存 comparator をそのまま使う。鳴き専用の
// EV や重み付けは持たない。完全に同値な候補では先に現れた候補を維持するため、合法 action の
// 列挙順が安定した tie-break になる。
fn select_eligible_candidate(candidates: &[CallCandidateDiagnostic]) -> Option<usize> {
    let (indices, evaluations): (Vec<usize>, Vec<DiscardEvaluation>) = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.reason == CallDecisionReason::EligibleTenpai)
        .filter_map(|(index, candidate)| {
            candidate
                .post_call_discard
                .clone()
                .map(|evaluation| (index, evaluation))
        })
        .unzip();

    if let Some(best) = best_discard_selection_index(&evaluations, &[]) {
        return Some(indices[best]);
    }

    // 既存の即テンパイ候補が無い場合だけ、Pass より厳密に高い Call の最大値を選ぶ。現在の
    // 向聴数は候補ごとに違わないので1向聴・2向聴・3向聴は同じ局面に並ばないが、判定順は従来
    // どおり向聴数の小さい方から見る。同値の Call 候補では合法 action の先頭を維持する。
    best_self_tsumo_candidate(
        candidates,
        CallDecisionReason::EligibleIishantenSelfTsumo,
        |candidate| {
            candidate
                .iishanten_self_tsumo
                .and_then(|diagnostic| diagnostic.call_expected_self_tsumo_value)
        },
    )
    .or_else(|| {
        best_self_tsumo_candidate(
            candidates,
            CallDecisionReason::EligibleTwoShantenSelfTsumo,
            |candidate| {
                candidate
                    .two_shanten_self_tsumo
                    .and_then(|diagnostic| diagnostic.call_expected_self_tsumo_value)
            },
        )
    })
    .or_else(|| {
        best_self_tsumo_candidate(
            candidates,
            CallDecisionReason::EligibleThreeShantenSelfTsumo,
            |candidate| {
                candidate
                    .three_shanten_self_tsumo
                    .and_then(|diagnostic| diagnostic.call_expected_self_tsumo_value)
            },
        )
    })
    // 値比較で成立した候補が無い場合だけ、速度優先 policy で成立した候補を同じ tie-break で
    // 選ぶ。値比較で成立する候補があればそちらを優先し、従来の選択を変えない。
    .or_else(|| {
        best_self_tsumo_candidate(
            candidates,
            CallDecisionReason::EligibleTwoShantenSpeed,
            |candidate| {
                candidate
                    .two_shanten_self_tsumo
                    .and_then(|diagnostic| diagnostic.call_expected_self_tsumo_value)
            },
        )
    })
    .or_else(|| {
        best_self_tsumo_candidate(
            candidates,
            CallDecisionReason::EligibleThreeShantenSpeed,
            |candidate| {
                candidate
                    .three_shanten_self_tsumo
                    .and_then(|diagnostic| diagnostic.call_expected_self_tsumo_value)
            },
        )
    })
}

// 成立した Call 候補のうち、Call 側 ExpectedSelfTsumoValue が最大のもの。値は候補の比較に
// 使ったものそのままで、同値では合法 action の先頭を維持する。
fn best_self_tsumo_candidate(
    candidates: &[CallCandidateDiagnostic],
    eligible: CallDecisionReason,
    call_value: impl Fn(&CallCandidateDiagnostic) -> Option<u64>,
) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.reason != eligible {
            continue;
        }
        let Some(value) = call_value(candidate) else {
            continue;
        };
        if best.is_none_or(|(_, best_value)| value > best_value) {
            best = Some((index, value));
        }
    }
    best.map(|(index, _)| index)
}

/// 安価な事前判定が組み立てた、鳴き後打牌選択の入力。
///
/// `evaluate_call_conditions()` の後半 ([`evaluate_post_call_discard`]) はこの struct と
/// `GameContext` しか読まない。鳴いた牌と consumed もここを組み立てるためだけに使う。
struct PostCallInputs {
    meld: Meld,
    post_call_tiles: Vec<TileId>,
    counts: TileCounts,
    current_fixed_meld_count: FixedMeldCount,
    post_call_fixed_meld_count: FixedMeldCount,
    legal_actions: Vec<LegalAction>,
    post_call_context: GameContext,
    /// 鳴き後の合法打牌候補の最小向聴数。既存の1手評価
    /// ([`post_call_discard_evaluations`]) が持つ値の最小値そのままで、ここで数え直さない。
    ///
    /// 比較順の先頭が向聴数なので、本番の鳴き後打牌選択が選ぶ候補の向聴数もこの値になる。
    /// 高コストな前方評価の前に「鳴いても1向聴のまま」を判定できるのはそのため。
    post_call_min_shanten: Option<i8>,
}

/// 現在2向聴からの鳴きについて、鳴き後打牌選択の入力。
struct TwoShantenCallInputs {
    meld: Meld,
    post_call_tiles: Vec<TileId>,
    /// 鳴き後の全合法打牌候補の1手評価。喰い替え禁止牌は既に除いてある。
    ///
    /// 前方評価も打点も通らない安価な評価なので、深い評価へ進む候補を確定させるために事前
    /// 判定で1回だけ求め、そのまま鳴き後の打牌比較へ渡す。同じ state を2回評価しない。
    post_call_discards: Vec<DiscardEvaluation>,
    /// 鳴き後の合法打牌候補の最小向聴数。`post_call_discards` の最小値そのままで、ここで
    /// 数え直さない。
    ///
    /// 比較順の先頭が向聴数なので、鳴き後の打牌比較が選ぶ候補の向聴数もこの値になる。
    post_call_min_shanten: Option<i8>,
}

/// 現在3向聴からの鳴きについて、鳴き後打牌選択の入力。
///
/// 中身は現在2向聴からの鳴きと同じで、読む policy と比較する向聴数だけが違う。
struct ThreeShantenCallInputs {
    meld: Meld,
    post_call_tiles: Vec<TileId>,
    /// 鳴き後の全合法打牌候補の1手評価。喰い替え禁止牌は既に除いてある。
    post_call_discards: Vec<DiscardEvaluation>,
    /// 鳴き後の合法打牌候補の最小向聴数。`post_call_discards` の最小値そのままで、ここで
    /// 数え直さない。
    post_call_min_shanten: Option<i8>,
}

// 鳴き成立条件のうち、高コストな鳴き後打牌選択より手前を評価する。
//
// 最初に落ちた条件を理由として candidate へ書き込み、`Settled` を返す。残りの条件を評価できる
// 候補は、鳴き後打牌選択の入力を組み立てて返す。
fn prepare_call_candidate(
    ctx: &GameContext,
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
    candidate: &mut CallCandidateDiagnostic,
) -> CallCandidatePreparation {
    let settled = |candidate: &mut CallCandidateDiagnostic, reason| {
        candidate.eligible = reason == CallDecisionReason::EligibleTenpai;
        candidate.reason = reason;
        CallCandidatePreparation::Settled
    };

    if ctx.any_opponent_reached() {
        return settled(candidate, CallDecisionReason::OpponentReached);
    }

    // Chi / Pon は他家捨て牌への reaction なので、既存 client の reaction context に drawn_tile は
    // 無い。drawn_tile がある不整合な context では、それを混ぜても無視しても正しい局面を復元
    // できないため鳴きを検討しない。
    if ctx.drawn_tile().is_some() {
        return settled(candidate, CallDecisionReason::UnexpectedDrawnTile);
    }

    let hand_tiles = ctx.hand_tiles();
    let Some((meld, post_call_tiles)) =
        call_meld_and_concealed_tiles(hand_tiles, kind, tile, consumed)
    else {
        return settled(candidate, CallDecisionReason::InvalidConsumed);
    };

    let Some(current_fixed_meld_count) = ctx.own_fixed_meld_count() else {
        return settled(candidate, CallDecisionReason::FixedMeldCountUnknown);
    };
    candidate.current_fixed_meld_count = Some(current_fixed_meld_count);

    let Some(post_call_fixed_meld_count) = FixedMeldCount::new(current_fixed_meld_count.get() + 1)
    else {
        return settled(candidate, CallDecisionReason::FixedMeldCountOverflow);
    };
    candidate.post_call_fixed_meld_count = Some(post_call_fixed_meld_count);

    let counts = TileCounts::from_tiles(hand_tiles.iter().copied());
    let current_shanten =
        calculate_shanten_with_fixed_melds(&counts, current_fixed_meld_count).min();
    candidate.current_shanten = Some(current_shanten);
    if current_shanten != CALL_CURRENT_SHANTEN {
        if matches!(
            current_shanten,
            CALL_TWO_SHANTEN_SHANTEN | CALL_THREE_SHANTEN_SHANTEN
        ) {
            // 1向聴からの鳴きと同じく、鳴き後の1手評価だけを先に通して最小向聴数を確定させる。
            // 深い評価へ進む候補が分かるので、Pass 側の継続評価をここから重ねられる。
            let forbidden_discards = forbidden_discards_after_call(&meld);
            let post_call_discards = post_call_discard_evaluations(
                ctx,
                &post_call_tiles,
                post_call_fixed_meld_count,
                &forbidden_discards,
            );
            candidate.post_call_forbidden_discards = Some(forbidden_discards);
            let post_call_min_shanten = post_call_discards
                .iter()
                .map(DiscardEvaluation::min_shanten_after_discard)
                .min();
            return if current_shanten == CALL_TWO_SHANTEN_SHANTEN {
                CallCandidatePreparation::TwoShantenCall(Box::new(TwoShantenCallInputs {
                    meld,
                    post_call_tiles,
                    post_call_discards,
                    post_call_min_shanten,
                }))
            } else {
                CallCandidatePreparation::ThreeShantenCall(Box::new(ThreeShantenCallInputs {
                    meld,
                    post_call_tiles,
                    post_call_discards,
                    post_call_min_shanten,
                }))
            };
        }
        return settled(candidate, CallDecisionReason::CurrentShantenNotCallable);
    }

    // 喰い替え禁止牌は鳴き直後だけの合法手制約なので、仮想 legal actions から先に除く。
    // 残った合法 Dahai は実際の通常打牌と同じ production selector へ渡す。
    let forbidden_discards = forbidden_discards_after_call(&meld);
    let legal_actions = post_call_legal_dahai_actions(&post_call_tiles, &forbidden_discards);
    let mut post_call_melds = ctx.own_melds().unwrap_or_default().to_vec();
    post_call_melds.push(meld.clone());
    let post_call_context = ctx.with_own_hand_state(post_call_tiles.clone(), post_call_melds);
    candidate.post_call_forbidden_discards = Some(forbidden_discards.clone());
    let Some(post_call_context) = post_call_context else {
        return settled(candidate, CallDecisionReason::PostCallEvaluationUnavailable);
    };

    // 鳴き後の1手評価だけを先に通し、最小向聴数を確定させる。前方評価も打点も通らない安価な
    // 評価で、本番の鳴き後打牌選択はこの後そのまま自分で候補を作り直す。
    let post_call_min_shanten = post_call_discard_evaluations(
        &post_call_context,
        &post_call_tiles,
        post_call_fixed_meld_count,
        &forbidden_discards,
    )
    .iter()
    .map(DiscardEvaluation::min_shanten_after_discard)
    .min();

    CallCandidatePreparation::PostCall(Box::new(PostCallInputs {
        meld,
        post_call_tiles,
        counts,
        current_fixed_meld_count,
        post_call_fixed_meld_count,
        legal_actions,
        post_call_context,
        post_call_min_shanten,
    }))
}

// 事前判定が準備した候補の高コストな評価。
//
// 安価な事前判定で理由が確定した候補と、先行候補の結果を複製する候補では何もせず `None`。
// 鳴き後の選択打牌が1向聴なら、その選択が求めた前方集計値を `iishanten_forward_metrics` へ残す。
fn evaluate_prepared_call_candidate(
    ctx: &GameContext,
    preparation: &CallCandidatePreparation,
    collect_observations: bool,
    candidate: &mut CallCandidateDiagnostic,
    iishanten_forward_metrics: &mut Option<ForwardMetrics>,
    timing: &mut CallCandidateTimer,
) -> Option<CallDecisionReason> {
    match preparation {
        CallCandidatePreparation::PostCall(inputs) => Some(evaluate_post_call_discard(
            ctx,
            inputs,
            collect_observations,
            candidate,
            iishanten_forward_metrics,
            timing,
        )),
        CallCandidatePreparation::TwoShantenCall(inputs) => {
            Some(evaluate_two_shanten_call_to_iishanten(
                ctx,
                inputs,
                candidate,
                iishanten_forward_metrics,
            ))
        }
        CallCandidatePreparation::ThreeShantenCall(inputs) => Some(
            evaluate_three_shanten_call_to_two_shanten(ctx, inputs, candidate),
        ),
        CallCandidatePreparation::Settled | CallCandidatePreparation::Reused(_) => None,
    }
}

// 鳴き成立条件のうち、鳴き後の打牌選択から先を評価する。評価が進んだ範囲の値だけを candidate
// へ書き込み、評価しなかった項目は None のままにする。
fn evaluate_post_call_discard(
    ctx: &GameContext,
    inputs: &PostCallInputs,
    collect_observations: bool,
    candidate: &mut CallCandidateDiagnostic,
    iishanten_forward_metrics: &mut Option<ForwardMetrics>,
    timing: &mut CallCandidateTimer,
) -> CallDecisionReason {
    let selection = timing.measure_post_call_discard_selection(|| {
        select_discard_action_with_evaluation(&inputs.post_call_context, &inputs.legal_actions)
    });
    let Some(evaluation) = selection.evaluation.as_ref() else {
        return CallDecisionReason::NoPostCallDiscard;
    };

    if evaluation.min_shanten_after_discard() != CALL_TENPAI_SHANTEN {
        if evaluation.min_shanten_after_discard() == CALL_CURRENT_SHANTEN {
            *iishanten_forward_metrics = selection.iishanten_forward_metrics;
            candidate.iishanten_self_tsumo = Some(CallIishantenSelfTsumoDiagnostic {
                reaction_source_player: ctx.reaction_source_player(),
                pass_expected_self_tsumo_value: None,
                call_expected_self_tsumo_value: selection
                    .iishanten_forward_metrics
                    .and_then(|metrics| metrics.expected_self_tsumo_value),
                comparison: CallIishantenComparison::Unknown,
            });
        }

        // production が選んだ打牌評価を診断にもそのまま載せる。
        if collect_observations {
            candidate.iishanten_acceptance = iishanten_acceptance_diagnostic(
                ctx,
                &inputs.counts,
                inputs.current_fixed_meld_count,
                &inputs.meld,
                evaluation,
            );
        }
        candidate.post_call_discard = Some(evaluation.clone());
        return CallDecisionReason::PostCallNotTenpai;
    }

    let Some(wait) = selection.tenpai_wait.clone().or_else(|| {
        discard_tenpai_wait_availability(
            &TileCounts::from_tiles(inputs.post_call_tiles.iter().copied()),
            inputs.post_call_fixed_meld_count,
            evaluation,
            &OwnDiscards::from_optional_river(ctx.own_discards()),
            ctx.history_furiten_after_own_discard(),
        )
    }) else {
        candidate.post_call_discard = Some(evaluation.clone());
        return CallDecisionReason::PostCallNotTenpai;
    };

    let reason = evaluate_post_call_conditions(
        ctx,
        &inputs.meld,
        &inputs.post_call_tiles,
        evaluation,
        &wait,
        candidate,
    );
    let reason = if reason == CallDecisionReason::EligibleTenpai {
        let decision = post_call_push_pull_decision(
            &inputs.post_call_context,
            &selection,
            &wait,
            &inputs.legal_actions,
        );
        candidate.post_call_push_pull = Some(decision);
        if decision.mode == PushPullMode::Push {
            CallDecisionReason::EligibleTenpai
        } else {
            CallDecisionReason::PostCallNotPush
        }
    } else {
        reason
    };
    candidate.post_call_discard = Some(evaluation.clone());
    candidate.post_call_wait = Some(wait);
    reason
}

// 鳴き後に切れる全物理牌を、喰い替え禁止牌だけ除いて仮想 legal actions にする。牌種の比較と
// 同牌種内の赤黒 preference は通常打牌 selector に委ねる。
fn post_call_legal_dahai_actions(
    post_call_tiles: &[TileId],
    forbidden_discards: &[TileType],
) -> Vec<LegalAction> {
    post_call_tiles
        .iter()
        .copied()
        .filter(|tile| !forbidden_discards.contains(&tile.tile_type()))
        .map(|tile| LegalAction::Dahai { tile })
        .collect()
}

// production selector が選んだ evaluation / wait / offense と同じ仮想 legal actions を既存
// Push/Pull 入力へ接続する。threat classification と threshold は push_pull 側に委ねる。
fn post_call_push_pull_decision(
    post_call_context: &GameContext,
    selection: &DiscardActionSelection,
    wait: &TenpaiWaitAvailability,
    legal_actions: &[LegalAction],
) -> PushPullDecision {
    let evaluation = selection
        .evaluation
        .as_ref()
        .expect("production selector が選んだ評価を渡す");
    let inputs = push_pull_inputs_from_selected_tenpai(
        post_call_context,
        evaluation,
        wait,
        selection.tenpai_offense_value,
        legal_actions,
    );
    decide_push_pull(&inputs)
}

// 現在2向聴からの鳴きを、既存の post-call selector へ通す。候補生成・喰い替え・向聴・
// acceptance・打牌比較・Call 側 value はすべて既存経路の結果をそのまま使う。
//
// 鳴き後の最良打牌が1向聴になる候補だけ Call 側の値を載せ、Pass との比較は
// [`apply_two_shanten_self_tsumo_policy`] が行う。1向聴にならない候補はそこで確定する。
// 選択に使う metric は1向聴の打牌候補にだけ求まるので、鳴いても2向聴のままの局面で前方評価
// が走ることはない。
fn evaluate_two_shanten_call_to_iishanten(
    ctx: &GameContext,
    inputs: &TwoShantenCallInputs,
    candidate: &mut CallCandidateDiagnostic,
    iishanten_forward_metrics: &mut Option<ForwardMetrics>,
) -> CallDecisionReason {
    let melds = post_call_melds(ctx, &inputs.meld);
    // 残り自摸機会の条件を満たす局面だけ、同じ前方評価から速度優先 policy の翻数判定も回収する。
    let own_future_draws = own_future_draws(ctx);
    let required_han = speed_required_han(own_future_draws);
    let Some(selection) = select_best_iishanten_post_call_discard(
        ctx,
        &inputs.post_call_tiles,
        &melds,
        &inputs.post_call_discards,
        required_han,
    ) else {
        return CallDecisionReason::NoPostCallDiscard;
    };
    let post_call_shanten = selection.evaluation.min_shanten_after_discard();
    if post_call_shanten != CALL_CURRENT_SHANTEN {
        candidate.post_call_discard = Some(selection.evaluation);
        return CallDecisionReason::PostCallNotIishanten;
    }

    candidate.post_call_discard = Some(selection.evaluation);
    *iishanten_forward_metrics = Some(selection.forward_metrics);
    candidate.two_shanten_self_tsumo = Some(CallTwoShantenSelfTsumoDiagnostic {
        reaction_source_player: ctx.reaction_source_player(),
        pass_evaluation: CallTwoShantenPassEvaluation::Full,
        pass_expected_self_tsumo_value: None,
        call_expected_self_tsumo_value: selection.forward_metrics.expected_self_tsumo_value,
        comparison: CallIishantenComparison::Unknown,
        speed: CallTwoShantenSpeedDiagnostic {
            own_future_draws,
            han: selection.continuation_han,
            overrides_pass: false,
        },
    });
    // Pass の評価はここでは行わないので、比較が終わるまでは未確定の理由を置く。
    CallDecisionReason::IishantenSelfTsumoUnknown
}

// 現在3向聴からの鳴きを、鳴き後2向聴の Progress-only 比較へ通す。候補生成・喰い替え・向聴・
// acceptance・打牌比較・Call 側 value はすべて既存経路の結果をそのまま使う。
//
// 鳴き後の最良打牌が2向聴になる候補だけ Call 側の値を載せ、Pass との比較は
// [`apply_three_shanten_self_tsumo_policy`] が行う。2向聴にならない候補はそこで確定する。
// 選択に使う metric は2向聴の打牌候補にだけ求まるので、鳴いても3向聴のままの局面で前方評価
// が走ることはない。
fn evaluate_three_shanten_call_to_two_shanten(
    ctx: &GameContext,
    inputs: &ThreeShantenCallInputs,
    candidate: &mut CallCandidateDiagnostic,
) -> CallDecisionReason {
    let melds = post_call_melds(ctx, &inputs.meld);
    // 残り自摸機会の条件を満たす局面だけ、同じ前方評価から速度優先 policy の翻数判定も回収する。
    let own_future_draws = own_future_draws(ctx);
    let required_han = three_shanten_speed_required_han(own_future_draws);
    let Some(selection) = select_best_two_shanten_post_call_discard(
        ctx,
        &inputs.post_call_tiles,
        &melds,
        &inputs.post_call_discards,
        required_han,
    ) else {
        return CallDecisionReason::NoPostCallDiscard;
    };
    let post_call_shanten = selection.evaluation.min_shanten_after_discard();
    if post_call_shanten != CALL_TWO_SHANTEN_SHANTEN {
        candidate.post_call_discard = Some(selection.evaluation);
        return CallDecisionReason::PostCallNotTwoShanten;
    }

    candidate.post_call_discard = Some(selection.evaluation);
    candidate.three_shanten_self_tsumo = Some(CallThreeShantenSelfTsumoDiagnostic {
        reaction_source_player: ctx.reaction_source_player(),
        pass_evaluation: CallThreeShantenPassEvaluation::ProgressOnly,
        pass_expected_self_tsumo_value: None,
        call_expected_self_tsumo_value: selection.expected_self_tsumo_value,
        comparison: CallIishantenComparison::Unknown,
        speed: CallThreeShantenSpeedDiagnostic {
            own_future_draws,
            han: selection.scored_han,
            overrides_pass: false,
        },
    });
    // Pass の評価はここでは行わないので、比較が終わるまでは未確定の理由を置く。
    CallDecisionReason::IishantenSelfTsumoUnknown
}

// 速度優先 policy が求める翻数。安価な残り自摸機会の条件を満たす局面だけ判定を要求する。
//
// 残り自摸機会は巡目や河の枚数から推測せず、鳴き後の打牌選択と self-tsumo continuation が使う
// 既存の [`own_future_draws`] をそのまま読む。確定できない局面と閾値未満の局面では判定を要求せず、
// policy も適用しない。
fn speed_required_han(own_future_draws: Option<u32>) -> Option<u8> {
    own_future_draws
        .is_some_and(|draws| draws >= CALL_TWO_SHANTEN_SPEED_MIN_DRAWS)
        .then_some(CALL_TWO_SHANTEN_SPEED_MIN_HAN)
}

// 3向聴からの速度優先 policy が求める翻数。読み方は [`speed_required_han`] と同じで、閾値だけが
// 違う。
fn three_shanten_speed_required_han(own_future_draws: Option<u32>) -> Option<u8> {
    own_future_draws
        .is_some_and(|draws| draws >= CALL_THREE_SHANTEN_SPEED_MIN_DRAWS)
        .then_some(CALL_THREE_SHANTEN_SPEED_MIN_HAN)
}

// 1向聴のままの Call 候補がある場合だけ Pass を1回評価し、全候補へ同じ値を配る。
//
// `pass` は Call 側の deep 評価と重ねて先に評価した結果。重ねなかった局面ではここで評価する。
// どちらの経路でも Pass を評価するのは1回だけで、値も比較も同じ。
fn apply_iishanten_self_tsumo_policy(
    ctx: &GameContext,
    candidates: &mut [CallCandidateDiagnostic],
    pass: Option<PassSelfTsumoContinuation>,
    timing: &mut CallDecisionTimer,
) {
    if !candidates
        .iter()
        .any(|candidate| candidate.iishanten_self_tsumo.is_some())
    {
        return;
    }

    let reaction_source_known = reaction_draw_distance(ctx).is_some();
    let pass_value = match pass {
        Some(pass) => pass.value,
        None => reaction_source_known
            .then(|| {
                timing.measure_pass_iishanten_self_tsumo(|| {
                    pass_iishanten_expected_self_tsumo_value(ctx)
                })
            })
            .flatten(),
    };

    for candidate in candidates {
        let Some(mut diagnostic) = candidate.iishanten_self_tsumo else {
            continue;
        };
        diagnostic.pass_expected_self_tsumo_value = pass_value;
        let (comparison, reason) = compare_call_pass_self_tsumo_values(
            reaction_source_known,
            diagnostic.call_expected_self_tsumo_value,
            diagnostic.pass_expected_self_tsumo_value,
            CallDecisionReason::EligibleIishantenSelfTsumo,
        );
        diagnostic.comparison = comparison;
        candidate.iishanten_self_tsumo = Some(diagnostic);
        candidate.eligible = reason == CallDecisionReason::EligibleIishantenSelfTsumo;
        candidate.reason = reason;
    }
}

// 鳴き後1向聴になる2向聴 Call 候補がある場合だけ Pass の2向聴 Full 値を1回求め、全候補へ
// 共有する。比較の semantics も同値・unknown の扱いも1向聴からの鳴きと同じ。
//
// `pass` は Call 側の deep 評価と重ねて先に評価した結果。重ねなかった局面ではここで評価する。
// どちらの経路でも Pass を評価するのは1回だけで、値も比較も同じ。
fn apply_two_shanten_self_tsumo_policy(
    ctx: &GameContext,
    candidates: &mut [CallCandidateDiagnostic],
    pass: Option<PassSelfTsumoContinuation>,
    timing: &mut CallDecisionTimer,
) {
    if !candidates
        .iter()
        .any(|candidate| candidate.two_shanten_self_tsumo.is_some())
    {
        return;
    }

    let reaction_source_known = reaction_draw_distance(ctx).is_some();
    let pass_value = match pass {
        Some(pass) => pass.value,
        None => reaction_source_known
            .then(|| {
                timing.measure_pass_two_shanten_self_tsumo(|| {
                    pass_two_shanten_expected_self_tsumo_value(ctx)
                })
            })
            .flatten(),
    };

    for candidate in candidates {
        let Some(mut diagnostic) = candidate.two_shanten_self_tsumo else {
            continue;
        };
        diagnostic.pass_expected_self_tsumo_value = pass_value;
        let (comparison, reason) = compare_call_pass_self_tsumo_values(
            reaction_source_known,
            diagnostic.call_expected_self_tsumo_value,
            diagnostic.pass_expected_self_tsumo_value,
            CallDecisionReason::EligibleTwoShantenSelfTsumo,
        );
        diagnostic.comparison = comparison;
        // 速度優先 policy が上書きするのは Call / Pass の値比較で Pass になる結論だけ。他の
        // rejection 条件はそのまま残し、打点による例外を持たせない。
        let reason = if reason == CallDecisionReason::PassSelfTsumoNotLower
            && diagnostic.speed.is_satisfied()
        {
            diagnostic.speed.overrides_pass = true;
            CallDecisionReason::EligibleTwoShantenSpeed
        } else {
            reason
        };
        candidate.two_shanten_self_tsumo = Some(diagnostic);
        candidate.eligible = matches!(
            reason,
            CallDecisionReason::EligibleTwoShantenSelfTsumo
                | CallDecisionReason::EligibleTwoShantenSpeed
        );
        candidate.reason = reason;
    }
}

// 鳴き後2向聴になる3向聴 Call 候補がある場合だけ Pass の3向聴 Progress-only 値を1回求め、
// 全候補へ共有する。比較の semantics も同値・unknown の扱いも1向聴・2向聴からの鳴きと同じ。
//
// `pass` は Call 側の deep 評価と重ねて先に評価した結果。重ねなかった局面ではここで評価する。
// どちらの経路でも Pass を評価するのは1回だけで、値も比較も同じ。
fn apply_three_shanten_self_tsumo_policy(
    ctx: &GameContext,
    candidates: &mut [CallCandidateDiagnostic],
    pass: Option<PassSelfTsumoContinuation>,
    timing: &mut CallDecisionTimer,
) {
    if !candidates
        .iter()
        .any(|candidate| candidate.three_shanten_self_tsumo.is_some())
    {
        return;
    }

    let reaction_source_known = reaction_draw_distance(ctx).is_some();
    let pass_value = match pass {
        Some(pass) => pass.value,
        None => reaction_source_known
            .then(|| {
                timing.measure_pass_three_shanten_self_tsumo(|| {
                    pass_three_shanten_progress_self_tsumo_value(ctx)
                })
            })
            .flatten(),
    };

    for candidate in candidates {
        let Some(mut diagnostic) = candidate.three_shanten_self_tsumo else {
            continue;
        };
        diagnostic.pass_expected_self_tsumo_value = pass_value;
        let (comparison, reason) = compare_call_pass_self_tsumo_values(
            reaction_source_known,
            diagnostic.call_expected_self_tsumo_value,
            diagnostic.pass_expected_self_tsumo_value,
            CallDecisionReason::EligibleThreeShantenSelfTsumo,
        );
        diagnostic.comparison = comparison;
        // 速度優先 policy が上書きするのは Call / Pass の値比較で Pass になる結論だけ。他の
        // rejection 条件はそのまま残し、打点による例外を持たせない。
        let reason = if reason == CallDecisionReason::PassSelfTsumoNotLower
            && diagnostic.speed.is_satisfied()
        {
            diagnostic.speed.overrides_pass = true;
            CallDecisionReason::EligibleThreeShantenSpeed
        } else {
            reason
        };
        candidate.three_shanten_self_tsumo = Some(diagnostic);
        candidate.eligible = matches!(
            reason,
            CallDecisionReason::EligibleThreeShantenSelfTsumo
                | CallDecisionReason::EligibleThreeShantenSpeed
        );
        candidate.reason = reason;
    }
}

// Call / Pass 比較で成立した非テンパイ Call を、鳴き後の既存 Push/Pull で確かめる。
//
// 対象は Call / Pass policy が成立させた候補 ([`is_call_pass_eligible_reason`]) だけで、比較で
// 落ちた候補の理由は上書きしない。即テンパイ Call は成立条件の中で同じ判定を既に通っている。
//
// 入力は通常の `act()` が打牌選択の結果を押し引きへ渡すのと同じ
// [`push_pull_inputs_from_threat_facts`] で、鳴き後の打牌選択が既に選んだ打牌評価・1向聴の
// 前方集計値 (configured horizon)・鳴き後の合法打牌をそのまま渡す。threat の分類・選択打牌の
// hard-safe 判定・threshold は push_pull 側が持つ。
//
// 1向聴 Push/Fold の threshold と比較する ExpectedSelfTsumoValue は `UNTIL_RYUKYOKU` の尺度で、
// configured horizon が `UNTIL_RYUKYOKU` なら選択の計算済み値を再利用して追加評価しない。それ以外の
// horizon では選択済みの1打牌だけを `UNTIL_RYUKYOKU` で評価し直し、全候補は再探索しない。
//
// threat facts は鳴く前の局面から1回だけ作り、全候補で共有する。鳴いて変わるのは自分の手牌と
// 副露だけで、押し引きが読む他家の facts は鳴く前と同じになる。
//
// 現在の [`decide_push_pull`] は `Neutral` を返さない。即テンパイ Call と同じく `Push` 以外は
// 鳴き後に攻撃を継続できない結論として [`CallDecisionReason::PostCallNotPush`] にする。
fn apply_post_call_push_pull_gate(
    ctx: &GameContext,
    candidates: &mut [CallCandidateDiagnostic],
    post_call_states: &[PostCallState],
) {
    let mut player_threats: Option<[PlayerThreatFacts; 4]> = None;
    for index in 0..candidates.len() {
        if !is_call_pass_eligible_reason(candidates[index].reason) {
            continue;
        }
        let decision = match &post_call_states[index] {
            // 先行候補と同じ鳴き後 state なので、同じ結論をそのまま使う。
            PostCallState::Reused(source) => candidates[*source].post_call_push_pull,
            PostCallState::Evaluated {
                preparation,
                iishanten_forward_metrics,
            } => {
                let player_threats =
                    *player_threats.get_or_insert_with(|| player_threat_facts_from_context(ctx));
                post_call_non_tenpai_push_pull_decision(
                    ctx,
                    player_threats,
                    preparation,
                    &candidates[index],
                    *iishanten_forward_metrics,
                )
            }
        };

        let candidate = &mut candidates[index];
        candidate.post_call_push_pull = decision;
        let reason = match decision {
            Some(decision) if decision.mode == PushPullMode::Push => continue,
            Some(_) => CallDecisionReason::PostCallNotPush,
            None => CallDecisionReason::PostCallEvaluationUnavailable,
        };
        candidate.eligible = false;
        candidate.reason = reason;
    }
}

/// 非テンパイ Call の Call / Pass policy が成立させた理由か。
fn is_call_pass_eligible_reason(reason: CallDecisionReason) -> bool {
    matches!(
        reason,
        CallDecisionReason::EligibleIishantenSelfTsumo
            | CallDecisionReason::EligibleTwoShantenSelfTsumo
            | CallDecisionReason::EligibleTwoShantenSpeed
            | CallDecisionReason::EligibleThreeShantenSelfTsumo
            | CallDecisionReason::EligibleThreeShantenSpeed
    )
}

// 鳴き後の打牌選択が既に選んだ非テンパイ打牌を、既存 Push/Pull 入力へ接続する。
//
// 鳴き後の手牌 state と合法打牌は、その候補の打牌選択が使ったものと同じ作り方をする。1向聴から
// の鳴きは事前判定が組み立てた鳴き後 context と合法 Dahai をそのまま使い、2向聴・3向聴からの
// 鳴きは同じ手牌・副露・喰い替え禁止牌から組み立てる。自分の席を特定できず鳴き後 context を
// 作れない場合は `None`。
fn post_call_non_tenpai_push_pull_decision(
    ctx: &GameContext,
    player_threats: [PlayerThreatFacts; 4],
    preparation: &CallCandidatePreparation,
    candidate: &CallCandidateDiagnostic,
    iishanten_forward_metrics: Option<ForwardMetrics>,
) -> Option<PushPullDecision> {
    let evaluation = candidate.post_call_discard.as_ref()?;
    let decide = |post_call_context: &GameContext, legal_actions: &[LegalAction]| {
        decide_push_pull(&push_pull_inputs_from_threat_facts(
            post_call_context,
            player_threats,
            Some(evaluation),
            iishanten_forward_metrics,
            None,
            None,
            legal_actions,
        ))
    };
    let (meld, post_call_tiles) = match preparation {
        CallCandidatePreparation::PostCall(inputs) => {
            return Some(decide(&inputs.post_call_context, &inputs.legal_actions));
        }
        CallCandidatePreparation::TwoShantenCall(inputs) => (&inputs.meld, &inputs.post_call_tiles),
        CallCandidatePreparation::ThreeShantenCall(inputs) => {
            (&inputs.meld, &inputs.post_call_tiles)
        }
        CallCandidatePreparation::Settled | CallCandidatePreparation::Reused(_) => return None,
    };
    let post_call_context =
        ctx.with_own_hand_state(post_call_tiles.clone(), post_call_melds(ctx, meld))?;
    let legal_actions = post_call_legal_dahai_actions(
        post_call_tiles,
        candidate.post_call_forbidden_discards.as_deref()?,
    );
    Some(decide(&post_call_context, &legal_actions))
}

// 既存副露に今回の Chi / Pon を加えた、鳴き後の副露一覧。
fn post_call_melds(ctx: &GameContext, meld: &Meld) -> Vec<Meld> {
    let mut melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
    melds.push(meld.clone());
    melds
}

// Call > Pass の比較そのもの。`eligible` は Call が厳密に高い場合の理由で、現在の向聴数によって
// だけ変わる。同値・どちらかが unknown・反応元不明はすべて鳴かない理由になる。
pub(crate) fn compare_call_pass_self_tsumo_values(
    reaction_source_known: bool,
    call: Option<u64>,
    pass: Option<u64>,
    eligible: CallDecisionReason,
) -> (CallIishantenComparison, CallDecisionReason) {
    if !reaction_source_known {
        return (
            CallIishantenComparison::Unknown,
            CallDecisionReason::ReactionSourceUnknown,
        );
    }
    match (call, pass) {
        (Some(call), Some(pass)) if call > pass => (CallIishantenComparison::CallHigher, eligible),
        (Some(_), Some(_)) => (
            CallIishantenComparison::PassNotLower,
            CallDecisionReason::PassSelfTsumoNotLower,
        ),
        _ => (
            CallIishantenComparison::Unknown,
            CallDecisionReason::IishantenSelfTsumoUnknown,
        ),
    }
}

// reaction 元の次巡から自分の次の自摸までに山から引かれる枚数。観測できない席や自家打牌は
// reaction として不整合なので None のままにする。
pub(crate) fn reaction_draw_distance(ctx: &GameContext) -> Option<u32> {
    let own = ctx.player_id()?;
    let source = ctx.reaction_source_player()?;
    if own >= 4 || source >= 4 || own == source {
        return None;
    }
    Some(u32::from((own + 4 - source) % 4))
}

// Pass 後から流局までの自分の自摸回数。source の次席から順に残り山を配るため、通常打牌後や
// Call 後の floor(remaining / 4) とは最初の自摸位置だけが異なる。
fn pass_own_future_draws(ctx: &GameContext) -> Option<u32> {
    let remaining = ctx.remaining_tiles()?;
    let distance = reaction_draw_distance(ctx)?;
    if remaining < distance {
        Some(0)
    } else {
        Some(1 + (remaining - distance) / 4)
    }
}

// 架空の現在打牌を作らず、現在の13枚を「action 済みで次の自摸を待つ state」として既存
// lookahead へ渡す。
fn pass_expected_self_tsumo_value(
    ctx: &GameContext,
    expected_shanten: i8,
    evaluate: impl FnOnce(
        &bot_logic::LookaheadInputs<'_>,
        &bot_logic::EffectiveAcceptance,
    ) -> Option<u64>,
) -> Option<u64> {
    let fixed_meld_count = ctx.own_fixed_meld_count()?;
    let counts = TileCounts::from_tiles(ctx.hand_tiles().iter().copied());
    let acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
        &counts,
        fixed_meld_count,
        ctx.visible_tiles(),
    );
    if acceptance.current_min_shanten() != expected_shanten {
        return None;
    }

    let valuator = ProductionProspectiveValuator::new_with_hand_state(ctx, ctx.own_melds());
    // Call 側は鳴いた後の production 打牌選択が求めるので、Pass 側も同じ1向聴 continuation の
    // 設定で評価する。片側だけ深度が違うと、比較そのものが尺度の違いを拾ってしまう。
    let inputs = with_production_iishanten_continuation(lookahead_inputs_with_own_future_draws(
        ctx,
        ctx.hand_tiles(),
        &valuator,
        LookaheadDiagnosticScope::None,
        Some(pass_own_future_draws(ctx)?),
    ));
    evaluate(&inputs, &acceptance)
}

fn pass_iishanten_expected_self_tsumo_value(ctx: &GameContext) -> Option<u64> {
    pass_expected_self_tsumo_value(
        ctx,
        CALL_CURRENT_SHANTEN,
        awaiting_draw_expected_self_tsumo_value,
    )
}

pub(crate) fn pass_two_shanten_expected_self_tsumo_value(ctx: &GameContext) -> Option<u64> {
    pass_expected_self_tsumo_value(
        ctx,
        CALL_TWO_SHANTEN_SHANTEN,
        awaiting_draw_two_shanten_expected_self_tsumo_value,
    )
}

/// 鳴かずに次の自摸を待つ現在2向聴 state の、最初のツモで1向聴へ進む枝だけの self-tsumo 寄与。
///
/// production の鳴き判断は読まない。`現在2向聴 → Call → 2向聴のまま` の observation が Progress
/// scope の Pass 側として使う。入力の組み立ては [`pass_two_shanten_expected_self_tsumo_value`] と
/// 共通で、違うのは最初のツモで2向聴を維持する枝を足さないことだけ。
pub(crate) fn pass_two_shanten_progress_self_tsumo_value(ctx: &GameContext) -> Option<u64> {
    pass_expected_self_tsumo_value(
        ctx,
        CALL_TWO_SHANTEN_SHANTEN,
        awaiting_draw_two_shanten_progress_self_tsumo_value,
    )
}

fn pass_three_shanten_progress_self_tsumo_value(ctx: &GameContext) -> Option<u64> {
    pass_expected_self_tsumo_value(
        ctx,
        CALL_THREE_SHANTEN_SHANTEN,
        awaiting_draw_three_shanten_progress_only_self_tsumo_value,
    )
}

// 鳴いても1向聴のままの候補について、鳴かない場合と鳴いた場合の受け入れを並べる。
//
// diagnostics が有効な場合だけ呼ばれる。返り値は成立条件にも候補の選択にも使わない。鳴いた
// 後の向聴・受け入れは本番の打牌評価 `evaluation` が持つ値をそのまま読み、鳴かない場合の受け
// 入れは既存の受け入れ計算へそのまま渡す。どちらもここで数え直さない。
//
// 対象は鳴き後の最良打牌が1向聴のままの候補だけ。テンパイになる候補は既存診断で足り、2向聴から
// の鳴きは尺度が揃わないので対象にしない。
fn iishanten_acceptance_diagnostic(
    ctx: &GameContext,
    counts: &TileCounts,
    current_fixed_meld_count: FixedMeldCount,
    meld: &Meld,
    evaluation: &DiscardEvaluation,
) -> Option<CallIishantenAcceptanceDiagnostic> {
    let post_call_shanten = evaluation.min_shanten_after_discard();
    if post_call_shanten != CALL_CURRENT_SHANTEN {
        return None;
    }

    // 鳴かない場合の受け入れは、現在の副露済み面子数と見え牌をそのまま反映した既存計算。
    let pass_acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
        counts,
        current_fixed_meld_count,
        ctx.visible_tiles(),
    );

    // 役保証の対象は既存副露 + 今回の面子。牌種による役牌判定をこの層で持たない。
    let mut fixed_melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
    fixed_melds.push(meld.clone());

    Some(CallIishantenAcceptanceDiagnostic {
        pass_acceptance_remaining: pass_acceptance.total_remaining(),
        pass_acceptance_type_count: pass_acceptance.tiles.len(),
        post_call_shanten,
        post_call_acceptance_remaining: evaluation.acceptance_total_remaining(),
        post_call_acceptance_type_count: evaluation.acceptance_type_count(),
        fixed_melds_guarantee_yaku: fixed_melds_guarantee_yaku(
            &fixed_melds,
            damaten_baseline_context(ctx),
        ),
    })
}

// 鳴き後テンパイが確定してからの条件を評価する。待ち枚数・ロン可否・役の順に見る。
fn evaluate_post_call_conditions(
    ctx: &GameContext,
    meld: &Meld,
    post_call_tiles: &[TileId],
    evaluation: &DiscardEvaluation,
    wait: &TenpaiWaitAvailability,
    candidate: &mut CallCandidateDiagnostic,
) -> CallDecisionReason {
    if wait.tsumo_remaining == 0 {
        return CallDecisionReason::NoLiveAcceptance;
    }
    if wait.tsumo_remaining < CALL_MIN_LIVE_WAIT_REMAINING {
        return CallDecisionReason::TooFewLiveWaits;
    }
    // フリテンとロン可否 unknown はどちらも鳴かない。非フリテンだと推測しない。
    if wait.can_ron() != Some(true) {
        return CallDecisionReason::CannotRon;
    }

    let Some(wait_yaku) = post_call_wait_yaku(ctx, meld, post_call_tiles, evaluation, wait) else {
        return CallDecisionReason::HandValueUnknown;
    };
    let reason = live_wait_yaku_reason(&wait_yaku);
    candidate.post_call_wait_yaku = Some(wait_yaku);
    reason
}

// 鳴き後テンパイの和了牌の物理牌ごとに、既存 HandValue でロン和了できるかを評価する。
//
// 待ち牌種と残枚数は鳴き後の打牌評価が持つ受け入れがそのまま source of truth で、ここで待ちを
// 数え直さない。赤5 / 黒5 の分割も既存の physical variant 規則に任せる。和了状況は既存の
// hypothetical ロン baseline をそのまま使い、鳴き判断専用の和了状況を組み立てない。
//
// 打牌後の手牌を組み立てられない場合と完成手を解析できない場合は None。役ありだと推測しない。
fn post_call_wait_yaku(
    ctx: &GameContext,
    meld: &Meld,
    post_call_tiles: &[TileId],
    evaluation: &DiscardEvaluation,
    wait: &TenpaiWaitAvailability,
) -> Option<Vec<CallWaitYakuDiagnostic>> {
    let (_, concealed_tiles) = split_discarded_tile(post_call_tiles.to_vec(), evaluation)?;

    let mut melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
    melds.push(meld.clone());

    let hands = tenpai_completed_hands(
        &concealed_tiles,
        &melds,
        &evaluation.acceptance_after_discard,
        Some(wait),
        ctx.visible_tiles(),
    )
    .ok()?;
    let profile = evaluate_tenpai_hand_value(
        &hands,
        damaten_baseline_context(ctx),
        ctx.dora_indicators(),
        None,
    );

    Some(
        profile
            .waits()
            .iter()
            .flat_map(|wait| wait.winning_tiles())
            .map(|winning_tile| CallWaitYakuDiagnostic {
                winning_tile: winning_tile.winning_tile(),
                remaining: winning_tile.remaining(),
                yaku: wait_yaku(winning_tile.outcome()),
            })
            .collect(),
    )
}

// 既存の手牌価値の結果を役の有無へ畳む。役なしと確定できない理由を潰さずに区別して持つ。
fn wait_yaku(outcome: Result<&HandValueOutcome<'_>, HandValueError>) -> CallWaitYaku {
    match outcome {
        Ok(HandValueOutcome::Known(_)) => CallWaitYaku::Present,
        Ok(HandValueOutcome::NoCandidate) => CallWaitYaku::Absent,
        Ok(HandValueOutcome::IndeterminateBonusHan) | Err(_) => CallWaitYaku::Unknown,
    }
}

// 残枚数 > 0 の variant だけを見て役の結論を出す。役なしが1つでもあれば片和了として鳴かない。
// 確定できない variant は役ありだと推測しない。残枚数 0 の variant は現在ロンできないので
// 判定対象にしない。
fn live_wait_yaku_reason(waits: &[CallWaitYakuDiagnostic]) -> CallDecisionReason {
    let live = || waits.iter().filter(|wait| wait.is_live());

    if live().any(|wait| wait.yaku == CallWaitYaku::Absent) {
        return CallDecisionReason::YakuMissing;
    }
    if live().any(|wait| wait.yaku == CallWaitYaku::Unknown) {
        return CallDecisionReason::HandValueUnknown;
    }
    CallDecisionReason::EligibleTenpai
}

// 鳴き後の副露面子と concealed hand を組み立てる。
//
// consumed は牌種単位で減らすのではなく物理牌 ID で除去するため、赤5を含む鳴きでも semantics を
// 保つ。枚数が2枚でない・手牌に無い・同じ物理牌が重複している場合は None。面子の形の検証は
// 既存 Meld::shape() が source of truth で、Chi なのに連続3牌でない・Pon なのに同一牌でない
// 場合も None になる。
fn call_meld_and_concealed_tiles(
    hand_tiles: &[TileId],
    kind: CallKind,
    tile: TileId,
    consumed: &[TileId],
) -> Option<(Meld, Vec<TileId>)> {
    if consumed.len() != CALL_CONSUMED_TILE_COUNT {
        return None;
    }

    let mut remaining = hand_tiles.to_vec();
    for consumed_tile in consumed {
        let position = remaining.iter().position(|held| held == consumed_tile)?;
        remaining.remove(position);
    }

    let mut tiles = Vec::with_capacity(consumed.len() + 1);
    tiles.push(tile);
    tiles.extend_from_slice(consumed);

    let meld = Meld::new(kind.meld_kind(), tiles, Some(tile));
    meld.shape()?;
    Some((meld, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use bot_logic::{
        DiscardLookaheadDiagnostic, DrawLookaheadDiagnostic, DrawTransition, EffectiveAcceptance,
        MeldShape, ProspectiveTenpai, ProspectiveTenpaiValuator, SelfTsumoHorizon,
        ThreeShantenSearchStats, forward_metrics_with_lookahead_for_candidate,
        prospective_branch_root_tiles, prospective_branch_tiles_after_draw,
    };

    use crate::discard_selection::{
        PostCallIishantenSelection, PostCallTwoShantenSelection, lookahead_inputs,
        with_production_three_shanten_continuation,
    };
    use crate::prospective_value::{
        continuation_han_verdict, han_floor_counter, scored_han_verdict, tenpai_value_memo_counter,
    };
    use crate::push_pull::PushPullReason;

    use crate::decision_timing::{CallCandidateDuration, CallDecisionDurations};

    fn tile(value: u8) -> TileId {
        TileId::new(value).unwrap()
    }

    fn tiles(values: &[u8]) -> Vec<TileId> {
        values.iter().map(|&value| tile(value)).collect()
    }

    // 速度優先 policy を評価しなかった候補の判断材料。
    fn not_applied_speed() -> CallTwoShantenSpeedDiagnostic {
        CallTwoShantenSpeedDiagnostic {
            own_future_draws: None,
            han: None,
            overrides_pass: false,
        }
    }

    fn wait(winning_tile: u8, remaining: u8, yaku: CallWaitYaku) -> CallWaitYakuDiagnostic {
        CallWaitYakuDiagnostic {
            winning_tile: tile(winning_tile),
            remaining,
            yaku,
        }
    }

    // 他家 (player 1) の打牌へ反応する局面。東場東家・リーチ者なし・副露なし・ツモ牌なしで、
    // 鳴き判断が読む fact だけを組み立てる。
    fn reaction_context(hand: &[u8], target: u8) -> GameContext {
        reaction_context_with_reach(hand, target, [false; 4])
    }

    // 同じ reaction 局面で、リーチ者だけを差し替える。
    fn reaction_context_with_reach(hand: &[u8], target: u8, reached: [bool; 4]) -> GameContext {
        let hand_tiles = tiles(hand);
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));

        GameContext::from_parts_with_melds(
            None,
            hand_tiles,
            vec![],
            TileType::new(EAST),
            TileType::new(EAST),
            visible,
            Some(0),
            Some(0),
            [vec![], vec![tile(target)], vec![], vec![]],
            reached,
            Default::default(),
        )
        // 実際の client が局開始で確定させる値。unknown だと全ての鳴きがロン可否不明で落ちる。
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
    }

    fn valued_reaction_context(
        hand: &[u8],
        target: u8,
        source: u8,
        remaining_tiles: u32,
    ) -> GameContext {
        reaction_context(hand, target)
            .with_reaction_source_player(Some(source))
            .with_table_state_facts(crate::context::TableStateFacts {
                remaining_tiles: Some(remaining_tiles),
                ..Default::default()
            })
    }

    // 既存2副露を持つ小さい局面。残る concealed hand を小さくして、2向聴 Full の focused
    // test が不要に大きな探索にならないようにする。
    fn valued_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        source: Option<u8>,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        // 白 Pon + 發 Pon。どの完成形にも役があるので、比較が役の有無で動かない。
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Pon, tiles(&[128, 129, 130]), Some(tile(128))),
        ];
        two_shanten_reaction_context_with_melds(hand, melds, target, source, remaining_tiles)
    }

    // 同じ2向聴 reaction 局面で、既存副露だけを差し替える。
    fn two_shanten_reaction_context_with_melds(
        hand: &[u8],
        melds: Vec<Meld>,
        target: u8,
        source: Option<u8>,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        let hand_tiles = tiles(hand);
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));
        visible.extend(melds.iter().flat_map(|meld| meld.tiles().iter().copied()));

        GameContext::from_parts_with_melds(
            None,
            hand_tiles,
            vec![],
            TileType::new(EAST),
            TileType::new(EAST),
            visible,
            Some(0),
            Some(0),
            [vec![], vec![tile(target)], vec![], vec![]],
            [false; 4],
            [melds, vec![], vec![], vec![]],
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
        .with_reaction_source_player(source)
        .with_table_state_facts(crate::context::TableStateFacts {
            remaining_tiles,
            ..Default::default()
        })
    }

    #[test]
    fn an_iishanten_call_with_a_higher_expected_self_tsumo_value_is_selected() {
        // 門前のまま進めた方が手変わりの経路を多く持つため、Call が上回るのは残り自摸機会が
        // 少ない局面。1向聴 continuation の深度はどちらの側も production のものを使う。
        // 残り 36 枚は production の soft horizon で Call / Pass とも自摸機会 3 回になる。
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 36);
        let (decision, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(
            candidate.reason,
            CallDecisionReason::EligibleIishantenSelfTsumo
        );
        assert!(candidate.eligible);
        assert_eq!(decision.selected, Some(action));
        assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);
        assert_eq!(comparison.pass_expected_self_tsumo_value, Some(20_956_462));
        assert_eq!(comparison.call_expected_self_tsumo_value, Some(21_531_497));
        assert!(
            comparison.call_expected_self_tsumo_value > comparison.pass_expected_self_tsumo_value
        );
        // forward-selected discard is legal after the call; the forbidden called tile is excluded.
        assert!(
            !candidate
                .post_call_forbidden_discards
                .as_ref()
                .unwrap()
                .contains(&candidate.post_call_discard.as_ref().unwrap().discard)
        );
        assert_eq!(candidate.post_call_fixed_meld_count, FixedMeldCount::new(1));

        let meld = Meld::new(
            MeldKind::Pon,
            tiles(&[
                IISHANTEN_PON_TARGET,
                IISHANTEN_PON_CONSUMED[0],
                IISHANTEN_PON_CONSUMED[1],
            ]),
            Some(tile(IISHANTEN_PON_TARGET)),
        );
        let melds = [meld];
        let post_call = ProductionProspectiveValuator::new_with_hand_state(&ctx, Some(&melds));
        assert_eq!(
            post_call.fixed_meld_count(),
            FixedMeldCount::new(1).unwrap()
        );
        assert!(!post_call.reach_legal());
    }

    #[test]
    fn an_iishanten_pass_with_a_higher_expected_self_tsumo_value_is_kept() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63);
        let (decision, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
        assert!(
            comparison.pass_expected_self_tsumo_value > comparison.call_expected_self_tsumo_value
        );
    }

    #[test]
    fn the_call_and_pass_values_both_use_the_production_iishanten_continuation_depth() {
        // Call 側は鳴いた後の production 打牌選択、Pass 側は同じ設定を適用した継続評価が求める。
        // どちらも production の手変わり深度で、片側だけ旧 shallow へ戻ると値が動く局面を使う
        // (手変わり1回までの旧設定では pass 176.885897 / call 165.908530 になる)。
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        // 値は流局までの horizon で固定する。
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63)
            .with_self_tsumo_horizon(SelfTsumoHorizon::UNTIL_RYUKYOKU);
        let (_, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(
            comparison.pass_expected_self_tsumo_value,
            Some(284_875_812),
            "pass",
        );
        assert_eq!(
            comparison.call_expected_self_tsumo_value,
            Some(239_138_199),
            "call",
        );
    }

    #[test]
    fn the_call_and_pass_share_the_soft_horizon() {
        // Call 後の floor(remaining / 4) と Pass 後の 1 + (remaining - distance) / 4 のどちらにも
        // 同じ soft horizon が掛かる。horizon 12 で残り 63 枚の局面は、どちらの側も horizon 18
        // で残り 39 枚の局面と同じ自摸機会 (Call 9 / Pass 10) になり、値も一致する。
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let values = |remaining_tiles, horizon: SelfTsumoHorizon| {
            let ctx = valued_reaction_context(
                &IISHANTEN_PON_HAND,
                IISHANTEN_PON_TARGET,
                1,
                remaining_tiles,
            )
            .with_self_tsumo_horizon(horizon);
            let (_, candidate) = single_candidate(&ctx, &action, false);
            let comparison = candidate.iishanten_self_tsumo.expect("comparison");
            (
                comparison.call_expected_self_tsumo_value,
                comparison.pass_expected_self_tsumo_value,
            )
        };
        let horizon = |horizon_turn| SelfTsumoHorizon {
            horizon_turn,
            late_min_future_draws: 2,
        };

        let production = values(63, SelfTsumoHorizon::PRODUCTION);
        assert_eq!(production, values(39, SelfTsumoHorizon::UNTIL_RYUKYOKU));
        assert_eq!(
            values(63, horizon(18)),
            values(63, SelfTsumoHorizon::UNTIL_RYUKYOKU)
        );
        assert_eq!(
            values(63, horizon(18)),
            (Some(239_138_199), Some(284_875_812))
        );

        let by_turn = [12, 14, 16, 18].map(|turn| values(63, horizon(turn)));
        assert_eq!(by_turn[0], production);
        for pair in by_turn.windows(2) {
            assert!(pair[0].0 < pair[1].0, "call {pair:?}");
            assert!(pair[0].1 < pair[1].1, "pass {pair:?}");
        }
    }

    #[test]
    fn the_two_shanten_call_and_pass_share_the_soft_horizon() {
        // 2向聴 Pass の Full evaluation と、Call 後の1向聴 continuation も同じ horizon を使う。
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let values = |remaining_tiles, horizon| {
            let ctx = valued_two_shanten_reaction_context(
                &TWO_SHANTEN_CALL_PON_HAND,
                TWO_SHANTEN_CALL_PON_TARGET,
                Some(1),
                Some(remaining_tiles),
            )
            .with_self_tsumo_horizon(horizon);
            let (_, candidate) = single_candidate(&ctx, &action, false);
            let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");
            (
                comparison.call_expected_self_tsumo_value,
                comparison.pass_expected_self_tsumo_value,
            )
        };

        assert_eq!(
            values(56, SelfTsumoHorizon::PRODUCTION),
            values(32, SelfTsumoHorizon::UNTIL_RYUKYOKU)
        );
        assert_eq!(
            values(32, SelfTsumoHorizon::UNTIL_RYUKYOKU),
            (Some(4_103_395_595), Some(189_935_840))
        );
    }

    #[test]
    fn equal_production_iishanten_values_keep_the_pass() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 0);
        let (decision, candidate) = single_candidate(&ctx, &action, false);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(comparison.pass_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.call_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn an_unknown_production_iishanten_value_keeps_the_pass() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET)
            .with_reaction_source_player(Some(1));
        let (decision, candidate) = single_candidate(&ctx, &action, false);
        let comparison = candidate.iishanten_self_tsumo.expect("comparison");

        assert_eq!(comparison.pass_expected_self_tsumo_value, None);
        assert_eq!(comparison.call_expected_self_tsumo_value, None);
        assert_eq!(comparison.comparison, CallIishantenComparison::Unknown);
        assert_eq!(
            candidate.reason,
            CallDecisionReason::IishantenSelfTsumoUnknown
        );
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn equal_and_unknown_iishanten_values_keep_the_pass() {
        assert_eq!(
            compare_call_pass_self_tsumo_values(
                true,
                Some(100),
                Some(100),
                CallDecisionReason::EligibleIishantenSelfTsumo,
            ),
            (
                CallIishantenComparison::PassNotLower,
                CallDecisionReason::PassSelfTsumoNotLower
            )
        );
        for values in [(None, Some(100)), (Some(100), None), (None, None)] {
            assert_eq!(
                compare_call_pass_self_tsumo_values(
                    true,
                    values.0,
                    values.1,
                    CallDecisionReason::EligibleIishantenSelfTsumo,
                ),
                (
                    CallIishantenComparison::Unknown,
                    CallDecisionReason::IishantenSelfTsumoUnknown
                )
            );
        }
    }

    #[test]
    fn an_unknown_reaction_source_is_not_inferred() {
        assert_eq!(
            reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET).reaction_source_player(),
            None
        );
        assert_eq!(
            compare_call_pass_self_tsumo_values(
                false,
                Some(200),
                Some(100),
                CallDecisionReason::EligibleIishantenSelfTsumo,
            ),
            (
                CallIishantenComparison::Unknown,
                CallDecisionReason::ReactionSourceUnknown
            )
        );
    }

    #[test]
    fn pass_draw_count_uses_the_observed_source_position() {
        for (remaining, expected) in [(60, 15), (63, 16)] {
            let context =
                valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, remaining);
            assert_eq!(pass_own_future_draws(&context), Some(expected));
        }
        assert_eq!(
            pass_own_future_draws(&reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET)),
            None
        );
    }

    fn pon_action(target: u8, consumed: &[u8]) -> LegalAction {
        LegalAction::Pon {
            tile: tile(target),
            consumed: tiles(consumed),
        }
    }

    fn chi_action(target: u8, consumed: &[u8]) -> LegalAction {
        LegalAction::Chi {
            tile: tile(target),
            consumed: tiles(consumed),
        }
    }

    fn single_candidate(
        ctx: &GameContext,
        action: &LegalAction,
        collect_observations: bool,
    ) -> (CallDecisionDiagnostic, CallCandidateDiagnostic) {
        let decision = evaluate_call_decision(
            ctx,
            &[action.clone(), LegalAction::None],
            collect_observations,
            &mut CallDecisionTimer::disabled(),
        )
        .expect("evaluated");
        assert_eq!(decision.candidates.len(), 1);
        let candidate = decision.candidates[0].clone();
        (decision, candidate)
    }

    const EAST: u8 = 27;

    // 234567m 68p 24s E FF の一向聴。FF を Pon して E を切っても一向聴のままで、雀頭が無い
    // 3面子2搭子になる。
    const IISHANTEN_PON_HAND: [u8; 13] = [4, 8, 12, 17, 20, 24, 56, 64, 76, 84, 108, 128, 129];
    const IISHANTEN_PON_TARGET: u8 = 130;
    const IISHANTEN_PON_CONSUMED: [u8; 2] = [128, 129];

    // IISHANTEN_PON_HAND と同じ手牌で 5m を Chi する局面。3m4m / 4m6m / 6m7m の3通りが
    // semantic に別の鳴きになるので、1局面で複数の unique な Call 候補を並べられる。
    const IISHANTEN_CHI_GROUP_HAND: [u8; 13] = IISHANTEN_PON_HAND;
    const IISHANTEN_CHI_GROUP_TARGET: u8 = 18;
    const IISHANTEN_CHI_GROUP_CONSUMED: [[u8; 2]; 3] = [[8, 12], [12, 20], [20, 24]];

    fn iishanten_chi_group_actions() -> Vec<LegalAction> {
        IISHANTEN_CHI_GROUP_CONSUMED
            .iter()
            .map(|consumed| chi_action(IISHANTEN_CHI_GROUP_TARGET, consumed))
            .chain(std::iter::once(LegalAction::None))
            .collect()
    }

    // 345m 789m 68p 24s E FF の一向聴。4m5m で 3m を Chi して E を切っても一向聴のまま。
    const IISHANTEN_CHI_HAND: [u8; 13] = [8, 12, 17, 24, 28, 32, 56, 64, 76, 84, 108, 128, 129];
    const IISHANTEN_CHI_TARGET: u8 = 9;
    const IISHANTEN_CHI_CONSUMED: [u8; 2] = [12, 17];

    // 123456m 55p 78s N PP の一向聴。PP を Pon して N を切ると即テンパイ。
    const TENPAI_PON_HAND: [u8; 13] = [0, 4, 8, 12, 17, 20, 53, 54, 96, 100, 120, 124, 125];
    const TENPAI_PON_TARGET: u8 = 126;
    const TENPAI_PON_CONSUMED: [u8; 2] = [124, 125];

    // 234m 68m 68p 24s E C FF の二向聴。
    const RYANSHANTEN_PON_HAND: [u8; 13] = [4, 8, 12, 20, 28, 56, 64, 76, 84, 108, 132, 128, 129];

    // 既存2副露 + CC 55p E S W の2向聴。C を Pon した後、最良打牌で1向聴になる。
    const TWO_SHANTEN_CALL_PON_HAND: [u8; 7] = [132, 133, 52, 53, 108, 112, 116];
    const TWO_SHANTEN_CALL_PON_TARGET: u8 = 134;
    const TWO_SHANTEN_CALL_PON_CONSUMED: [u8; 2] = [132, 133];

    // 既存2副露 + 68m 55p E S W の2向聴。6m8m で7mを Chi した後、Pon と同じ
    // post-call concealed hand になり、共通の評価 path で1向聴になる。
    const TWO_SHANTEN_CALL_CHI_HAND: [u8; 7] = [20, 28, 52, 53, 108, 112, 116];
    const TWO_SHANTEN_CALL_CHI_TARGET: u8 = 24;
    const TWO_SHANTEN_CALL_CHI_CONSUMED: [u8; 2] = [20, 28];

    #[test]
    fn two_shanten_chi_and_pon_take_the_same_call_pass_value_path() {
        let cases = [
            (
                CallKind::Pon,
                &TWO_SHANTEN_CALL_PON_HAND[..],
                pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                TWO_SHANTEN_CALL_PON_TARGET,
                1,
            ),
            (
                CallKind::Chi,
                &TWO_SHANTEN_CALL_CHI_HAND[..],
                chi_action(TWO_SHANTEN_CALL_CHI_TARGET, &TWO_SHANTEN_CALL_CHI_CONSUMED),
                TWO_SHANTEN_CALL_CHI_TARGET,
                3,
            ),
        ];

        for (kind, hand, action, target, source) in cases {
            let ctx = valued_two_shanten_reaction_context(hand, target, Some(source), Some(32));
            let (decision, candidate) = single_candidate(&ctx, &action, true);
            let comparison = candidate
                .two_shanten_self_tsumo
                .expect("2向聴 Call / Pass 比較対象");

            assert_eq!(candidate.action, action);
            assert_eq!(candidate.kind, kind);
            assert_eq!(candidate.current_shanten, Some(CALL_TWO_SHANTEN_SHANTEN));
            assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));
            assert_eq!(
                candidate.reason,
                CallDecisionReason::EligibleTwoShantenSelfTsumo
            );
            assert!(candidate.eligible);
            assert_eq!(decision.selected.as_ref(), Some(&action));
            assert!(
                comparison.call_expected_self_tsumo_value
                    > comparison.pass_expected_self_tsumo_value
            );
            assert_eq!(comparison.reaction_source_player, Some(source));
            assert_eq!(
                comparison.pass_evaluation,
                CallTwoShantenPassEvaluation::Full
            );
            assert!(comparison.pass_expected_self_tsumo_value.is_some());
            assert!(comparison.call_expected_self_tsumo_value.is_some());
            assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);

            // 喰い替え禁止牌を除いた既存候補だけが comparator に渡される。
            let forbidden = candidate
                .post_call_forbidden_discards
                .as_ref()
                .expect("喰い替え制約を評価済み");
            assert!(!forbidden.is_empty());
            assert!(!forbidden.contains(&candidate.post_call_discard.as_ref().unwrap().discard));
            let (_, called_tile, consumed) = normalize_call(&action).expect("Chi / Pon");
            let (_, post_call_tiles) =
                call_meld_and_concealed_tiles(ctx.hand_tiles(), kind, called_tile, consumed)
                    .expect("合法な鳴き");
            let evaluations = post_call_discard_evaluations(
                &ctx,
                &post_call_tiles,
                candidate.post_call_fixed_meld_count.unwrap(),
                forbidden,
            );
            assert!(
                evaluations
                    .iter()
                    .all(|evaluation| !forbidden.contains(&evaluation.discard))
            );
        }
    }

    // 既存2副露 + CC 55p E S W の2向聴。TWO_SHANTEN_CALL_PON_HAND と同じ形だが、赤5p を
    // 黒5p に置き換えて赤ドラ1翻が乗らないようにしたもの。
    const LOW_VALUE_TWO_SHANTEN_CALL_PON_HAND: [u8; 7] = [132, 133, 53, 54, 108, 112, 116];

    // 白 Pon + 234m Chi の2副露。白 と 中 の役牌2翻だけで、速度優先 policy の要求翻数に
    // 届かない。向聴数は副露済み面子数だけで決まるので、手牌側の評価は大三元の fixture と同じ。
    fn low_value_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Chi, tiles(&[4, 8, 12]), Some(tile(4))),
        ];
        two_shanten_reaction_context_with_melds(hand, melds, target, Some(1), remaining_tiles)
    }

    // 同じ2向聴 reaction 局面で、reaction 元の他家だけがリーチしている場合。
    fn reached_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Pon, tiles(&[128, 129, 130]), Some(tile(128))),
        ];
        let hand_tiles = tiles(hand);
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));
        visible.extend(melds.iter().flat_map(|meld| meld.tiles().iter().copied()));

        GameContext::from_parts_with_melds(
            None,
            hand_tiles,
            vec![],
            TileType::new(EAST),
            TileType::new(EAST),
            visible,
            Some(0),
            Some(0),
            [vec![], vec![tile(target)], vec![], vec![]],
            [false, true, false, false],
            [melds, vec![], vec![], vec![]],
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
        .with_reaction_source_player(Some(1))
        .with_table_state_facts(crate::context::TableStateFacts {
            remaining_tiles,
            ..Default::default()
        })
    }

    fn speed_facts(
        own_future_draws: Option<u32>,
        han: Option<ProspectiveHanVerdict>,
    ) -> CallTwoShantenSpeedDiagnostic {
        CallTwoShantenSpeedDiagnostic {
            own_future_draws,
            han,
            overrides_pass: false,
        }
    }

    // 速度優先 policy の条件をすべて満たす判断材料。
    fn satisfied_speed_facts() -> CallTwoShantenSpeedDiagnostic {
        speed_facts(
            Some(CALL_TWO_SHANTEN_SPEED_MIN_DRAWS),
            Some(ProspectiveHanVerdict::AtLeast),
        )
    }

    // 鳴き後1向聴まで評価が進んだ2向聴候補。Call / Pass の値と速度優先 policy の材料だけを
    // 差し替えて、比較と上書きの semantics だけを見る。
    fn two_shanten_speed_candidate(
        call_value: Option<u64>,
        speed: CallTwoShantenSpeedDiagnostic,
    ) -> CallCandidateDiagnostic {
        CallCandidateDiagnostic {
            current_shanten: Some(CALL_TWO_SHANTEN_SHANTEN),
            two_shanten_self_tsumo: Some(CallTwoShantenSelfTsumoDiagnostic {
                reaction_source_player: Some(1),
                pass_evaluation: CallTwoShantenPassEvaluation::Full,
                pass_expected_self_tsumo_value: None,
                call_expected_self_tsumo_value: call_value,
                comparison: CallIishantenComparison::Unknown,
                speed,
            }),
            reason: CallDecisionReason::IishantenSelfTsumoUnknown,
            ..new_call_candidate(
                &pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                CallKind::Pon,
            )
        }
    }

    // 重ねて評価済みの Pass 値を渡して policy を1回適用する。Pass の再評価は行わない。
    fn applied_two_shanten_policy(
        ctx: &GameContext,
        call_value: Option<u64>,
        pass_value: Option<u64>,
        speed: CallTwoShantenSpeedDiagnostic,
    ) -> CallCandidateDiagnostic {
        let mut candidates = vec![two_shanten_speed_candidate(call_value, speed)];
        apply_two_shanten_self_tsumo_policy(
            ctx,
            &mut candidates,
            Some(PassSelfTsumoContinuation {
                kind: PassSelfTsumoContinuationKind::TwoShanten,
                value: pass_value,
                elapsed: Duration::ZERO,
            }),
            &mut CallDecisionTimer::disabled(),
        );
        candidates.remove(0)
    }

    #[test]
    fn a_high_value_two_shanten_call_is_taken_even_when_the_pass_value_is_not_lower() {
        // continuation の全テンパイが3翻以上確定 + 残り自摸10回以上の 2向聴 → 1向聴 は、Call の
        // ExpectedSelfTsumoValue が Pass 以下でも速度優先で鳴く。同値と Call 側が低い場合の
        // 両方を含む。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );

        for call in [Some(100), Some(99)] {
            let candidate =
                applied_two_shanten_policy(&ctx, call, Some(100), satisfied_speed_facts());
            let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");

            assert_eq!(
                candidate.reason,
                CallDecisionReason::EligibleTwoShantenSpeed
            );
            assert!(candidate.eligible);
            // 上書きするのは理由だけで、値比較そのものは Pass のまま診断へ残す。
            assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
            assert!(comparison.speed.overrides_pass);
            assert!(comparison.speed.is_satisfied());
            assert_eq!(
                select_eligible_candidate(std::slice::from_ref(&candidate)),
                Some(0),
                "{candidate:?}"
            );
        }
    }

    #[test]
    fn a_two_shanten_call_below_the_required_han_keeps_the_value_comparison() {
        // 2翻以下・翻数 unknown・翻数を評価していない候補はどれも従来どおり Pass。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );

        for han in [
            Some(ProspectiveHanVerdict::Below),
            Some(ProspectiveHanVerdict::Unknown),
            None,
        ] {
            let speed = speed_facts(Some(CALL_TWO_SHANTEN_SPEED_MIN_DRAWS), han);
            let candidate = applied_two_shanten_policy(&ctx, Some(100), Some(100), speed);
            let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");

            assert_eq!(
                candidate.reason,
                CallDecisionReason::PassSelfTsumoNotLower,
                "{han:?}"
            );
            assert!(!candidate.eligible, "{han:?}");
            assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
            assert!(!comparison.speed.overrides_pass, "{han:?}");
            assert!(!comparison.speed.is_satisfied(), "{han:?}");
            assert_eq!(
                select_eligible_candidate(std::slice::from_ref(&candidate)),
                None,
                "{han:?}"
            );
        }
    }

    #[test]
    fn a_two_shanten_call_with_too_few_remaining_draws_keeps_the_value_comparison() {
        // 残り自摸9回以下と、残り自摸数を確定できない局面はどちらも速度優先 policy を適用せず
        // 従来の比較へ戻す。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );

        for draws in [Some(CALL_TWO_SHANTEN_SPEED_MIN_DRAWS - 1), Some(0), None] {
            let speed = speed_facts(draws, Some(ProspectiveHanVerdict::AtLeast));
            let candidate = applied_two_shanten_policy(&ctx, Some(100), Some(100), speed);
            let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");

            assert_eq!(
                candidate.reason,
                CallDecisionReason::PassSelfTsumoNotLower,
                "{draws:?}"
            );
            assert!(!candidate.eligible, "{draws:?}");
            assert!(!comparison.speed.overrides_pass, "{draws:?}");
            assert!(!comparison.speed.is_satisfied(), "{draws:?}");
        }
    }

    #[test]
    fn the_two_shanten_speed_policy_overrides_only_the_value_comparison() {
        // 上書きするのは値比較が Pass と結論した場合だけ。Call が既に高い場合は従来の理由の
        // まま、値 unknown と反応元不明は速度優先 policy でも鳴かない。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );

        let call_higher =
            applied_two_shanten_policy(&ctx, Some(101), Some(100), satisfied_speed_facts());
        assert_eq!(
            call_higher.reason,
            CallDecisionReason::EligibleTwoShantenSelfTsumo
        );
        assert!(call_higher.eligible);
        assert!(
            !call_higher
                .two_shanten_self_tsumo
                .expect("比較対象")
                .speed
                .overrides_pass
        );

        let unknown_value =
            applied_two_shanten_policy(&ctx, None, Some(100), satisfied_speed_facts());
        assert_eq!(
            unknown_value.reason,
            CallDecisionReason::IishantenSelfTsumoUnknown
        );
        assert!(!unknown_value.eligible);
        assert!(
            !unknown_value
                .two_shanten_self_tsumo
                .expect("比較対象")
                .speed
                .overrides_pass
        );

        let unknown_source = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            None,
            Some(40),
        );
        let candidate = applied_two_shanten_policy(
            &unknown_source,
            Some(100),
            Some(100),
            satisfied_speed_facts(),
        );
        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
        assert!(!candidate.eligible);
        assert!(
            !candidate
                .two_shanten_self_tsumo
                .expect("比較対象")
                .speed
                .overrides_pass
        );
    }

    // 白 Pon + 1m Pon の2副露。中 を Pon すると副露がすべて刻子になるので、鳴き後1向聴から次の
    // Progress ツモで直接到達するテンパイはどれも対々和 + 役牌2つで要求翻数以上になる。一方、
    // 手変わりを経由して順子の形へ組み替えた先のテンパイでは対々和が消えて役牌2翻だけになる。
    fn toitoi_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Pon, tiles(&[0, 1, 2]), Some(tile(0))),
        ];
        two_shanten_reaction_context_with_melds(hand, melds, target, Some(1), remaining_tiles)
    }

    // 鳴き後1向聴の打牌選択を production と同じ入力で組み立て直すための材料。
    struct PostCallIishantenCase {
        ctx: GameContext,
        melds: Vec<Meld>,
        tiles: Vec<TileId>,
        evaluations: Vec<DiscardEvaluation>,
    }

    fn post_call_iishanten_case(ctx: GameContext, action: &LegalAction) -> PostCallIishantenCase {
        let (kind, called_tile, consumed) = normalize_call(action).expect("Chi / Pon");
        let (meld, tiles) =
            call_meld_and_concealed_tiles(ctx.hand_tiles(), kind, called_tile, consumed)
                .expect("合法な鳴き");
        let forbidden = forbidden_discards_after_call(&meld);
        let fixed_meld_count =
            FixedMeldCount::new(ctx.own_fixed_meld_count().expect("副露数").get() + 1)
                .expect("上限内");
        let evaluations = post_call_discard_evaluations(&ctx, &tiles, fixed_meld_count, &forbidden);
        let mut melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
        melds.push(meld);
        PostCallIishantenCase {
            ctx,
            melds,
            tiles,
            evaluations,
        }
    }

    impl PostCallIishantenCase {
        fn selection(&self, required_han: Option<u8>) -> PostCallIishantenSelection {
            select_best_iishanten_post_call_discard(
                &self.ctx,
                &self.tiles,
                &self.melds,
                &self.evaluations,
                required_han,
            )
            .expect("鳴き後の打牌を選べる")
        }

        fn valuator(&self) -> ProductionProspectiveValuator<'_> {
            ProductionProspectiveValuator::new_with_hand_state(&self.ctx, Some(&self.melds))
                .collecting_han_floor(true)
        }
    }

    // production の鳴き後打牌選択が使うのと同じ入力で、選んだ候補の枝を組み立て直す。評価器の
    // memo には production の探索と同じ terminal の打点が載る。
    fn production_post_call_lookahead<'a>(
        ctx: &'a GameContext,
        tiles: &'a [TileId],
        valuator: &'a ProductionProspectiveValuator<'a>,
        evaluation: &DiscardEvaluation,
    ) -> DiscardLookaheadDiagnostic {
        let inputs = with_production_iishanten_continuation(lookahead_inputs(
            ctx,
            tiles,
            valuator,
            LookaheadDiagnosticScope::None,
        ));
        forward_metrics_with_lookahead_for_candidate(&inputs, evaluation).1
    }

    // 探索済みの枝から、次の Progress ツモで直接到達するテンパイだけを取り出す。scope を広げる
    // 前の速度優先 policy が見ていた範囲そのもので、広げた差を test から観測するために使う。
    fn direct_progress_terminals<'a>(
        tiles: &[TileId],
        evaluation: &DiscardEvaluation,
        candidate: &'a DiscardLookaheadDiagnostic,
    ) -> Vec<(Vec<TileId>, Vec<TileId>, &'a EffectiveAcceptance)> {
        let (concealed_tiles, discarded_tiles) = prospective_branch_root_tiles(tiles, evaluation)
            .expect("打牌後の物理牌を組み立てられる");
        candidate
            .draws_with(DrawTransition::Progress)
            .flat_map(|draw| draw.variants.iter())
            .filter(|variant| variant.remaining > 0)
            .filter_map(|variant| {
                let next = variant.next_discard.as_ref()?;
                let (concealed, discarded) = prospective_branch_tiles_after_draw(
                    &concealed_tiles,
                    &discarded_tiles,
                    variant.drawn_tile,
                    next,
                )?;
                Some((concealed, discarded, &next.acceptance_after_discard))
            })
            .collect()
    }

    // 手変わりを経由してから到達する terminal の件数。判定が直接到達分だけを見ていないことを
    // 確かめるために数える。
    fn same_shanten_terminal_count(candidate: &DiscardLookaheadDiagnostic) -> usize {
        fn count(draws: &[DrawLookaheadDiagnostic]) -> usize {
            draws
                .iter()
                .flat_map(|draw| {
                    draw.variants
                        .iter()
                        .map(move |variant| match draw.transition {
                            DrawTransition::Progress => usize::from(variant.next_discard.is_some()),
                            DrawTransition::SameShanten => variant
                                .downstream
                                .as_ref()
                                .map_or(0, |downstream| count(&downstream.draws)),
                        })
                })
                .sum()
        }

        candidate
            .draws_with(DrawTransition::SameShanten)
            .flat_map(|draw| draw.variants.iter())
            .map(|variant| {
                variant
                    .downstream
                    .as_ref()
                    .map_or(0, |downstream| count(&downstream.draws))
            })
            .sum()
    }

    #[test]
    fn the_two_shanten_speed_han_covers_the_whole_call_continuation() {
        // 3翻判定の対象は、Call 側 ExpectedSelfTsumoValue が評価した terminal 全体。直接到達
        // するテンパイがすべて要求翻数以上でも、production が既に評価している手変わり先の
        // terminal に届かないものがあれば高打点確定として扱わない。
        let ctx = toitoi_two_shanten_reaction_context(
            &LOW_VALUE_TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let case = post_call_iishanten_case(ctx, &action);
        let selection = case.selection(Some(CALL_TWO_SHANTEN_SPEED_MIN_HAN));

        let valuator = case.valuator();
        let lookahead = production_post_call_lookahead(
            &case.ctx,
            &case.tiles,
            &valuator,
            &selection.evaluation,
        );

        // 直接到達するテンパイはどれも対々和が付いて要求翻数以上。
        let direct = direct_progress_terminals(&case.tiles, &selection.evaluation, &lookahead);
        assert!(!direct.is_empty());
        for (concealed_tiles, discarded_tiles, acceptance) in &direct {
            let tenpai = ProspectiveTenpai {
                concealed_tiles,
                acceptance,
                discarded_tiles,
            };
            assert_eq!(
                valuator
                    .memoized_han_floor(&tenpai)
                    .is_at_least(CALL_TWO_SHANTEN_SPEED_MIN_HAN),
                Some(true)
            );
        }

        // 手変わり先まで含めると届かない terminal があるので、候補全体では確定しない。
        assert!(same_shanten_terminal_count(&lookahead) > 0);
        assert_eq!(
            selection.continuation_han,
            Some(ProspectiveHanVerdict::Below)
        );

        // したがって速度優先 policy も適用しない。
        let (_, candidate) = single_candidate(&case.ctx, &action, false);
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::Below));
        assert!(!speed.is_satisfied());
    }

    #[test]
    fn the_two_shanten_speed_han_accepts_a_high_value_continuation() {
        // 手変わりを経由した先の terminal まで含めてすべて要求翻数以上なら、これまでどおり
        // 速度優先 policy を適用できる。役牌2翻 + 赤5p が鳴き後のどの経路でも残る手。
        let ctx = low_value_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let case = post_call_iishanten_case(ctx, &action);
        let selection = case.selection(Some(CALL_TWO_SHANTEN_SPEED_MIN_HAN));

        let valuator = case.valuator();
        let lookahead = production_post_call_lookahead(
            &case.ctx,
            &case.tiles,
            &valuator,
            &selection.evaluation,
        );

        // 判定の対象に手変わり先の terminal が実際に含まれる。
        assert!(same_shanten_terminal_count(&lookahead) > 0);
        assert_eq!(
            selection.continuation_han,
            Some(ProspectiveHanVerdict::AtLeast)
        );

        let (_, candidate) = single_candidate(&case.ctx, &action, false);
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::AtLeast));
        assert!(speed.is_satisfied());
    }

    #[test]
    fn an_unscored_continuation_terminal_keeps_the_two_shanten_speed_han_unknown() {
        // 判定が読むのは探索が memo へ載せた下限だけ。手変わり先の terminal の翻数を確定でき
        // ない評価器では、その terminal のために点数計算をやり直さず Unknown のままにする。
        let ctx = low_value_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let case = post_call_iishanten_case(ctx, &action);
        let selection = case.selection(Some(CALL_TWO_SHANTEN_SPEED_MIN_HAN));
        // 探索が全 terminal を評価した場合はこの手で policy を適用できる。
        assert_eq!(
            selection.continuation_han,
            Some(ProspectiveHanVerdict::AtLeast)
        );

        let searched = case.valuator();
        let lookahead = production_post_call_lookahead(
            &case.ctx,
            &case.tiles,
            &searched,
            &selection.evaluation,
        );

        // 直接到達するテンパイの打点だけを持つ評価器を作る。
        let partial = case.valuator();
        for (concealed_tiles, discarded_tiles, acceptance) in
            direct_progress_terminals(&case.tiles, &selection.evaluation, &lookahead)
        {
            partial.tenpai_value(&ProspectiveTenpai {
                concealed_tiles: &concealed_tiles,
                acceptance,
                discarded_tiles: &discarded_tiles,
            });
        }

        let (verdict, folds) = han_floor_counter::count_during(|| {
            continuation_han_verdict(
                &partial,
                &case.tiles,
                &selection.evaluation,
                &lookahead,
                CALL_TWO_SHANTEN_SPEED_MIN_HAN,
            )
        });
        assert_eq!(verdict, ProspectiveHanVerdict::Unknown);
        // 判定のために点数計算をやり直さない。
        assert_eq!(folds, 0);
    }

    #[test]
    fn the_two_shanten_speed_han_verdict_adds_no_search_and_no_scoring() {
        // 判定は探索済みの枝と memo を読むだけ。探索 node も枝も terminal scoring も増やさない。
        let ctx = low_value_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let case = post_call_iishanten_case(ctx, &action);
        let selection = case.selection(Some(CALL_TWO_SHANTEN_SPEED_MIN_HAN));

        let valuator = case.valuator();
        let inputs = with_production_iishanten_continuation(lookahead_inputs(
            &case.ctx,
            &case.tiles,
            &valuator,
            LookaheadDiagnosticScope::None,
        ))
        .with_three_shanten_search_stats();
        let (_, lookahead) =
            forward_metrics_with_lookahead_for_candidate(&inputs, &selection.evaluation);
        let searched = inputs.three_shanten_search_stats();

        let (verdict, folds) = han_floor_counter::count_during(|| {
            continuation_han_verdict(
                &valuator,
                &case.tiles,
                &selection.evaluation,
                &lookahead,
                CALL_TWO_SHANTEN_SPEED_MIN_HAN,
            )
        });

        assert_eq!(verdict, ProspectiveHanVerdict::AtLeast);
        assert_eq!(inputs.three_shanten_search_stats(), searched);
        assert_eq!(folds, 0);
    }

    #[test]
    fn the_two_shanten_speed_verdict_does_not_change_the_post_call_selection() {
        // 翻数の判定は鳴き後1向聴の打牌選択が使った前方評価から回収するだけ。要求の有無で選ぶ
        // 打牌も Call 側 ExpectedSelfTsumoValue も変わらず、要求しない場合は確定打点の下限を
        // 畳む処理そのものを通らない。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (kind, called_tile, consumed) = normalize_call(&action).expect("Chi / Pon");
        let (meld, post_call_tiles) =
            call_meld_and_concealed_tiles(ctx.hand_tiles(), kind, called_tile, consumed)
                .expect("合法な鳴き");
        let forbidden = forbidden_discards_after_call(&meld);
        let post_call_fixed_meld_count =
            FixedMeldCount::new(ctx.own_fixed_meld_count().expect("副露数").get() + 1)
                .expect("上限内");
        let evaluations = post_call_discard_evaluations(
            &ctx,
            &post_call_tiles,
            post_call_fixed_meld_count,
            &forbidden,
        );
        let mut melds: Vec<Meld> = ctx.own_melds().unwrap_or_default().to_vec();
        melds.push(meld);

        let (without, without_folds) = han_floor_counter::count_during(|| {
            select_best_iishanten_post_call_discard(
                &ctx,
                &post_call_tiles,
                &melds,
                &evaluations,
                None,
            )
            .expect("鳴き後の打牌を選べる")
        });
        let (with, with_folds) = han_floor_counter::count_during(|| {
            select_best_iishanten_post_call_discard(
                &ctx,
                &post_call_tiles,
                &melds,
                &evaluations,
                Some(CALL_TWO_SHANTEN_SPEED_MIN_HAN),
            )
            .expect("鳴き後の打牌を選べる")
        });

        assert_eq!(with.evaluation, without.evaluation);
        assert_eq!(
            with.forward_metrics.expected_self_tsumo_value,
            without.forward_metrics.expected_self_tsumo_value
        );
        assert_eq!(without.continuation_han, None);
        assert_eq!(with.continuation_han, Some(ProspectiveHanVerdict::AtLeast));

        // 要求しない呼び出しは下限を1件も畳まない。要求した場合だけ、探索が評価した未来テンパイ
        // について畳む。
        assert_eq!(without_folds, 0);
        assert!(with_folds > 0);
    }

    #[test]
    fn the_two_shanten_speed_facts_come_from_the_existing_layers() {
        // 残り自摸機会は既存の own_future_draws、翻数は鳴き後の手牌 state を既存 prospective
        // scoring で評価した結果。どちらも診断専用の計算を持たない。
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);

        // 白 Pon + 發 Pon の後に 中 を Pon すると大三元。continuation が評価するどのテンパイでも
        // 3翻以上が確定する。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );
        let (_, candidate) = single_candidate(&ctx, &action, false);
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.own_future_draws, own_future_draws(&ctx));
        assert_eq!(
            speed.own_future_draws,
            Some(CALL_TWO_SHANTEN_SPEED_MIN_DRAWS)
        );
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::AtLeast));
        assert!(speed.is_satisfied());

        // 役牌2翻だけの手は要求翻数に届かない。赤5p を引き当てる枝だけは3翻になるが、直接
        // 到達する枝に確定翻数が足りないものがあれば候補全体が届かない扱いになる。
        let low_value = low_value_two_shanten_reaction_context(
            &LOW_VALUE_TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(40),
        );
        let (_, candidate) = single_candidate(&low_value, &action, false);
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::Below));
        assert!(!speed.is_satisfied());

        // 残り自摸9回以下の局面では高コストな将来打点を評価しない。
        let late = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(39),
        );
        let (_, candidate) = single_candidate(&late, &action, false);
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.own_future_draws, Some(9));
        assert_eq!(speed.han, None);
        assert!(!speed.is_satisfied());

        // 山の残枚数が unknown な局面も同じく評価しない。
        let unknown_wall = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            None,
        );
        let (_, candidate) = single_candidate(&unknown_wall, &action, false);
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.own_future_draws, None);
        assert_eq!(speed.han, None);
        assert!(!speed.is_satisfied());
    }

    #[test]
    fn the_han_floor_is_only_collected_when_the_speed_policy_needs_it() {
        // 判定を要求しない局面へ下限の集約コストを載せない。残り自摸9回以下・残り自摸数 unknown・
        // 鳴き後も2向聴のままの候補はどれも判定を要求しないので、鳴き判断1回で1件も畳まない。
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let policy_off = [Some(39), Some(0), None];
        for remaining in policy_off {
            let ctx = valued_two_shanten_reaction_context(
                &TWO_SHANTEN_CALL_PON_HAND,
                TWO_SHANTEN_CALL_PON_TARGET,
                Some(1),
                remaining,
            );
            let ((_, candidate), folds) =
                han_floor_counter::count_during(|| single_candidate(&ctx, &action, false));
            let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
            assert_eq!(speed.han, None, "{remaining:?}");
            assert_eq!(folds, 0, "{remaining:?}");
        }

        // 鳴いても2向聴のままの候補も判定を要求しない。
        let stays_two_shanten =
            valued_reaction_context(&RYANSHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 40);
        let stays_action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ((_, candidate), folds) = han_floor_counter::count_during(|| {
            single_candidate(&stays_two_shanten, &stays_action, false)
        });
        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotIishanten);
        assert_eq!(folds, 0);

        // 残り自摸10回以上で鳴き後1向聴になる候補だけ、探索が評価した未来テンパイについて畳む。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );
        let ((_, candidate), folds) =
            han_floor_counter::count_during(|| single_candidate(&ctx, &action, false));
        let speed = candidate.two_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::AtLeast));
        assert!(folds > 0);
    }

    #[test]
    fn the_two_shanten_speed_policy_does_not_override_an_opponent_reach() {
        // 他家リーチ時は速度優先 policy でも鳴かない。判定は従来どおり OpponentReached で、
        // 高打点・残り自摸の材料も評価しない。
        let ctx = reached_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, false);

        assert_eq!(candidate.reason, CallDecisionReason::OpponentReached);
        assert!(!candidate.eligible);
        assert_eq!(candidate.two_shanten_self_tsumo, None);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_two_shanten_speed_policy_is_not_applied_outside_two_shanten_to_iishanten() {
        // 鳴いても2向聴のままの候補と、1向聴のまま鳴く候補はどちらも対象外。速度優先 policy の
        // 材料を持つ診断そのものが付かない。
        let stays_two_shanten =
            valued_reaction_context(&RYANSHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 40);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&stays_two_shanten, &action, false);
        assert_eq!(candidate.current_shanten, Some(CALL_TWO_SHANTEN_SHANTEN));
        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotIishanten);
        assert_eq!(candidate.two_shanten_self_tsumo, None);
        assert_eq!(decision.selected, None);

        let stays_iishanten =
            valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 40);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (_, candidate) = single_candidate(&stays_iishanten, &action, false);
        assert_eq!(candidate.current_shanten, Some(CALL_CURRENT_SHANTEN));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));
        assert!(candidate.iishanten_self_tsumo.is_some());
        assert_eq!(candidate.two_shanten_self_tsumo, None);
    }

    // 既存2副露 + CC 5p E S W N の3向聴。C を Pon すると大三元の3副露になり、最良打牌で
    // 2向聴になる。
    const THREE_SHANTEN_CALL_PON_HAND: [u8; 7] = [132, 133, 53, 108, 112, 116, 120];
    const THREE_SHANTEN_CALL_PON_TARGET: u8 = 134;
    const THREE_SHANTEN_CALL_PON_CONSUMED: [u8; 2] = [132, 133];

    // 既存2副露 + 68m 5p E S W N の3向聴。6m8m で 7m を Chi した後、Pon と同じ post-call
    // concealed hand になり、共通の評価 path で2向聴になる。
    const THREE_SHANTEN_CALL_CHI_HAND: [u8; 7] = [20, 28, 53, 108, 112, 116, 120];
    const THREE_SHANTEN_CALL_CHI_TARGET: u8 = 24;
    const THREE_SHANTEN_CALL_CHI_CONSUMED: [u8; 2] = [20, 28];

    // 23m 67m 23p 67p 1s 5s 9s CC の3向聴。副露は無く、C を Pon しても雀頭が消えるだけで
    // 3向聴のまま。
    const THREE_SHANTEN_STAYS_PON_HAND: [u8; 13] =
        [4, 8, 20, 24, 40, 44, 56, 60, 72, 88, 104, 132, 133];

    // 速度優先 policy の残り自摸 12 回をちょうど満たす山の残枚数。
    const THREE_SHANTEN_SPEED_REMAINING: u32 = 48;

    // 白 Pon + 234m Chi の2副露。中 を Pon しても 白 と 中 の役牌2翻だけで、速度優先 policy の
    // 要求翻数に届かない。向聴数は副露済み面子数だけで決まるので、手牌側の評価は大三元の
    // fixture と同じ。
    fn low_value_three_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Chi, tiles(&[4, 8, 12]), Some(tile(4))),
        ];
        two_shanten_reaction_context_with_melds(hand, melds, target, Some(1), remaining_tiles)
    }

    // 同じ3向聴 reaction 局面で、reaction 元の他家だけがリーチしている場合。
    fn reached_three_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: Option<u32>,
    ) -> GameContext {
        reached_two_shanten_reaction_context(hand, target, remaining_tiles)
    }

    #[test]
    fn three_shanten_chi_and_pon_take_the_same_call_pass_value_path() {
        // 現在3向聴 → Chi / Pon → 打牌後2向聴 を、どちらの鳴き種別でも同じ Progress-only の
        // Call / Pass 比較で評価する。鳴き後は3副露になる。
        let cases = [
            (
                CallKind::Pon,
                &THREE_SHANTEN_CALL_PON_HAND[..],
                pon_action(
                    THREE_SHANTEN_CALL_PON_TARGET,
                    &THREE_SHANTEN_CALL_PON_CONSUMED,
                ),
                THREE_SHANTEN_CALL_PON_TARGET,
                1,
            ),
            (
                CallKind::Chi,
                &THREE_SHANTEN_CALL_CHI_HAND[..],
                chi_action(
                    THREE_SHANTEN_CALL_CHI_TARGET,
                    &THREE_SHANTEN_CALL_CHI_CONSUMED,
                ),
                THREE_SHANTEN_CALL_CHI_TARGET,
                3,
            ),
        ];

        for (kind, hand, action, target, source) in cases {
            let ctx = valued_two_shanten_reaction_context(
                hand,
                target,
                Some(source),
                Some(THREE_SHANTEN_SPEED_REMAINING),
            );
            let (decision, candidate) = single_candidate(&ctx, &action, true);
            let comparison = candidate
                .three_shanten_self_tsumo
                .expect("3向聴 Call / Pass 比較対象");

            assert_eq!(candidate.action, action);
            assert_eq!(candidate.kind, kind);
            assert_eq!(candidate.current_shanten, Some(CALL_THREE_SHANTEN_SHANTEN));
            assert_eq!(
                candidate.post_call_shanten(),
                Some(CALL_TWO_SHANTEN_SHANTEN)
            );
            // 現在3向聴は副露2件までしか取り得ないので、鳴き後は最大3副露になる。
            assert_eq!(candidate.post_call_fixed_meld_count, FixedMeldCount::new(3));
            assert_eq!(comparison.reaction_source_player, Some(source));
            assert_eq!(
                comparison.pass_evaluation,
                CallThreeShantenPassEvaluation::ProgressOnly
            );
            assert!(comparison.pass_expected_self_tsumo_value.is_some());
            assert!(comparison.call_expected_self_tsumo_value.is_some());
            assert!(
                comparison.call_expected_self_tsumo_value
                    > comparison.pass_expected_self_tsumo_value
            );
            assert_eq!(comparison.comparison, CallIishantenComparison::CallHigher);
            assert_eq!(
                candidate.reason,
                CallDecisionReason::EligibleThreeShantenSelfTsumo
            );
            assert!(candidate.eligible);
            assert_eq!(decision.selected.as_ref(), Some(&action));

            // 喰い替え禁止牌を除いた既存候補だけが comparator に渡される。
            let forbidden = candidate
                .post_call_forbidden_discards
                .as_ref()
                .expect("喰い替え制約を評価済み");
            assert!(!forbidden.is_empty());
            assert!(!forbidden.contains(&candidate.post_call_discard.as_ref().unwrap().discard));
        }
    }

    #[test]
    fn the_three_shanten_call_and_pass_share_the_progress_only_primitives() {
        // Call 側は鳴き後2向聴の既存 Progress 評価と既存 comparator、Pass 側は次の自摸を待つ
        // 3向聴 state の Progress-only 入口。どちらも production が実際に使った値そのもので、
        // 鳴き判断側に別の探索を持たない。
        let ctx = valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(THREE_SHANTEN_SPEED_REMAINING),
        );
        let action = pon_action(
            THREE_SHANTEN_CALL_PON_TARGET,
            &THREE_SHANTEN_CALL_PON_CONSUMED,
        );
        let (_, candidate) = single_candidate(&ctx, &action, false);
        let comparison = candidate.three_shanten_self_tsumo.expect("比較対象");

        let case = post_call_two_shanten_case(ctx, &action);
        let selection = case.selection(None);
        assert_eq!(
            comparison.call_expected_self_tsumo_value,
            selection.expected_self_tsumo_value
        );
        assert_eq!(
            candidate.post_call_discard.as_ref().map(|e| e.discard),
            Some(selection.evaluation.discard)
        );

        // Pass 側は架空の現在打牌を作らず、現在の13枚を次の自摸を待つ state として既存入口へ
        // 渡す。horizon も既存の Pass 用の残り自摸機会そのもの。
        let pass = pass_expected_self_tsumo_value(
            &case.ctx,
            CALL_THREE_SHANTEN_SHANTEN,
            awaiting_draw_three_shanten_progress_only_self_tsumo_value,
        );
        assert_eq!(comparison.pass_expected_self_tsumo_value, pass);
        assert!(pass.is_some());
        assert_eq!(pass_own_future_draws(&case.ctx), Some(12));
        assert_eq!(own_future_draws(&case.ctx), Some(12));
    }

    #[test]
    fn a_three_shanten_call_that_stays_three_shanten_is_not_compared() {
        // 鳴いても3向聴のままの候補は対象外。Call / Pass 比較の診断そのものが付かない。
        let ctx = valued_reaction_context(
            &THREE_SHANTEN_STAYS_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            1,
            THREE_SHANTEN_SPEED_REMAINING,
        );
        let action = pon_action(
            THREE_SHANTEN_CALL_PON_TARGET,
            &THREE_SHANTEN_CALL_PON_CONSUMED,
        );
        let ((decision, candidate), folds) =
            han_floor_counter::count_during(|| single_candidate(&ctx, &action, false));

        assert_eq!(candidate.current_shanten, Some(CALL_THREE_SHANTEN_SHANTEN));
        assert_eq!(
            candidate.post_call_shanten(),
            Some(CALL_THREE_SHANTEN_SHANTEN)
        );
        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotTwoShanten);
        assert!(!candidate.eligible);
        assert_eq!(candidate.three_shanten_self_tsumo, None);
        assert_eq!(decision.selected, None);
        // 判定を要求しない候補なので、確定打点の下限も畳まない。
        assert_eq!(folds, 0);
    }

    // 鳴き後2向聴の打牌選択を production と同じ入力で組み立て直すための材料。
    struct PostCallTwoShantenCase {
        ctx: GameContext,
        melds: Vec<Meld>,
        tiles: Vec<TileId>,
        evaluations: Vec<DiscardEvaluation>,
    }

    fn post_call_two_shanten_case(
        ctx: GameContext,
        action: &LegalAction,
    ) -> PostCallTwoShantenCase {
        let case = post_call_iishanten_case(ctx, action);
        PostCallTwoShantenCase {
            ctx: case.ctx,
            melds: case.melds,
            tiles: case.tiles,
            evaluations: case.evaluations,
        }
    }

    impl PostCallTwoShantenCase {
        fn selection(&self, required_han: Option<u8>) -> PostCallTwoShantenSelection {
            select_best_two_shanten_post_call_discard(
                &self.ctx,
                &self.tiles,
                &self.melds,
                &self.evaluations,
                required_han,
            )
            .expect("鳴き後の打牌を選べる")
        }

        // production の鳴き後打牌選択と同じ入力を、探索規模の計上付きで組み立てる。
        fn measured_selection(
            &self,
            required_han: Option<u8>,
        ) -> (
            Option<u64>,
            Option<ProspectiveHanVerdict>,
            ThreeShantenSearchStats,
            Vec<Option<ProspectiveHanVerdict>>,
        ) {
            let valuator =
                ProductionProspectiveValuator::new_with_hand_state(&self.ctx, Some(&self.melds))
                    .collecting_han_floor(required_han.is_some());
            let inputs = with_production_three_shanten_continuation(lookahead_inputs(
                &self.ctx,
                &self.tiles,
                &valuator,
                LookaheadDiagnosticScope::None,
            ))
            .with_three_shanten_search_stats();
            let mut candidate_verdicts = Vec::new();
            let (_, value, verdict) = bot_logic::best_two_shanten_progress_discard_among_observed(
                &inputs,
                &self.evaluations,
                || valuator.reset_scored_han_floor(),
                || {
                    let verdict = required_han.map(|han| scored_han_verdict(&valuator, han));
                    candidate_verdicts.push(verdict);
                    verdict
                },
            )
            .expect("鳴き後の打牌を選べる");
            (
                value,
                verdict,
                inputs.three_shanten_search_stats(),
                candidate_verdicts,
            )
        }
    }

    fn three_shanten_han_regression_case(all_man_seen: bool) -> PostCallTwoShantenCase {
        let mut hand = THREE_SHANTEN_CALL_CHI_HAND;
        hand[2] = 36; // 1p
        hand[3] = 0; // 1m
        let ctx = valued_two_shanten_reaction_context(
            &hand,
            THREE_SHANTEN_CALL_CHI_TARGET,
            Some(1),
            Some(48),
        );
        let ctx = if all_man_seen {
            let mut visible = ctx.visible_tiles().to_vec();
            // 萬子をすべて見え牌にし、1mを残した continuation の受け入れを減らす。
            for value in 0..36 {
                let tile = tile(value);
                if !visible.contains(&tile) {
                    visible.push(tile);
                }
            }
            GameContext::from_parts_with_melds(
                None,
                ctx.hand_tiles().to_vec(),
                vec![],
                TileType::new(EAST),
                TileType::new(EAST),
                visible,
                Some(0),
                Some(0),
                [
                    vec![],
                    vec![tile(THREE_SHANTEN_CALL_CHI_TARGET)],
                    vec![],
                    vec![],
                ],
                [false; 4],
                ctx.melds().clone(),
            )
            .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
                same_turn: Some(false),
                riichi_missed_win: Some(false),
            })
            .with_reaction_source_player(Some(1))
            .with_table_state_facts(crate::context::TableStateFacts {
                remaining_tiles: Some(48),
                ..Default::default()
            })
        } else {
            ctx
        };
        let action = chi_action(
            THREE_SHANTEN_CALL_CHI_TARGET,
            &THREE_SHANTEN_CALL_CHI_CONSUMED,
        );
        let mut case = post_call_two_shanten_case(ctx, &action);
        case.evaluations
            .retain(|e| e.discard == tile(36).tile_type() || e.discard == tile(0).tile_type());
        case
    }

    // 白・發 Pon + 678m Chi の鳴き後に 1p 1m S W N が残る。
    // 1p 切りの continuation は混一色等で全 terminal 4翻以上。
    // 1m 切りの continuation は筒子の完成形に3翻以下を含む。
    fn assert_selected_three_shanten_han_verdict(
        all_man_seen: bool,
        selected_discard: u8,
        expected: ProspectiveHanVerdict,
    ) {
        let mut case = three_shanten_han_regression_case(all_man_seen);
        assert_eq!(case.evaluations.len(), 2);
        for evaluation in &case.evaluations {
            let single = select_best_two_shanten_post_call_discard(
                &case.ctx,
                &case.tiles,
                &case.melds,
                std::slice::from_ref(evaluation),
                Some(4),
            )
            .unwrap();
            assert_eq!(
                single.scored_han,
                Some(if evaluation.discard == tile(36).tile_type() {
                    ProspectiveHanVerdict::AtLeast
                } else {
                    ProspectiveHanVerdict::Below
                })
            );
        }
        // 入力順を反転しても、最後の候補ではなく選択候補の verdict が返る。
        for _ in 0..2 {
            let plain = case.selection(None);
            let observed = case.selection(Some(4));
            assert_eq!(
                observed.evaluation.discard,
                tile(selected_discard).tile_type()
            );
            assert_eq!(observed.evaluation, plain.evaluation);
            assert_eq!(
                observed.expected_self_tsumo_value,
                plain.expected_self_tsumo_value
            );
            assert_eq!(observed.scored_han, Some(expected));
            let (plain_value, plain_verdict, plain_stats, _) = case.measured_selection(None);
            let (value, verdict, stats, candidate_verdicts) = case.measured_selection(Some(4));
            // 未選択候補も Progress 評価され、異なる判定が実際に得られている。
            assert_eq!(candidate_verdicts.len(), 2);
            assert!(candidate_verdicts.contains(&Some(ProspectiveHanVerdict::AtLeast)));
            assert!(candidate_verdicts.contains(&Some(ProspectiveHanVerdict::Below)));
            assert_eq!(plain_verdict, None);
            assert_eq!(verdict, Some(expected));
            assert_eq!(value, plain_value);
            assert_eq!(value, observed.expected_self_tsumo_value);
            assert!(stats.terminal_scorings > 0);
            assert_eq!(stats, plain_stats);
            assert_eq!(stats.iishanten_same_shanten_variants, 0);
            assert_eq!(stats.iishanten_downstream_variants, 0);
            case.evaluations.reverse();
        }
    }

    #[test]
    fn selected_four_han_continuation_excludes_unselected_below_four_han_terminals() {
        // 1m が3枚残る局面では、1p切りが選ばれる。
        assert_selected_three_shanten_han_verdict(false, 36, ProspectiveHanVerdict::AtLeast);
    }

    #[test]
    fn selected_below_four_han_continuation_excludes_unselected_four_han_terminals() {
        // 萬子がすべて見えている局面では、1m切りが選ばれる。
        assert_selected_three_shanten_han_verdict(true, 0, ProspectiveHanVerdict::Below);
    }

    #[test]
    fn the_three_shanten_speed_verdict_adds_no_search_and_no_scoring() {
        // 4翻判定は鳴き後2向聴の Progress-only 評価が行った terminal scoring から回収するだけ。
        // 要求の有無で選ぶ打牌も Call 側の値も変わらず、探索 node も terminal scoring も
        // 増やさない。判定のために SameShanten を追加探索することもない。
        let ctx = valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(THREE_SHANTEN_SPEED_REMAINING),
        );
        let action = pon_action(
            THREE_SHANTEN_CALL_PON_TARGET,
            &THREE_SHANTEN_CALL_PON_CONSUMED,
        );
        let case = post_call_two_shanten_case(ctx, &action);

        let (without, without_folds) = han_floor_counter::count_during(|| case.selection(None));
        let (with, with_folds) = han_floor_counter::count_during(|| {
            case.selection(Some(CALL_THREE_SHANTEN_SPEED_MIN_HAN))
        });

        assert_eq!(with.evaluation, without.evaluation);
        assert_eq!(
            with.expected_self_tsumo_value,
            without.expected_self_tsumo_value
        );
        assert_eq!(without.scored_han, None);
        assert_eq!(with.scored_han, Some(ProspectiveHanVerdict::AtLeast));
        // 要求しない呼び出しは下限を1件も畳まない。
        assert_eq!(without_folds, 0);
        assert!(with_folds > 0);

        let (plain_value, plain_verdict, plain_stats, _) = case.measured_selection(None);
        let (verdict_value, verdict, verdict_stats, _) =
            case.measured_selection(Some(CALL_THREE_SHANTEN_SPEED_MIN_HAN));
        assert_eq!(plain_verdict, None);
        assert_eq!(verdict, Some(ProspectiveHanVerdict::AtLeast));
        assert_eq!(verdict_value, plain_value);
        // terminal scoring の回数も探索規模も判定の有無で同じ。
        assert!(plain_stats.terminal_scorings > 0);
        assert_eq!(verdict_stats, plain_stats);
        // Progress-only なので1向聴到達後に SameShanten の枝を列挙しない。
        assert_eq!(verdict_stats.iishanten_same_shanten_variants, 0);
        assert_eq!(verdict_stats.iishanten_downstream_variants, 0);
        assert!(verdict_stats.iishanten_progress_variants > 0);
    }

    #[test]
    fn the_three_shanten_speed_facts_come_from_the_existing_layers() {
        // 残り自摸機会は既存の own_future_draws、翻数は鳴き後の手牌 state を既存 prospective
        // scoring で評価した結果。どちらも診断専用の計算を持たない。
        let action = pon_action(
            THREE_SHANTEN_CALL_PON_TARGET,
            &THREE_SHANTEN_CALL_PON_CONSUMED,
        );

        // 白 Pon + 發 Pon の後に 中 を Pon すると大三元。評価したどのテンパイでも4翻以上が
        // 確定する。
        let ctx = valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(THREE_SHANTEN_SPEED_REMAINING),
        );
        let (_, candidate) = single_candidate(&ctx, &action, false);
        let speed = candidate.three_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.own_future_draws, own_future_draws(&ctx));
        assert_eq!(
            speed.own_future_draws,
            Some(CALL_THREE_SHANTEN_SPEED_MIN_DRAWS)
        );
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::AtLeast));
        assert!(speed.is_satisfied());

        // 役牌2翻だけの手は要求翻数に届かない。1件でも足りない terminal があれば候補全体が
        // 届かない扱いになる。
        let low_value = low_value_three_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(THREE_SHANTEN_SPEED_REMAINING),
        );
        let (_, candidate) = single_candidate(&low_value, &action, false);
        let speed = candidate.three_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.han, Some(ProspectiveHanVerdict::Below));
        assert!(!speed.is_satisfied());

        // 残り自摸11回以下の局面では高コストな将来打点を評価しない。
        let late = valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(THREE_SHANTEN_SPEED_REMAINING - 4),
        );
        let ((_, candidate), folds) =
            han_floor_counter::count_during(|| single_candidate(&late, &action, false));
        let speed = candidate.three_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(
            speed.own_future_draws,
            Some(CALL_THREE_SHANTEN_SPEED_MIN_DRAWS - 1)
        );
        assert_eq!(speed.han, None);
        assert!(!speed.is_satisfied());
        assert_eq!(folds, 0);

        // 山の残枚数が unknown な局面も同じく評価しない。
        let unknown_wall = valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(1),
            None,
        );
        let ((_, candidate), folds) =
            han_floor_counter::count_during(|| single_candidate(&unknown_wall, &action, false));
        let speed = candidate.three_shanten_self_tsumo.expect("比較対象").speed;
        assert_eq!(speed.own_future_draws, None);
        assert_eq!(speed.han, None);
        assert!(!speed.is_satisfied());
        assert_eq!(folds, 0);
    }

    #[test]
    fn the_three_shanten_speed_policy_does_not_override_an_opponent_reach() {
        // 他家リーチ時は速度優先 policy でも鳴かない。判定は従来どおり OpponentReached で、
        // 高打点・残り自摸の材料も評価しない。
        let ctx = reached_three_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(THREE_SHANTEN_SPEED_REMAINING),
        );
        let action = pon_action(
            THREE_SHANTEN_CALL_PON_TARGET,
            &THREE_SHANTEN_CALL_PON_CONSUMED,
        );
        let (decision, candidate) = single_candidate(&ctx, &action, false);

        assert_eq!(candidate.reason, CallDecisionReason::OpponentReached);
        assert!(!candidate.eligible);
        assert_eq!(candidate.three_shanten_self_tsumo, None);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_three_shanten_policy_is_not_applied_outside_three_shanten_to_two_shanten() {
        // 1向聴・2向聴からの鳴きは従来どおり。3向聴 policy の診断そのものが付かない。
        let two_shanten = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(40),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (_, candidate) = single_candidate(&two_shanten, &action, false);
        assert_eq!(candidate.current_shanten, Some(CALL_TWO_SHANTEN_SHANTEN));
        assert_eq!(
            candidate.reason,
            CallDecisionReason::EligibleTwoShantenSelfTsumo
        );
        assert!(candidate.two_shanten_self_tsumo.is_some());
        assert_eq!(candidate.three_shanten_self_tsumo, None);

        let stays_iishanten =
            valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 40);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (_, candidate) = single_candidate(&stays_iishanten, &action, false);
        assert_eq!(candidate.current_shanten, Some(CALL_CURRENT_SHANTEN));
        assert!(candidate.iishanten_self_tsumo.is_some());
        assert_eq!(candidate.three_shanten_self_tsumo, None);
    }

    fn three_shanten_speed_facts(
        own_future_draws: Option<u32>,
        han: Option<ProspectiveHanVerdict>,
    ) -> CallThreeShantenSpeedDiagnostic {
        CallThreeShantenSpeedDiagnostic {
            own_future_draws,
            han,
            overrides_pass: false,
        }
    }

    // 速度優先 policy の条件をすべて満たす判断材料。
    fn satisfied_three_shanten_speed_facts() -> CallThreeShantenSpeedDiagnostic {
        three_shanten_speed_facts(
            Some(CALL_THREE_SHANTEN_SPEED_MIN_DRAWS),
            Some(ProspectiveHanVerdict::AtLeast),
        )
    }

    // 鳴き後2向聴まで評価が進んだ3向聴候補。Call / Pass の値と速度優先 policy の材料だけを
    // 差し替えて、比較と上書きの semantics だけを見る。
    fn three_shanten_speed_candidate(
        call_value: Option<u64>,
        speed: CallThreeShantenSpeedDiagnostic,
    ) -> CallCandidateDiagnostic {
        CallCandidateDiagnostic {
            current_shanten: Some(CALL_THREE_SHANTEN_SHANTEN),
            three_shanten_self_tsumo: Some(CallThreeShantenSelfTsumoDiagnostic {
                reaction_source_player: Some(1),
                pass_evaluation: CallThreeShantenPassEvaluation::ProgressOnly,
                pass_expected_self_tsumo_value: None,
                call_expected_self_tsumo_value: call_value,
                comparison: CallIishantenComparison::Unknown,
                speed,
            }),
            reason: CallDecisionReason::IishantenSelfTsumoUnknown,
            ..new_call_candidate(
                &pon_action(
                    THREE_SHANTEN_CALL_PON_TARGET,
                    &THREE_SHANTEN_CALL_PON_CONSUMED,
                ),
                CallKind::Pon,
            )
        }
    }

    // 重ねて評価済みの Pass 値を渡して policy を1回適用する。Pass の再評価は行わない。
    fn applied_three_shanten_policy(
        ctx: &GameContext,
        call_value: Option<u64>,
        pass_value: Option<u64>,
        speed: CallThreeShantenSpeedDiagnostic,
    ) -> CallCandidateDiagnostic {
        let mut candidates = vec![three_shanten_speed_candidate(call_value, speed)];
        apply_three_shanten_self_tsumo_policy(
            ctx,
            &mut candidates,
            Some(PassSelfTsumoContinuation {
                kind: PassSelfTsumoContinuationKind::ThreeShanten,
                value: pass_value,
                elapsed: Duration::ZERO,
            }),
            &mut CallDecisionTimer::disabled(),
        );
        candidates.remove(0)
    }

    // 3向聴 policy の値比較だけを見る局面。値は呼び出し側が差し替える。
    fn three_shanten_policy_context() -> GameContext {
        valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(THREE_SHANTEN_SPEED_REMAINING),
        )
    }

    #[test]
    fn a_three_shanten_call_with_a_higher_progress_value_is_selected() {
        // 通常判定は Call Progress value > Pass Progress value のときだけ Call。同値・Call が
        // 低い・unknown はすべて Pass になる。
        let ctx = three_shanten_policy_context();
        let not_applied = three_shanten_speed_facts(None, None);

        let call_higher = applied_three_shanten_policy(&ctx, Some(101), Some(100), not_applied);
        assert_eq!(
            call_higher.reason,
            CallDecisionReason::EligibleThreeShantenSelfTsumo
        );
        assert!(call_higher.eligible);
        assert_eq!(
            call_higher
                .three_shanten_self_tsumo
                .expect("比較対象")
                .comparison,
            CallIishantenComparison::CallHigher
        );
        assert_eq!(
            select_eligible_candidate(std::slice::from_ref(&call_higher)),
            Some(0)
        );

        for call in [Some(100), Some(99), None] {
            let candidate = applied_three_shanten_policy(&ctx, call, Some(100), not_applied);
            assert!(!candidate.eligible, "{call:?}");
            assert_ne!(
                candidate.reason,
                CallDecisionReason::EligibleThreeShantenSelfTsumo,
                "{call:?}"
            );
            assert_eq!(
                select_eligible_candidate(std::slice::from_ref(&candidate)),
                None,
                "{call:?}"
            );
        }
    }

    #[test]
    fn a_high_value_three_shanten_call_is_taken_even_when_the_pass_value_is_not_lower() {
        // 評価した全テンパイが4翻以上確定 + 残り自摸12回以上の 3向聴 → 2向聴 は、Call の
        // Progress value が Pass 以下でも速度優先で鳴く。同値と Call 側が低い場合の両方を含む。
        let ctx = three_shanten_policy_context();

        for call in [Some(100), Some(99)] {
            let candidate = applied_three_shanten_policy(
                &ctx,
                call,
                Some(100),
                satisfied_three_shanten_speed_facts(),
            );
            let comparison = candidate.three_shanten_self_tsumo.expect("比較対象");

            assert_eq!(
                candidate.reason,
                CallDecisionReason::EligibleThreeShantenSpeed
            );
            assert!(candidate.eligible);
            // 上書きするのは理由だけで、値比較そのものは Pass のまま診断へ残す。
            assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
            assert!(comparison.speed.overrides_pass);
            assert!(comparison.speed.is_satisfied());
            assert_eq!(
                select_eligible_candidate(std::slice::from_ref(&candidate)),
                Some(0),
                "{candidate:?}"
            );
        }
    }

    #[test]
    fn a_three_shanten_call_below_the_required_han_keeps_the_value_comparison() {
        // 3翻以下・翻数 unknown・翻数を評価していない候補はどれも従来どおり Pass。
        let ctx = three_shanten_policy_context();

        for han in [
            Some(ProspectiveHanVerdict::Below),
            Some(ProspectiveHanVerdict::Unknown),
            None,
        ] {
            let speed = three_shanten_speed_facts(Some(CALL_THREE_SHANTEN_SPEED_MIN_DRAWS), han);
            let candidate = applied_three_shanten_policy(&ctx, Some(100), Some(100), speed);
            let comparison = candidate.three_shanten_self_tsumo.expect("比較対象");

            assert_eq!(
                candidate.reason,
                CallDecisionReason::PassSelfTsumoNotLower,
                "{han:?}"
            );
            assert!(!candidate.eligible, "{han:?}");
            assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
            assert!(!comparison.speed.overrides_pass, "{han:?}");
            assert!(!comparison.speed.is_satisfied(), "{han:?}");
            assert_eq!(
                select_eligible_candidate(std::slice::from_ref(&candidate)),
                None,
                "{han:?}"
            );
        }
    }

    #[test]
    fn a_three_shanten_call_with_too_few_remaining_draws_keeps_the_value_comparison() {
        // 残り自摸11回以下と、残り自摸数を確定できない局面はどちらも速度優先 policy を適用せず
        // 従来の比較へ戻す。
        let ctx = three_shanten_policy_context();

        for draws in [Some(CALL_THREE_SHANTEN_SPEED_MIN_DRAWS - 1), Some(0), None] {
            let speed = three_shanten_speed_facts(draws, Some(ProspectiveHanVerdict::AtLeast));
            let candidate = applied_three_shanten_policy(&ctx, Some(100), Some(100), speed);
            let comparison = candidate.three_shanten_self_tsumo.expect("比較対象");

            assert_eq!(
                candidate.reason,
                CallDecisionReason::PassSelfTsumoNotLower,
                "{draws:?}"
            );
            assert!(!candidate.eligible, "{draws:?}");
            assert!(!comparison.speed.overrides_pass, "{draws:?}");
            assert!(!comparison.speed.is_satisfied(), "{draws:?}");
        }
    }

    #[test]
    fn the_three_shanten_speed_policy_overrides_only_the_value_comparison() {
        // 上書きするのは値比較が Pass と結論した場合だけ。Call が既に高い場合は従来の理由の
        // まま、値 unknown と反応元不明は速度優先 policy でも鳴かない。
        let ctx = three_shanten_policy_context();

        let call_higher = applied_three_shanten_policy(
            &ctx,
            Some(101),
            Some(100),
            satisfied_three_shanten_speed_facts(),
        );
        assert_eq!(
            call_higher.reason,
            CallDecisionReason::EligibleThreeShantenSelfTsumo
        );
        assert!(call_higher.eligible);
        assert!(
            !call_higher
                .three_shanten_self_tsumo
                .expect("比較対象")
                .speed
                .overrides_pass
        );

        let unknown_value = applied_three_shanten_policy(
            &ctx,
            None,
            Some(100),
            satisfied_three_shanten_speed_facts(),
        );
        assert_eq!(
            unknown_value.reason,
            CallDecisionReason::IishantenSelfTsumoUnknown
        );
        assert!(!unknown_value.eligible);
        assert!(
            !unknown_value
                .three_shanten_self_tsumo
                .expect("比較対象")
                .speed
                .overrides_pass
        );

        let unknown_source = valued_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            None,
            Some(THREE_SHANTEN_SPEED_REMAINING),
        );
        let candidate = applied_three_shanten_policy(
            &unknown_source,
            Some(100),
            Some(100),
            satisfied_three_shanten_speed_facts(),
        );
        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
        assert!(!candidate.eligible);
        assert!(
            !candidate
                .three_shanten_self_tsumo
                .expect("比較対象")
                .speed
                .overrides_pass
        );
    }

    #[test]
    fn two_shanten_call_pass_comparison_keeps_all_three_outcomes() {
        // 2向聴からの鳴きも1向聴からの鳴きと同じ比較 semantics を使う。厳密に高い場合だけ
        // 鳴き、同値と unknown は Pass の理由になる。
        for (call, pass, expected) in [
            (
                Some(101),
                Some(100),
                (
                    CallIishantenComparison::CallHigher,
                    CallDecisionReason::EligibleTwoShantenSelfTsumo,
                ),
            ),
            (
                Some(100),
                Some(100),
                (
                    CallIishantenComparison::PassNotLower,
                    CallDecisionReason::PassSelfTsumoNotLower,
                ),
            ),
            (
                Some(99),
                Some(100),
                (
                    CallIishantenComparison::PassNotLower,
                    CallDecisionReason::PassSelfTsumoNotLower,
                ),
            ),
            (
                None,
                Some(100),
                (
                    CallIishantenComparison::Unknown,
                    CallDecisionReason::IishantenSelfTsumoUnknown,
                ),
            ),
        ] {
            assert_eq!(
                compare_call_pass_self_tsumo_values(
                    true,
                    call,
                    pass,
                    CallDecisionReason::EligibleTwoShantenSelfTsumo,
                ),
                expected
            );
        }

        // 反応元の席が分からない局面では Pass の horizon を作れないので鳴かない。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            None,
            Some(32),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");
        assert!(comparison.call_expected_self_tsumo_value.is_some());
        assert_eq!(comparison.pass_expected_self_tsumo_value, None);
        assert_eq!(comparison.comparison, CallIishantenComparison::Unknown);
        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);

        // 自摸機会が残っていない局面は両側とも 0 で同値になる。同値は鳴かない。
        let zero_draws = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(0),
        );
        let (decision, candidate) = single_candidate(&zero_draws, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");
        assert_eq!(comparison.pass_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.call_expected_self_tsumo_value, Some(0));
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_two_shanten_post_call_iishanten_value_uses_the_production_continuation_depth() {
        // 2向聴からの鳴きを観測する経路も、鳴いた後の1向聴候補比較は production の手変わり深度を
        // 通る。手変わり1回までの旧設定では call 2239.229406 になる局面を使う。Pass 側は次の
        // 自摸を待つ2向聴 state の既存 Full evaluation そのままで、深度には依らない。
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(56),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("観測対象");

        assert_eq!(
            comparison.call_expected_self_tsumo_value,
            Some(4_103_395_595),
            "call",
        );
        assert_eq!(
            comparison.pass_expected_self_tsumo_value,
            Some(189_935_840),
            "pass",
        );
    }

    // `現在2向聴 → Call → 2向聴のまま` の observation が使う Pass Progress 値は、Pass Full と同じ入力を
    // 既存の awaiting-draw Progress helper へ渡した値そのもの。
    #[test]
    fn the_two_shanten_pass_progress_is_the_existing_awaiting_draw_progress_helper() {
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(56),
        );
        let counts = TileCounts::from_tiles(ctx.hand_tiles().iter().copied());
        let acceptance = calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &counts,
            ctx.own_fixed_meld_count().unwrap(),
            ctx.visible_tiles(),
        );
        let valuator = ProductionProspectiveValuator::new_with_hand_state(&ctx, ctx.own_melds());
        let inputs =
            with_production_iishanten_continuation(lookahead_inputs_with_own_future_draws(
                &ctx,
                ctx.hand_tiles(),
                &valuator,
                LookaheadDiagnosticScope::None,
                pass_own_future_draws(&ctx),
            ));
        let progress = awaiting_draw_two_shanten_progress_self_tsumo_value(&inputs, &acceptance);

        assert!(progress.is_some());
        assert_eq!(pass_two_shanten_progress_self_tsumo_value(&ctx), progress);
        assert!(pass_two_shanten_expected_self_tsumo_value(&ctx) > progress);
    }

    #[test]
    fn the_two_shanten_call_uses_the_same_values_with_and_without_diagnostics() {
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(32),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);

        let (production, production_candidate) = single_candidate(&ctx, &action, false);
        let (diagnosed, diagnosed_candidate) = single_candidate(&ctx, &action, true);

        // diagnostics は判断に使わない観測値を足すだけで、Call / Pass の値も比較も action も
        // 同じ経路の結果そのもの。
        assert_eq!(production_candidate, diagnosed_candidate);
        assert_eq!(production.selected, diagnosed.selected);
        assert_eq!(production.selected.as_ref(), Some(&action));
        assert_eq!(
            production_candidate.two_shanten_self_tsumo,
            diagnosed_candidate.two_shanten_self_tsumo
        );
        assert_eq!(
            production_candidate
                .two_shanten_self_tsumo
                .expect("比較対象")
                .comparison,
            CallIishantenComparison::CallHigher
        );
    }

    #[test]
    fn only_the_highest_two_shanten_call_value_is_selected() {
        // 複数の成立候補があっても、選ぶのは比較に使った Call 側 ExpectedSelfTsumoValue が
        // 最大の候補。同値では合法 action の先頭を維持する。
        let two_shanten = |call_value: u64, reason| CallCandidateDiagnostic {
            two_shanten_self_tsumo: Some(CallTwoShantenSelfTsumoDiagnostic {
                reaction_source_player: Some(1),
                pass_evaluation: CallTwoShantenPassEvaluation::Full,
                pass_expected_self_tsumo_value: Some(100),
                call_expected_self_tsumo_value: Some(call_value),
                comparison: if reason == CallDecisionReason::EligibleTwoShantenSelfTsumo {
                    CallIishantenComparison::CallHigher
                } else {
                    CallIishantenComparison::PassNotLower
                },
                speed: not_applied_speed(),
            }),
            eligible: reason == CallDecisionReason::EligibleTwoShantenSelfTsumo,
            reason,
            ..new_call_candidate(
                &pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                CallKind::Pon,
            )
        };

        let candidates = vec![
            two_shanten(101, CallDecisionReason::EligibleTwoShantenSelfTsumo),
            two_shanten(90, CallDecisionReason::PassSelfTsumoNotLower),
            two_shanten(300, CallDecisionReason::EligibleTwoShantenSelfTsumo),
        ];
        assert_eq!(select_eligible_candidate(&candidates), Some(2));

        let declined = vec![
            two_shanten(90, CallDecisionReason::PassSelfTsumoNotLower),
            two_shanten(100, CallDecisionReason::PassSelfTsumoNotLower),
        ];
        assert_eq!(select_eligible_candidate(&declined), None);
    }

    #[test]
    fn an_iishanten_call_that_stays_iishanten_compares_the_pass_and_post_call_acceptance() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
        assert_eq!(candidate.current_shanten, Some(CALL_CURRENT_SHANTEN));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));

        let acceptance = candidate
            .iishanten_acceptance
            .expect("1向聴 → 1向聴 が対象");

        // 鳴かなかった場合の受け入れは、現在の副露済み面子数と見え牌を反映した既存計算そのもの。
        let pass = calculate_acceptance_with_fixed_melds_and_visible_tiles(
            &TileCounts::from_tiles(ctx.hand_tiles().iter().copied()),
            ctx.own_fixed_meld_count().unwrap(),
            ctx.visible_tiles(),
        );
        assert_eq!(acceptance.pass_acceptance_remaining, pass.total_remaining());
        assert_eq!(acceptance.pass_acceptance_type_count, pass.tiles.len());

        // 鳴いた後の向聴と受け入れは、本番の鳴き後打牌評価が持つ値そのもの。
        let evaluation = candidate.post_call_discard.as_ref().unwrap();
        assert_eq!(
            acceptance.post_call_shanten,
            evaluation.min_shanten_after_discard()
        );
        assert_eq!(
            acceptance.post_call_acceptance_remaining,
            evaluation.acceptance_total_remaining()
        );
        assert_eq!(
            acceptance.post_call_acceptance_type_count,
            evaluation.acceptance_type_count()
        );

        assert_eq!(
            (
                acceptance.pass_acceptance_remaining,
                acceptance.pass_acceptance_type_count
            ),
            (8, 2)
        );
        assert_eq!(
            (
                acceptance.post_call_acceptance_remaining,
                acceptance.post_call_acceptance_type_count
            ),
            (20, 6)
        );
        assert_eq!(acceptance.acceptance_remaining_delta(), 12);
        assert_eq!(acceptance.acceptance_type_delta(), 4);

        // 観測用の値で、受け入れが増えても鳴かない判断のまま。
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_fixed_meld_yaku_guarantee_comes_from_the_shared_helper() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);

        let acceptance = candidate
            .iishanten_acceptance
            .expect("1向聴 → 1向聴 が対象");
        let melds = vec![Meld::new(
            MeldKind::Pon,
            tiles(&[
                IISHANTEN_PON_TARGET,
                IISHANTEN_PON_CONSUMED[0],
                IISHANTEN_PON_CONSUMED[1],
            ]),
            Some(tile(IISHANTEN_PON_TARGET)),
        )];

        assert_eq!(
            acceptance.fixed_melds_guarantee_yaku,
            fixed_melds_guarantee_yaku(&melds, damaten_baseline_context(&ctx))
        );
        assert!(acceptance.fixed_melds_guarantee_yaku);
    }

    #[test]
    fn a_chi_meld_does_not_guarantee_a_yaku() {
        let ctx = reaction_context(&IISHANTEN_CHI_HAND, IISHANTEN_CHI_TARGET);
        let action = chi_action(IISHANTEN_CHI_TARGET, &IISHANTEN_CHI_CONSUMED);
        let (_, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));

        let acceptance = candidate
            .iishanten_acceptance
            .expect("1向聴 → 1向聴 が対象");
        let melds = vec![Meld::new(
            MeldKind::Chi,
            tiles(&[
                IISHANTEN_CHI_TARGET,
                IISHANTEN_CHI_CONSUMED[0],
                IISHANTEN_CHI_CONSUMED[1],
            ]),
            Some(tile(IISHANTEN_CHI_TARGET)),
        )];

        assert_eq!(
            acceptance.fixed_melds_guarantee_yaku,
            fixed_melds_guarantee_yaku(&melds, damaten_baseline_context(&ctx))
        );
        assert!(!acceptance.fixed_melds_guarantee_yaku);
    }

    fn measured_call_decision(
        ctx: &GameContext,
        legal_actions: &[LegalAction],
        collect_observations: bool,
    ) -> (
        Option<CallDecisionDiagnostic>,
        CallDecisionDurations,
        Vec<CallCandidateDuration>,
    ) {
        let mut timing = CallDecisionTimer::armed();
        let decision =
            evaluate_call_decision(ctx, legal_actions, collect_observations, &mut timing);
        let (durations, candidates) = timing.finish();
        (decision, durations, candidates)
    }

    // Call / Pass の評価順を比べる focused fixture。
    //
    // - 単独の Call 候補 + 1向聴 Pass
    // - unique な Call 候補が複数 + 1向聴 Pass
    // - semantic に同一な重複候補を含む + 1向聴 Pass
    // - 現在2向聴から鳴いて1向聴になる候補 + 2向聴 Pass Full
    // - Pass の継続評価が不要な局面 (即テンパイ / 2向聴のまま / リーチ者あり / 反応元不明)
    fn call_pass_order_fixtures() -> Vec<(
        &'static str,
        GameContext,
        Vec<LegalAction>,
        Option<PassSelfTsumoContinuationKind>,
    )> {
        vec![
            (
                "single call candidate",
                valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
                Some(PassSelfTsumoContinuationKind::Iishanten),
            ),
            (
                "multiple unique call candidates",
                valued_reaction_context(
                    &IISHANTEN_CHI_GROUP_HAND,
                    IISHANTEN_CHI_GROUP_TARGET,
                    1,
                    63,
                ),
                iishanten_chi_group_actions(),
                Some(PassSelfTsumoContinuationKind::Iishanten),
            ),
            (
                "duplicate call candidates",
                valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
                Some(PassSelfTsumoContinuationKind::Iishanten),
            ),
            (
                "immediate tenpai call",
                valued_reaction_context(&TENPAI_PON_HAND, TENPAI_PON_TARGET, 1, 63),
                vec![
                    pon_action(TENPAI_PON_TARGET, &TENPAI_PON_CONSUMED),
                    LegalAction::None,
                ],
                None,
            ),
            (
                "two-shanten call to one shanten",
                valued_two_shanten_reaction_context(
                    &TWO_SHANTEN_CALL_PON_HAND,
                    TWO_SHANTEN_CALL_PON_TARGET,
                    Some(1),
                    Some(63),
                ),
                vec![
                    pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                    LegalAction::None,
                ],
                Some(PassSelfTsumoContinuationKind::TwoShanten),
            ),
            (
                "duplicate two-shanten call candidates",
                valued_two_shanten_reaction_context(
                    &TWO_SHANTEN_CALL_PON_HAND,
                    TWO_SHANTEN_CALL_PON_TARGET,
                    Some(1),
                    Some(63),
                ),
                vec![
                    pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                    pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                    LegalAction::None,
                ],
                Some(PassSelfTsumoContinuationKind::TwoShanten),
            ),
            (
                "two-shanten call that stays two shanten",
                valued_reaction_context(&RYANSHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
                None,
            ),
            (
                "an opponent has reached",
                reaction_context_with_reach(
                    &IISHANTEN_PON_HAND,
                    IISHANTEN_PON_TARGET,
                    [false, false, true, false],
                )
                .with_reaction_source_player(Some(1)),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
                None,
            ),
            (
                "the reaction source is unknown",
                reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
                None,
            ),
        ]
    }

    #[test]
    fn the_multiple_candidate_fixture_evaluates_three_unique_calls_against_one_pass() {
        // request 279 型の「複数の unique な Call 評価の group と Pass を重ねる」形を、
        // focused fixture が実際に再現していることを固定する。3件とも semantic に別の鳴きな
        // ので鳴き後の打牌選択を1件ずつ通り、そのうち1向聴のまま残る候補が Pass と比較される。
        let ctx =
            valued_reaction_context(&IISHANTEN_CHI_GROUP_HAND, IISHANTEN_CHI_GROUP_TARGET, 1, 63);
        let legal_actions = iishanten_chi_group_actions();
        let (decision, _, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        assert_eq!(
            decision.candidates.len(),
            IISHANTEN_CHI_GROUP_CONSUMED.len()
        );
        assert!(candidates.iter().all(|candidate| !candidate.reused));
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.post_call_discard_selection > Duration::ZERO)
        );
        assert!(
            decision
                .candidates
                .iter()
                .filter(|candidate| candidate.iishanten_self_tsumo.is_some())
                .count()
                > 1
        );
    }

    #[test]
    fn the_duplicate_candidate_fixture_reuses_the_post_call_evaluation() {
        // 重複候補の再利用 (#311) は評価順を変えても従来どおり1回だけ評価する。
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let legal_actions = [action.clone(), action, LegalAction::None];
        let (decision, _, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        assert_eq!(decision.candidates.len(), 2);
        assert_eq!(
            decision.candidates[0].iishanten_self_tsumo,
            decision.candidates[1].iishanten_self_tsumo
        );
        assert!(!candidates[0].reused);
        assert!(candidates[1].reused);
        assert_eq!(candidates[1].post_call_discard_selection, Duration::ZERO);
    }

    #[test]
    fn the_overlapped_call_pass_evaluation_is_bit_exact_with_the_sequential_one() {
        // 比べるのは判断そのもの。採用 action・理由・候補ごとの Call / Pass
        // ExpectedSelfTsumoValue・比較結果・鳴き後の選択打牌・診断がすべて一致する。
        for (label, ctx, legal_actions, _) in call_pass_order_fixtures() {
            for collect_observations in [false, true] {
                let sequential = evaluate_call_decision_with_order(
                    &ctx,
                    &legal_actions,
                    collect_observations,
                    CallPassEvaluationOrder::Sequential,
                    &mut CallDecisionTimer::disabled(),
                );
                let overlapped = evaluate_call_decision_with_order(
                    &ctx,
                    &legal_actions,
                    collect_observations,
                    CallPassEvaluationOrder::Overlapped,
                    &mut CallDecisionTimer::disabled(),
                );

                assert_eq!(sequential, overlapped, "{label} ({collect_observations})");
            }
        }
    }

    #[test]
    fn the_cheap_gate_matches_the_candidates_that_compare_the_call_and_the_pass() {
        // 重ねるかどうかを決める安価な事前判定は、Pass を1回評価する既存条件 (鳴き後の最良打牌
        // が1向聴になる Call 候補があり、反応元の席が分かる) と一致する。どちらの Pass を
        // 評価するかも、実際に比較へ入った候補の向聴数と一致する。推測で speculative に
        // 走らせない。
        for (label, ctx, legal_actions, expected) in call_pass_order_fixtures() {
            for collect_observations in [false, true] {
                let slots = prepare_call_candidates(
                    &ctx,
                    &legal_actions,
                    &mut CallDecisionTimer::disabled(),
                );
                let required = pass_continuation_is_required(&ctx, &slots);

                let decision = evaluate_call_decision(
                    &ctx,
                    &legal_actions,
                    collect_observations,
                    &mut CallDecisionTimer::disabled(),
                )
                .expect("evaluated");
                let compares_the_pass = reaction_draw_distance(&ctx).is_some().then(|| {
                    if decision
                        .candidates
                        .iter()
                        .any(|candidate| candidate.iishanten_self_tsumo.is_some())
                    {
                        return Some(PassSelfTsumoContinuationKind::Iishanten);
                    }
                    decision
                        .candidates
                        .iter()
                        .any(|candidate| candidate.two_shanten_self_tsumo.is_some())
                        .then_some(PassSelfTsumoContinuationKind::TwoShanten)
                });

                assert_eq!(
                    required,
                    compares_the_pass.flatten(),
                    "{label} ({collect_observations})"
                );
                assert_eq!(required, expected, "{label} ({collect_observations})");
            }
        }
    }

    #[test]
    fn a_position_without_the_comparison_does_not_evaluate_the_pass() {
        // Pass が不要な局面へ高コストな継続評価を足さない。計測は実際に走った時間だけを持つ。
        for (label, ctx, legal_actions, expected) in call_pass_order_fixtures() {
            if expected.is_some() {
                continue;
            }
            let (_, durations, _) = measured_call_decision(&ctx, &legal_actions, false);

            assert_eq!(
                durations.pass_iishanten_self_tsumo,
                Duration::ZERO,
                "{label}"
            );
            assert_eq!(
                durations.pass_two_shanten_self_tsumo,
                Duration::ZERO,
                "{label}"
            );
        }
    }

    // 同じ局面を S / P で交互に測り、方式ごとの中央値を並べる。
    //
    // ```text
    // cargo test --release -p bot-core --lib benchmark_call_pass_evaluation_order \
    //     -- --ignored --nocapture
    // ```
    #[test]
    #[ignore]
    fn benchmark_call_pass_evaluation_order() {
        let cases: [(&str, GameContext, Vec<LegalAction>); 5] = [
            (
                "single call candidate + pass",
                valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
            ),
            (
                "multiple unique call candidates + pass",
                valued_reaction_context(
                    &IISHANTEN_CHI_GROUP_HAND,
                    IISHANTEN_CHI_GROUP_TARGET,
                    1,
                    63,
                ),
                iishanten_chi_group_actions(),
            ),
            (
                "duplicate call candidate + pass",
                valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63),
                vec![
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
                    LegalAction::None,
                ],
            ),
            (
                "immediate tenpai call, no pass",
                valued_reaction_context(&TENPAI_PON_HAND, TENPAI_PON_TARGET, 1, 63),
                vec![
                    pon_action(TENPAI_PON_TARGET, &TENPAI_PON_CONSUMED),
                    LegalAction::None,
                ],
            ),
            (
                "two-shanten call to one shanten + two-shanten pass",
                valued_two_shanten_reaction_context(
                    &TWO_SHANTEN_CALL_PON_HAND,
                    TWO_SHANTEN_CALL_PON_TARGET,
                    Some(1),
                    Some(63),
                ),
                vec![
                    pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
                    LegalAction::None,
                ],
            ),
        ];

        println!("available_parallelism: {}", available_parallelism());
        for (label, ctx, legal_actions) in &cases {
            let mut measured: Vec<(CallPassEvaluationOrder, CallDecisionDurations)> = Vec::new();
            let mut decisions: Vec<(CallPassEvaluationOrder, Option<CallDecisionDiagnostic>)> =
                Vec::new();
            for _ in 0..BENCHMARK_ROUNDS {
                for order in [
                    CallPassEvaluationOrder::Sequential,
                    CallPassEvaluationOrder::Overlapped,
                ] {
                    let mut timing = CallDecisionTimer::armed();
                    let decision = evaluate_call_decision_with_order(
                        ctx,
                        legal_actions,
                        false,
                        order,
                        &mut timing,
                    );
                    let (durations, _) = timing.finish();
                    measured.push((order, durations));
                    decisions.push((order, decision));
                }
            }

            println!("\n{label}");
            let sequential = decisions
                .iter()
                .filter(|(order, _)| *order == CallPassEvaluationOrder::Sequential)
                .map(|(_, decision)| decision);
            let overlapped = decisions
                .iter()
                .filter(|(order, _)| *order == CallPassEvaluationOrder::Overlapped)
                .map(|(_, decision)| decision);
            let bit_exact = sequential
                .zip(overlapped)
                .all(|(sequential, overlapped)| sequential == overlapped);
            println!("  bit-exact decision: {bit_exact}");
            for order in [
                CallPassEvaluationOrder::Sequential,
                CallPassEvaluationOrder::Overlapped,
            ] {
                let runs: Vec<_> = measured
                    .iter()
                    .filter(|(measured_order, _)| *measured_order == order)
                    .map(|(_, durations)| *durations)
                    .collect();
                println!(
                    "  {order:?}: total {:?}, call candidates {:?}, 1向聴 pass {:?}, \
                     2向聴 pass {:?}",
                    median(runs.iter().map(|durations| durations.total)),
                    median(runs.iter().map(|durations| durations.candidates)),
                    median(
                        runs.iter()
                            .map(|durations| durations.pass_iishanten_self_tsumo)
                    ),
                    median(
                        runs.iter()
                            .map(|durations| durations.pass_two_shanten_self_tsumo)
                    ),
                );
            }
        }
    }

    // Call を評価した thread で続けて Pass を評価した場合と、まっさらな thread で Pass を
    // 評価した場合を、CPU 競合の無い状態で比べる。差は thread 分離で失う thread-local memo
    // (向聴 / 受け入れなど) の分で、重ねることで増える総仕事量そのものになる。
    //
    // ```text
    // cargo test --release -p bot-core --lib benchmark_pass_continuation_thread_locality \
    //     -- --ignored --nocapture
    // ```
    #[test]
    #[ignore]
    fn benchmark_pass_continuation_thread_locality() {
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63);
        let legal_actions = [
            pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
            LegalAction::None,
        ];
        for _ in 0..5 {
            let warm = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let _ = evaluate_call_decision_with_order(
                            &ctx,
                            &legal_actions,
                            false,
                            CallPassEvaluationOrder::Sequential,
                            &mut CallDecisionTimer::disabled(),
                        );
                        let since = Instant::now();
                        let value = pass_iishanten_expected_self_tsumo_value(&ctx);
                        (value, since.elapsed())
                    })
                    .join()
                    .expect("warm")
            });
            let cold = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let since = Instant::now();
                        let value = pass_iishanten_expected_self_tsumo_value(&ctx);
                        (value, since.elapsed())
                    })
                    .join()
                    .expect("cold")
            });
            assert_eq!(warm.0, cold.0);
            println!("warm thread {:?}  cold thread {:?}", warm.1, cold.1);
        }
    }

    const BENCHMARK_ROUNDS: usize = 7;

    fn median(durations: impl Iterator<Item = Duration>) -> Duration {
        let mut durations: Vec<_> = durations.collect();
        durations.sort();
        durations[durations.len() / 2]
    }

    #[test]
    fn a_request_without_a_legal_call_measures_nothing() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let legal_actions = [
            LegalAction::Dahai {
                tile: tile(IISHANTEN_PON_HAND[0]),
            },
            LegalAction::None,
        ];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);

        assert_eq!(decision, None);
        assert_eq!(durations, CallDecisionDurations::default());
        assert!(candidates.is_empty());
    }

    #[test]
    fn every_call_candidate_is_measured_in_the_legal_action_order() {
        // 重複候補も行をまとめず、合法 action の順にそれぞれ1件ずつ並ぶ。semantic に同一な
        // 2件目は評価を再利用するので、実測は 0 で reused になる。
        let chi = chi_action(IISHANTEN_CHI_TARGET, &IISHANTEN_CHI_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_CHI_HAND, IISHANTEN_CHI_TARGET, 1, 12);
        let legal_actions = [chi.clone(), chi.clone(), LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);

        assert_eq!(decision.expect("evaluated").candidates.len(), 2);
        assert_eq!(candidates.len(), 2);
        for candidate in &candidates {
            assert_eq!(candidate.kind, CallKind::Chi);
            assert_eq!(candidate.tile, tile(IISHANTEN_CHI_TARGET));
            assert_eq!(candidate.consumed, tiles(&IISHANTEN_CHI_CONSUMED));
        }
        assert!(!candidates[0].reused);
        assert!(candidates[0].elapsed > Duration::ZERO);
        assert!(candidates[0].post_call_discard_selection > Duration::ZERO);
        assert!(candidates[0].post_call_discard_selection <= candidates[0].elapsed);
        assert!(candidates[1].reused);
        assert_eq!(candidates[1].elapsed, Duration::ZERO);
        assert_eq!(candidates[1].post_call_discard_selection, Duration::ZERO);
        assert_eq!(
            durations.candidates,
            candidates
                .iter()
                .map(|candidate| candidate.elapsed)
                .sum::<Duration>()
        );
        assert!(durations.total >= durations.candidates);
    }

    #[test]
    fn the_shared_iishanten_pass_comparison_is_measured_once() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let legal_actions = [action, LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let candidate = &decision.expect("evaluated").candidates[0];

        assert!(candidate.iishanten_self_tsumo.is_some());
        assert!(durations.pass_iishanten_self_tsumo > Duration::ZERO);
        // total は壁時計なので、Call 側と Pass 側を重ねた分だけ内訳の合計より短くなり得る。
        // 内訳のどちらか一方を下回ることはない。
        assert!(durations.total >= durations.candidates);
        assert!(durations.total >= durations.pass_iishanten_self_tsumo);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn the_pass_continuation_overlaps_the_call_candidate_group() {
        // 重ねられる runtime では、Call 側の候補評価と Pass 側の継続評価の合計が鳴き判断全体の
        // 壁時計を超える。どちらも実際に走った時間で、待ち時間は含まない。
        if !call_pass_overlap_is_available() {
            return;
        }

        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 63);
        let legal_actions = [action, LegalAction::None];
        let (decision, durations, _) = measured_call_decision(&ctx, &legal_actions, false);
        let candidate = &decision.expect("evaluated").candidates[0];

        assert!(candidate.iishanten_self_tsumo.is_some());
        assert!(durations.candidates > Duration::ZERO);
        assert!(durations.pass_iishanten_self_tsumo > Duration::ZERO);
        assert!(durations.total < durations.candidates + durations.pass_iishanten_self_tsumo);
        assert_eq!(durations.remaining(), Duration::ZERO);
    }

    #[test]
    fn the_shared_two_shanten_pass_comparison_is_measured_once() {
        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(32),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let legal_actions = [action, LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let candidate = &decision.expect("evaluated").candidates[0];

        assert!(candidate.two_shanten_self_tsumo.is_some());
        assert!(durations.pass_two_shanten_self_tsumo > Duration::ZERO);
        // 現在の向聴数がどちらの Pass を評価するかを決めるので、1向聴側は 0 のままになる。
        assert_eq!(durations.pass_iishanten_self_tsumo, Duration::ZERO);
        assert!(durations.total >= durations.candidates);
        assert!(durations.total >= durations.pass_two_shanten_self_tsumo);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn the_two_shanten_pass_full_overlaps_the_call_candidate_group() {
        // 2向聴 Pass Full も1向聴 Pass と同じ経路で Call 側の deep 評価と重なる。内訳は
        // どちらも実際に走った時間で、join の待ち時間は含まない。
        if !call_pass_overlap_is_available() {
            return;
        }

        let ctx = valued_two_shanten_reaction_context(
            &TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            Some(1),
            Some(63),
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let legal_actions = [action, LegalAction::None];
        let (decision, durations, _) = measured_call_decision(&ctx, &legal_actions, false);
        let candidate = &decision.expect("evaluated").candidates[0];

        assert!(candidate.two_shanten_self_tsumo.is_some());
        assert!(durations.candidates > Duration::ZERO);
        assert!(durations.pass_two_shanten_self_tsumo > Duration::ZERO);
        assert!(durations.total < durations.candidates + durations.pass_two_shanten_self_tsumo);
        assert_eq!(durations.remaining(), Duration::ZERO);
    }

    #[test]
    fn a_call_without_the_iishanten_comparison_does_not_measure_the_pass() {
        // 即テンパイ候補は Call / Pass 比較へ入らないので、Pass の計測も 0 のままになる。
        let ctx = reaction_context(&TENPAI_PON_HAND, TENPAI_PON_TARGET);
        let action = pon_action(TENPAI_PON_TARGET, &TENPAI_PON_CONSUMED);
        let legal_actions = [action.clone(), LegalAction::None];
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        assert_eq!(decision.selected, Some(action));
        assert_eq!(decision.candidates[0].iishanten_self_tsumo, None);
        assert_eq!(durations.pass_iishanten_self_tsumo, Duration::ZERO);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].elapsed > Duration::ZERO);
    }

    #[test]
    fn the_call_decision_is_the_same_with_and_without_the_timing_and_the_diagnostics() {
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let legal_actions = [action.clone(), LegalAction::None];
        let untimed = evaluate_call_decision(
            &ctx,
            &legal_actions,
            false,
            &mut CallDecisionTimer::disabled(),
        )
        .expect("evaluated");
        let (timed, _, _) = measured_call_decision(&ctx, &legal_actions, false);
        let (diagnosed, _, _) = measured_call_decision(&ctx, &legal_actions, true);

        assert_eq!(untimed.selected, Some(action));
        assert_eq!(timed.expect("evaluated").selected, untimed.selected);
        assert_eq!(diagnosed.expect("evaluated").selected, untimed.selected);
    }

    #[test]
    fn an_immediate_tenpai_call_keeps_the_existing_eligible_tenpai_decision() {
        let ctx = reaction_context(&TENPAI_PON_HAND, TENPAI_PON_TARGET);
        let action = pon_action(TENPAI_PON_TARGET, &TENPAI_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::EligibleTenpai);
        assert!(candidate.eligible);
        assert_eq!(decision.selected.as_ref(), Some(&action));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_TENPAI_SHANTEN));
        // 即テンパイ候補は既存診断で足りるので、1向聴 → 1向聴 の観測対象にしない。
        assert_eq!(candidate.iishanten_acceptance, None);

        // 診断を集めない通常経路でも同じ判断。
        let (production, production_candidate) = single_candidate(&ctx, &action, false);
        assert_eq!(production_candidate, candidate);
        assert_eq!(production.selected, decision.selected);
    }

    // 2m Pon + 3p Pon + 55p 7s8s 5m 8p 2s の2向聴。7s8s で 9s を Chi すると1向聴になるが、
    // 役牌も断么九も残らず、鳴かずに進めた方が self-tsumo value が高い。
    const TWO_SHANTEN_YAKULESS_CHI_HAND: [u8; 7] = [53, 54, 96, 100, 17, 64, 76];
    const TWO_SHANTEN_YAKULESS_CHI_TARGET: u8 = 104;
    const TWO_SHANTEN_YAKULESS_CHI_CONSUMED: [u8; 2] = [96, 100];

    fn two_shanten_yakuless_chi_melds() -> Vec<Meld> {
        vec![
            Meld::new(MeldKind::Pon, tiles(&[4, 5, 6]), Some(tile(4))),
            Meld::new(MeldKind::Pon, tiles(&[44, 45, 46]), Some(tile(44))),
        ]
    }

    #[test]
    fn a_two_shanten_pass_with_a_higher_expected_self_tsumo_value_is_kept() {
        let ctx = two_shanten_reaction_context_with_melds(
            &TWO_SHANTEN_YAKULESS_CHI_HAND,
            two_shanten_yakuless_chi_melds(),
            TWO_SHANTEN_YAKULESS_CHI_TARGET,
            Some(1),
            Some(56),
        );
        let action = chi_action(
            TWO_SHANTEN_YAKULESS_CHI_TARGET,
            &TWO_SHANTEN_YAKULESS_CHI_CONSUMED,
        );
        let (decision, candidate) = single_candidate(&ctx, &action, true);
        let comparison = candidate.two_shanten_self_tsumo.expect("比較対象");

        assert_eq!(candidate.current_shanten, Some(CALL_TWO_SHANTEN_SHANTEN));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));
        assert_eq!(comparison.comparison, CallIishantenComparison::PassNotLower);
        assert!(
            comparison.pass_expected_self_tsumo_value > comparison.call_expected_self_tsumo_value
        );
        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn two_shanten_call_that_stays_two_shanten_is_not_compared() {
        // 対象は鳴き後1向聴になる候補だけ。2向聴のままの鳴きは従来どおり Pass で、Call / Pass
        // の比較も Pass 側の Full 評価も行わない。
        let ctx = reaction_context(&RYANSHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, true);

        assert_eq!(candidate.reason, CallDecisionReason::PostCallNotIishanten);
        assert!(!candidate.eligible);
        assert_eq!(candidate.current_shanten, Some(CALL_TWO_SHANTEN_SHANTEN));
        assert_eq!(
            candidate.post_call_shanten(),
            Some(CALL_TWO_SHANTEN_SHANTEN)
        );
        assert_eq!(candidate.iishanten_acceptance, None);
        assert_eq!(candidate.two_shanten_self_tsumo, None);
        assert_eq!(decision.selected, None);
    }

    #[test]
    fn the_iishanten_acceptance_is_not_collected_without_diagnostics() {
        let ctx = reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, false);

        // 解析専用の観測値なので、通常の判断経路では構築しない。
        assert_eq!(candidate.iishanten_acceptance, None);

        // 判断に使う fact と結論は診断の有無で変わらない。
        assert_eq!(candidate.reason, CallDecisionReason::ReactionSourceUnknown);
        assert_eq!(candidate.current_shanten, Some(CALL_CURRENT_SHANTEN));
        assert_eq!(candidate.post_call_shanten(), Some(CALL_CURRENT_SHANTEN));
        assert!(!candidate.eligible);
        assert_eq!(decision.selected, None);

        let (_, diagnosed) = single_candidate(&ctx, &action, true);
        assert!(diagnosed.iishanten_acceptance.is_some());
        assert_eq!(
            CallCandidateDiagnostic {
                iishanten_acceptance: None,
                ..diagnosed
            },
            candidate
        );
    }

    #[test]
    fn the_iishanten_acceptance_diagnostic_does_not_change_the_selected_action() {
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let actions = [action.clone(), LegalAction::None];

        let mut agent = crate::agents::ShantenAgent;
        let acted = crate::agent::Agent::act(&mut agent, &ctx, &actions);
        assert_eq!(acted, action);

        // diagnose() は観測値を集めるが、選ぶ action は act() と同じ。
        let diagnostic = crate::agents::ShantenAgent::diagnose(&ctx, &actions);
        assert_eq!(diagnostic.selected_action, acted);
        let call = diagnostic.call.as_ref().expect("evaluated");
        assert_eq!(call.selected, Some(acted));
        assert!(call.candidates[0].iishanten_acceptance.is_some());
        assert_eq!(
            call.candidates[0]
                .iishanten_self_tsumo
                .expect("production comparison")
                .comparison,
            CallIishantenComparison::CallHigher
        );
    }

    #[test]
    fn normalizes_only_chi_and_pon() {
        let chi = LegalAction::Chi {
            tile: tile(89),
            consumed: tiles(&[84, 92]),
        };
        let pon = LegalAction::Pon {
            tile: tile(126),
            consumed: tiles(&[124, 125]),
        };

        assert_eq!(
            normalize_call(&chi).map(|(kind, ..)| kind),
            Some(CallKind::Chi)
        );
        assert_eq!(
            normalize_call(&pon).map(|(kind, ..)| kind),
            Some(CallKind::Pon)
        );

        for action in [
            LegalAction::Daiminkan {
                tile: tile(104),
                consumed: tiles(&[105, 106, 107]),
            },
            LegalAction::Ankan {
                consumed: tiles(&[72, 73, 74, 75]),
            },
            LegalAction::Kakan {
                tile: tile(124),
                consumed: tiles(&[125, 126, 127]),
            },
            LegalAction::Dahai { tile: tile(0) },
            LegalAction::Reach,
            LegalAction::None,
        ] {
            assert!(normalize_call(&action).is_none(), "{action:?}");
        }
    }

    #[test]
    fn builds_the_pon_meld_and_removes_the_consumed_physical_tiles() {
        let hand = tiles(&[0, 124, 125, 126]);
        let (meld, remaining) =
            call_meld_and_concealed_tiles(&hand, CallKind::Pon, tile(127), &tiles(&[124, 125]))
                .unwrap();

        assert_eq!(meld.kind(), MeldKind::Pon);
        assert_eq!(meld.called_tile(), Some(tile(127)));
        assert!(meld.shape().unwrap().is_triplet_like());
        // 暗刻から鳴いた場合も、除去は consumed の物理牌2枚だけ。
        assert_eq!(remaining, tiles(&[0, 126]));
    }

    #[test]
    fn builds_the_chi_meld_and_removes_the_consumed_physical_tiles() {
        let hand = tiles(&[0, 84, 92]);
        let (meld, remaining) =
            call_meld_and_concealed_tiles(&hand, CallKind::Chi, tile(89), &tiles(&[84, 92]))
                .unwrap();

        assert_eq!(meld.kind(), MeldKind::Chi);
        assert_eq!(
            meld.shape(),
            Some(MeldShape::Sequence {
                start: tile(84).tile_type()
            })
        );
        assert_eq!(remaining, tiles(&[0]));
    }

    #[test]
    fn rejects_calls_that_cannot_build_a_meld() {
        let hand = tiles(&[0, 84, 92, 124, 125, 126]);

        for (kind, called, consumed) in [
            // 枚数が2枚でない
            (CallKind::Pon, 127u8, vec![124u8]),
            (CallKind::Pon, 127, vec![124, 125, 126]),
            // 手牌に無い物理牌
            (CallKind::Pon, 127, vec![124, 127]),
            // 同じ物理牌の重複
            (CallKind::Pon, 127, vec![124, 124]),
            // 刻子にならない
            (CallKind::Pon, 127, vec![124, 0]),
            // 順子にならない
            (CallKind::Chi, 89, vec![124, 125]),
            (CallKind::Chi, 89, vec![0, 84]),
        ] {
            assert!(
                call_meld_and_concealed_tiles(&hand, kind, tile(called), &tiles(&consumed))
                    .is_none(),
                "{kind:?} {called} {consumed:?}"
            );
        }
    }

    #[test]
    fn every_live_variant_needs_a_yaku() {
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 2, CallWaitYaku::Present),
                wait(100, 4, CallWaitYaku::Present),
            ]),
            CallDecisionReason::EligibleTenpai
        );
    }

    #[test]
    fn a_live_variant_without_a_yaku_blocks_the_call() {
        // 役なしが確定した variant は、確定できない variant より先に理由になる。
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 2, CallWaitYaku::Present),
                wait(100, 1, CallWaitYaku::Absent),
                wait(104, 3, CallWaitYaku::Unknown),
            ]),
            CallDecisionReason::YakuMissing
        );
    }

    #[test]
    fn an_indeterminate_live_variant_blocks_the_call() {
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 2, CallWaitYaku::Present),
                wait(100, 1, CallWaitYaku::Unknown),
            ]),
            CallDecisionReason::HandValueUnknown
        );
    }

    // 3m を 4m5m で Chi できる 4m が2枚ある一向聴。456m 78s ではなく 678m 234s を使う形で、
    // 消費する物理牌だけが違う同じ Chi が2件並ぶ。鳴いた後は余った 4m を切ると 3p / 7s の
    // シャンポン待ちテンパイになり、全て中張牌なので役もある。
    const DUPLICATE_CHI_HAND: [u8; 13] = [12, 13, 17, 20, 24, 28, 44, 45, 76, 80, 84, 96, 97];
    const DUPLICATE_CHI_TARGET: u8 = 8;
    const DUPLICATE_CHI_CONSUMED: [[u8; 2]; 2] = [[12, 17], [13, 17]];

    // 同じ形で 4m を1枚にし、5m を赤5と黒5の2枚にしたもの。2件の Chi は consumed の赤5と、
    // 鳴き後に手牌へ残る5の赤5が入れ替わる。
    const RED_FIVE_CHI_HAND: [u8; 13] = [12, 16, 17, 20, 24, 28, 44, 45, 76, 80, 84, 96, 97];
    const RED_FIVE_CHI_CONSUMED: [[u8; 2]; 2] = [[12, 16], [12, 17]];

    fn duplicate_chi_actions(consumed: &[[u8; 2]; 2]) -> Vec<LegalAction> {
        consumed
            .iter()
            .map(|consumed| chi_action(DUPLICATE_CHI_TARGET, consumed))
            .collect()
    }

    // 候補1件を単独で評価した結果。dedup で複製した候補が、独立に評価した場合と同じ内容かを
    // 比べるための基準にする。`selected` は候補集合で決まるので比較対象から外す。
    fn independently_evaluated_candidate(
        ctx: &GameContext,
        action: &LegalAction,
        collect_observations: bool,
    ) -> CallCandidateDiagnostic {
        let (_, mut candidate) = single_candidate(ctx, action, collect_observations);
        candidate.selected = false;
        candidate
    }

    #[test]
    fn semantically_equal_calls_evaluate_the_post_call_discard_once() {
        let ctx = valued_reaction_context(&DUPLICATE_CHI_HAND, DUPLICATE_CHI_TARGET, 1, 12);
        let actions = duplicate_chi_actions(&DUPLICATE_CHI_CONSUMED);
        let mut legal_actions = actions.clone();
        legal_actions.push(LegalAction::None);
        let (decision, durations, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        // 件数・順序・action は元の合法 action のまま。
        assert_eq!(decision.candidates.len(), actions.len());
        for (candidate, action) in decision.candidates.iter().zip(&actions) {
            assert_eq!(&candidate.action, action);
        }

        // 高コストな鳴き後の打牌評価は1回だけ。
        assert_eq!(candidates.len(), actions.len());
        assert!(!candidates[0].reused);
        assert!(candidates[0].post_call_discard_selection > Duration::ZERO);
        assert!(candidates[1].reused);
        assert_eq!(candidates[1].elapsed, Duration::ZERO);
        assert_eq!(candidates[1].post_call_discard_selection, Duration::ZERO);
        assert_eq!(durations.candidates, candidates[0].elapsed);

        // 再利用した候補は consumed の物理牌だけが違い、判断内容は独立評価と一致する。
        assert_eq!(candidates[1].consumed, tiles(&DUPLICATE_CHI_CONSUMED[1]));
        for (candidate, action) in decision.candidates.iter().zip(&actions) {
            let mut expected = independently_evaluated_candidate(&ctx, action, false);
            expected.selected = candidate.selected;
            assert_eq!(candidate, &expected);
        }
    }

    #[test]
    fn semantically_equal_calls_keep_the_first_legal_action_as_the_selected_call() {
        let ctx = valued_reaction_context(&DUPLICATE_CHI_HAND, DUPLICATE_CHI_TARGET, 1, 12);
        let actions = duplicate_chi_actions(&DUPLICATE_CHI_CONSUMED);
        let mut legal_actions = actions.clone();
        legal_actions.push(LegalAction::None);
        let decision = evaluate_call_decision(
            &ctx,
            &legal_actions,
            false,
            &mut CallDecisionTimer::disabled(),
        )
        .expect("evaluated");

        // 完全同値の候補なので、既存 tie-break どおり先頭の合法 action を採る。
        assert_eq!(decision.reason, CallDecisionReason::EligibleTenpai);
        assert_eq!(decision.selected.as_ref(), Some(&actions[0]));
        assert!(decision.candidates[0].selected);
        assert!(!decision.candidates[1].selected);

        // 候補が1件だけの場合と同じ action を選ぶ。
        let (single, _) = single_candidate(&ctx, &actions[0], false);
        assert_eq!(single.selected, decision.selected);
    }

    #[test]
    fn calls_that_differ_only_in_the_red_five_are_evaluated_separately() {
        let ctx = valued_reaction_context(&RED_FIVE_CHI_HAND, DUPLICATE_CHI_TARGET, 1, 12);
        let actions = duplicate_chi_actions(&RED_FIVE_CHI_CONSUMED);
        let mut legal_actions = actions.clone();
        legal_actions.push(LegalAction::None);
        let (decision, _, candidates) = measured_call_decision(&ctx, &legal_actions, false);
        let decision = decision.expect("evaluated");

        assert_eq!(decision.candidates.len(), actions.len());
        for (candidate, action) in decision.candidates.iter().zip(&actions) {
            assert_eq!(&candidate.action, action);
        }
        // 表示上は同じ 3m<-4m,5m でも赤5の位置が違うので、どちらも実際に評価する。
        assert_eq!(candidates.len(), actions.len());
        for candidate in &candidates {
            assert!(!candidate.reused);
            assert!(candidate.post_call_discard_selection > Duration::ZERO);
        }
    }

    fn evaluation_key(hand: &[u8], target: u8, consumed: &[u8]) -> CallEvaluationKey {
        let (meld, post_call_tiles) = call_meld_and_concealed_tiles(
            &tiles(hand),
            CallKind::Chi,
            tile(target),
            &tiles(consumed),
        )
        .expect("valid chi");
        CallEvaluationKey::new(&meld, &post_call_tiles)
    }

    #[test]
    fn the_semantic_key_only_ignores_the_physical_copy_of_the_same_tile() {
        // 同じ牌種・同じ赤黒の別コピーを消費する Chi は同じ key。
        assert_eq!(
            evaluation_key(
                &DUPLICATE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &DUPLICATE_CHI_CONSUMED[0]
            ),
            evaluation_key(
                &DUPLICATE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &DUPLICATE_CHI_CONSUMED[1]
            )
        );

        // 赤5と黒5のどちらを鳴くかは別 key。
        assert_ne!(
            evaluation_key(
                &RED_FIVE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &RED_FIVE_CHI_CONSUMED[0]
            ),
            evaluation_key(
                &RED_FIVE_CHI_HAND,
                DUPLICATE_CHI_TARGET,
                &RED_FIVE_CHI_CONSUMED[1]
            )
        );

        // 喰い替え禁止牌が変わる鳴き方も別 key。
        assert_ne!(
            evaluation_key(&DUPLICATE_CHI_HAND, 20, &[12, 17]),
            evaluation_key(&DUPLICATE_CHI_HAND, DUPLICATE_CHI_TARGET, &[12, 17])
        );
    }

    #[test]
    fn dead_variants_are_not_part_of_the_yaku_judgement() {
        assert_eq!(
            live_wait_yaku_reason(&[
                wait(89, 3, CallWaitYaku::Present),
                wait(100, 0, CallWaitYaku::Absent),
                wait(104, 0, CallWaitYaku::Unknown),
            ]),
            CallDecisionReason::EligibleTenpai
        );
    }

    // ---- 鳴き後 Push/Pull gate ----

    // 反応元でない player 2 の3副露。既存 OpenHandThreat の Danger になる。副露した牌は、どの
    // 手牌の受け入れにも関わらない 1s / 9p / 9s。
    fn danger_open_hand_melds() -> Vec<Meld> {
        [72, 68, 104]
            .map(|first| {
                Meld::new(
                    MeldKind::Pon,
                    tiles(&[first, first + 1, first + 2]),
                    Some(tile(first)),
                )
            })
            .to_vec()
    }

    // player 2 に Danger の副露を置いた reaction 局面。副露した牌は見え牌にも加える。
    fn threatened_reaction_context(
        hand: &[u8],
        own_melds: Vec<Meld>,
        target: u8,
        remaining_tiles: u32,
    ) -> GameContext {
        let hand_tiles = tiles(hand);
        let opponent_melds = danger_open_hand_melds();
        let mut visible = hand_tiles.clone();
        visible.push(tile(target));
        visible.extend(
            own_melds
                .iter()
                .chain(&opponent_melds)
                .flat_map(|meld| meld.tiles().iter().copied()),
        );

        GameContext::from_parts_with_melds(
            None,
            hand_tiles,
            vec![],
            TileType::new(EAST),
            TileType::new(EAST),
            visible,
            Some(0),
            Some(0),
            [vec![], vec![tile(target)], vec![], vec![]],
            [false; 4],
            [own_melds, vec![], opponent_melds, vec![]],
        )
        .with_history_furiten_facts(bot_logic::HistoryFuritenFacts {
            same_turn: Some(false),
            riichi_missed_win: Some(false),
        })
        .with_reaction_source_player(Some(1))
        .with_table_state_facts(crate::context::TableStateFacts {
            remaining_tiles: Some(remaining_tiles),
            ..Default::default()
        })
    }

    // low_value_two_shanten_reaction_context と同じ 白 Pon + 234m Chi を持つ局面。役牌2翻の
    // 手なので、鳴き後1向聴の ExpectedSelfTsumoValue は押し引きの threshold に届かない。
    fn threatened_low_value_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: u32,
    ) -> GameContext {
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Chi, tiles(&[4, 8, 12]), Some(tile(4))),
        ];
        threatened_reaction_context(hand, melds, target, remaining_tiles)
    }

    // valued_two_shanten_reaction_context と同じ 白 Pon + 發 Pon を持つ局面。
    fn threatened_two_shanten_reaction_context(
        hand: &[u8],
        target: u8,
        remaining_tiles: u32,
    ) -> GameContext {
        let melds = vec![
            Meld::new(MeldKind::Pon, tiles(&[124, 125, 126]), Some(tile(124))),
            Meld::new(MeldKind::Pon, tiles(&[128, 129, 130]), Some(tile(128))),
        ];
        threatened_reaction_context(hand, melds, target, remaining_tiles)
    }

    // 同じ局面で、鳴き後の選択打牌だけを Danger の相手に対する一時通過牌にする。見え牌も
    // 手牌も変えないので、Call / Pass 比較と鳴き後の打牌選択は変わらない。
    fn with_selected_discard_passed_by_the_threat(
        ctx: &GameContext,
        candidate: &CallCandidateDiagnostic,
    ) -> GameContext {
        let discard = candidate
            .post_call_discard
            .as_ref()
            .expect("鳴き後打牌")
            .discard;
        let mut passed: [Vec<TileType>; 4] = Default::default();
        passed[2] = vec![discard];
        ctx.clone().with_temporary_passed_tiles(Some(passed))
    }

    // Call / Pass 比較で成立した候補が、鳴き後 Push/Pull の Fold で不成立になり、選択打牌が
    // hard-safe な同じ局面では従来どおり成立することを確かめる。
    fn assert_post_call_push_pull_gate(
        ctx: &GameContext,
        action: &LegalAction,
        eligible: CallDecisionReason,
        fold: PushPullReason,
        safe: PushPullReason,
    ) {
        let (decision, folded) = single_candidate(ctx, action, false);
        assert_eq!(folded.reason, CallDecisionReason::PostCallNotPush);
        assert!(!folded.eligible);
        assert_eq!(decision.selected, None);
        assert_eq!(decision.reason, CallDecisionReason::PostCallNotPush);
        assert_eq!(folded.call_pass_eligible_reason(), Some(eligible));
        assert_eq!(
            folded.post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Fold,
                reason: fold,
            })
        );

        let safe_ctx = with_selected_discard_passed_by_the_threat(ctx, &folded);
        let (decision, pushed) = single_candidate(&safe_ctx, action, false);
        assert_eq!(pushed.reason, eligible);
        assert!(pushed.eligible);
        assert_eq!(decision.selected.as_ref(), Some(action));
        assert_eq!(pushed.call_pass_eligible_reason(), Some(eligible));
        assert_eq!(
            pushed.post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Push,
                reason: safe,
            })
        );
        // gate は Call / Pass 比較にも鳴き後の打牌選択にも触れない。
        assert_eq!(pushed.post_call_discard, folded.post_call_discard);
        assert_eq!(pushed.iishanten_self_tsumo, folded.iishanten_self_tsumo);
        assert_eq!(pushed.two_shanten_self_tsumo, folded.two_shanten_self_tsumo);
        assert_eq!(
            pushed.three_shanten_self_tsumo,
            folded.three_shanten_self_tsumo
        );
    }

    #[test]
    fn an_iishanten_call_that_stays_iishanten_is_declined_when_the_post_call_push_pull_folds() {
        let ctx =
            threatened_reaction_context(&IISHANTEN_PON_HAND, vec![], IISHANTEN_PON_TARGET, 12);
        assert_post_call_push_pull_gate(
            &ctx,
            &pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED),
            CallDecisionReason::EligibleIishantenSelfTsumo,
            PushPullReason::IishantenAgainstHighOpenHand,
            PushPullReason::SafeIishantenAgainstHighOpenHand,
        );
    }

    #[test]
    fn a_two_shanten_call_to_iishanten_is_declined_when_the_post_call_push_pull_folds() {
        let ctx = threatened_low_value_two_shanten_reaction_context(
            &LOW_VALUE_TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            32,
        );
        assert_post_call_push_pull_gate(
            &ctx,
            &pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED),
            CallDecisionReason::EligibleTwoShantenSelfTsumo,
            PushPullReason::IishantenAgainstHighOpenHand,
            PushPullReason::SafeIishantenAgainstHighOpenHand,
        );
    }

    #[test]
    fn a_three_shanten_call_to_two_shanten_is_declined_when_the_post_call_push_pull_folds() {
        let ctx = threatened_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            THREE_SHANTEN_SPEED_REMAINING,
        );
        assert_post_call_push_pull_gate(
            &ctx,
            &pon_action(
                THREE_SHANTEN_CALL_PON_TARGET,
                &THREE_SHANTEN_CALL_PON_CONSUMED,
            ),
            CallDecisionReason::EligibleThreeShantenSelfTsumo,
            PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
            PushPullReason::SafeTwoShantenAgainstHighOpenHand,
        );
    }

    #[test]
    fn a_non_tenpai_call_without_a_clear_threat_keeps_the_call_pass_decision() {
        // threat がいなければ既存 Push/Pull は NoThreat で押すので、Call / Pass 比較の結論が
        // そのまま残る。
        let ctx = valued_reaction_context(&IISHANTEN_PON_HAND, IISHANTEN_PON_TARGET, 1, 12);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, false);

        assert_eq!(
            candidate.reason,
            CallDecisionReason::EligibleIishantenSelfTsumo
        );
        assert_eq!(decision.selected, Some(action));
        assert_eq!(
            candidate.post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Push,
                reason: PushPullReason::NoThreat,
            })
        );
    }

    #[test]
    fn a_call_rejected_by_the_call_pass_comparison_keeps_its_reason() {
        // Call / Pass 比較で落ちた候補には鳴き後 Push/Pull を評価せず、理由を上書きしない。
        let ctx =
            threatened_reaction_context(&IISHANTEN_PON_HAND, vec![], IISHANTEN_PON_TARGET, 63);
        let action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
        let (decision, candidate) = single_candidate(&ctx, &action, false);

        assert_eq!(candidate.reason, CallDecisionReason::PassSelfTsumoNotLower);
        assert_eq!(decision.selected, None);
        assert_eq!(candidate.call_pass_eligible_reason(), None);
        assert_eq!(candidate.post_call_push_pull, None);
    }

    // 高コストな評価まで済ませた候補と、その鳴き後 state。Call / Pass policy は適用しない。
    fn evaluated_candidates(
        ctx: &GameContext,
        action: &LegalAction,
    ) -> (Vec<CallCandidateDiagnostic>, Vec<PostCallState>) {
        let mut timing = CallDecisionTimer::disabled();
        let mut slots = prepare_call_candidates(ctx, std::slice::from_ref(action), &mut timing);
        evaluate_call_candidate_group(ctx, &mut slots, false, &mut timing);
        into_call_candidates(slots)
    }

    fn pass(kind: PassSelfTsumoContinuationKind, value: Option<u64>) -> PassSelfTsumoContinuation {
        PassSelfTsumoContinuation {
            kind,
            value,
            elapsed: Duration::ZERO,
        }
    }

    #[test]
    fn the_post_call_push_pull_gate_also_applies_to_a_speed_override() {
        // 値比較では Pass でも速度優先 policy が成立させた Call も、同じ gate を通る。Pass 値を
        // Call と同値にして値比較の結論を Pass にし、速度優先の判断材料は成立した値に置く。
        // 鳴き後の state・打牌選択・前方集計値は実際の評価のまま使う。
        let two_shanten = threatened_low_value_two_shanten_reaction_context(
            &LOW_VALUE_TWO_SHANTEN_CALL_PON_HAND,
            TWO_SHANTEN_CALL_PON_TARGET,
            40,
        );
        let action = pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);
        let (mut candidates, post_call_states) = evaluated_candidates(&two_shanten, &action);
        let comparison = candidates[0]
            .two_shanten_self_tsumo
            .as_mut()
            .expect("2向聴 Call / Pass 比較対象");
        comparison.speed = satisfied_speed_facts();
        let call_value = comparison.call_expected_self_tsumo_value;
        apply_two_shanten_self_tsumo_policy(
            &two_shanten,
            &mut candidates,
            Some(pass(PassSelfTsumoContinuationKind::TwoShanten, call_value)),
            &mut CallDecisionTimer::disabled(),
        );
        assert_eq!(
            candidates[0].reason,
            CallDecisionReason::EligibleTwoShantenSpeed
        );
        apply_post_call_push_pull_gate(&two_shanten, &mut candidates, &post_call_states);
        assert_eq!(candidates[0].reason, CallDecisionReason::PostCallNotPush);
        assert!(!candidates[0].eligible);
        assert_eq!(
            candidates[0].call_pass_eligible_reason(),
            Some(CallDecisionReason::EligibleTwoShantenSpeed)
        );
        assert_eq!(
            candidates[0].post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Fold,
                reason: PushPullReason::IishantenAgainstHighOpenHand,
            })
        );
        assert_eq!(select_eligible_candidate(&candidates), None);

        let three_shanten = threatened_two_shanten_reaction_context(
            &THREE_SHANTEN_CALL_PON_HAND,
            THREE_SHANTEN_CALL_PON_TARGET,
            THREE_SHANTEN_SPEED_REMAINING,
        );
        let action = pon_action(
            THREE_SHANTEN_CALL_PON_TARGET,
            &THREE_SHANTEN_CALL_PON_CONSUMED,
        );
        let (mut candidates, post_call_states) = evaluated_candidates(&three_shanten, &action);
        let comparison = candidates[0]
            .three_shanten_self_tsumo
            .as_mut()
            .expect("3向聴 Call / Pass 比較対象");
        comparison.speed = satisfied_three_shanten_speed_facts();
        let call_value = comparison.call_expected_self_tsumo_value;
        apply_three_shanten_self_tsumo_policy(
            &three_shanten,
            &mut candidates,
            Some(pass(
                PassSelfTsumoContinuationKind::ThreeShanten,
                call_value,
            )),
            &mut CallDecisionTimer::disabled(),
        );
        assert_eq!(
            candidates[0].reason,
            CallDecisionReason::EligibleThreeShantenSpeed
        );
        apply_post_call_push_pull_gate(&three_shanten, &mut candidates, &post_call_states);
        assert_eq!(candidates[0].reason, CallDecisionReason::PostCallNotPush);
        assert_eq!(
            candidates[0].call_pass_eligible_reason(),
            Some(CallDecisionReason::EligibleThreeShantenSpeed)
        );
        assert_eq!(
            candidates[0].post_call_push_pull,
            Some(PushPullDecision {
                mode: PushPullMode::Fold,
                reason: PushPullReason::TwoOrMoreShantenAgainstHighOpenHand,
            })
        );
        assert_eq!(select_eligible_candidate(&candidates), None);
    }

    #[test]
    fn the_post_call_push_pull_gate_reuses_the_selected_iishanten_forward_metrics() {
        // gate は鳴き後の打牌選択が求めた前方集計値を転記する。Push/Fold 用の値は configured
        // horizon が UNTIL_RYUKYOKU なら選択の値をそのまま使い、1向聴の前方評価も terminal
        // scoring もやり直さない。それ以外の horizon では選んだ1打牌だけを UNTIL_RYUKYOKU で
        // 評価し直す。
        for horizon in [
            SelfTsumoHorizon::UNTIL_RYUKYOKU,
            SelfTsumoHorizon::PRODUCTION,
        ] {
            let iishanten =
                threatened_reaction_context(&IISHANTEN_PON_HAND, vec![], IISHANTEN_PON_TARGET, 12)
                    .with_self_tsumo_horizon(horizon);
            let iishanten_action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);
            let two_shanten = threatened_low_value_two_shanten_reaction_context(
                &LOW_VALUE_TWO_SHANTEN_CALL_PON_HAND,
                TWO_SHANTEN_CALL_PON_TARGET,
                32,
            )
            .with_self_tsumo_horizon(horizon);
            let two_shanten_action =
                pon_action(TWO_SHANTEN_CALL_PON_TARGET, &TWO_SHANTEN_CALL_PON_CONSUMED);

            for (ctx, action, kind) in [
                (
                    &iishanten,
                    &iishanten_action,
                    PassSelfTsumoContinuationKind::Iishanten,
                ),
                (
                    &two_shanten,
                    &two_shanten_action,
                    PassSelfTsumoContinuationKind::TwoShanten,
                ),
            ] {
                let (mut candidates, post_call_states) = evaluated_candidates(ctx, action);
                let mut timing = CallDecisionTimer::disabled();
                apply_iishanten_self_tsumo_policy(
                    ctx,
                    &mut candidates,
                    (kind == PassSelfTsumoContinuationKind::Iishanten).then(|| pass(kind, Some(0))),
                    &mut timing,
                );
                apply_two_shanten_self_tsumo_policy(
                    ctx,
                    &mut candidates,
                    (kind == PassSelfTsumoContinuationKind::TwoShanten)
                        .then(|| pass(kind, Some(0))),
                    &mut timing,
                );
                assert!(candidates[0].eligible, "{:?}", candidates[0].reason);

                let PostCallState::Evaluated {
                    iishanten_forward_metrics,
                    ..
                } = &post_call_states[0]
                else {
                    panic!("高コストな評価を行った候補");
                };
                let selected_metrics = iishanten_forward_metrics.expect("選択が求めた前方集計値");
                assert_eq!(
                    selected_metrics.expected_self_tsumo_value,
                    candidates[0]
                        .iishanten_self_tsumo
                        .map(|diagnostic| diagnostic.call_expected_self_tsumo_value)
                        .or(candidates[0]
                            .two_shanten_self_tsumo
                            .map(|diagnostic| diagnostic.call_expected_self_tsumo_value))
                        .expect("Call / Pass 比較対象")
                );

                let ((), hits, misses) = tenpai_value_memo_counter::count_during(|| {
                    apply_post_call_push_pull_gate(ctx, &mut candidates, &post_call_states)
                });
                if horizon == SelfTsumoHorizon::UNTIL_RYUKYOKU {
                    assert_eq!((hits, misses), (0, 0));
                } else {
                    assert!(misses > 0);
                }
                assert_eq!(
                    candidates[0].post_call_push_pull,
                    Some(PushPullDecision {
                        mode: PushPullMode::Fold,
                        reason: PushPullReason::IishantenAgainstHighOpenHand,
                    })
                );
            }
        }
        let iishanten =
            threatened_reaction_context(&IISHANTEN_PON_HAND, vec![], IISHANTEN_PON_TARGET, 12);
        let iishanten_action = pon_action(IISHANTEN_PON_TARGET, &IISHANTEN_PON_CONSUMED);

        // 選択の計算済み値を持たない入口は、同じ鳴き後 state の前方評価をやり直す。gate が
        // この入口を通らないことの対照。
        let (candidates, post_call_states) = evaluated_candidates(&iishanten, &iishanten_action);
        let PostCallState::Evaluated {
            preparation: CallCandidatePreparation::PostCall(inputs),
            ..
        } = &post_call_states[0]
        else {
            panic!("1向聴からの鳴き");
        };
        let (_, _, misses) = tenpai_value_memo_counter::count_during(|| {
            crate::push_pull::push_pull_inputs_from_context_with_evaluation(
                &inputs.post_call_context,
                candidates[0].post_call_discard.as_ref(),
                &inputs.legal_actions,
            )
        });
        assert!(misses > 0);
    }
}
