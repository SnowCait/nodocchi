# 打牌選択

通常打牌は打牌後の手牌を評価し、向聴数、Acceptance、ドラ、フリテン、先読み指標などを comparator で比較します。この文書は問題局面を調査するための主要な見方をまとめます。全 enum と tie-break の正確な順序は production code と tests を source of truth としてください。

## shanten と Acceptance

各候補は打牌後の通常手・七対子・国士無双を含む最小向聴を評価します。Acceptance は次に引くことで向聴が進む牌種と、見え牌を差し引いた残枚数です。

`bot-scenario` の `Normal discard candidates` では候補ごとの shanten、acceptance remaining / types、ドラやフリテンの情報を確認できます。見え牌には手牌、ツモ牌、ドラ表示牌、河、副露、`extra_visible_tiles` が反映されます。

## 1向聴: WeightedTenpaiWait

1向聴候補では、受け入れ牌を引いた後にテンパイへ進む各 branch の待ちを重み付きで集約した `weighted tenpai wait` を表示します。単なる現在の受け入れ枚数だけでなく、テンパイ後に残る待ちも比較するための指標です。

これは期待得点や和了率ではありません。対象外または計算不要の候補は `-` です。

## 2向聴以上: WeightedNextAcceptance

2向聴以上では次の指標を表示します。

```text
weighted next acceptance
= Σ(first draw remaining × next acceptance)
```

既存 Acceptance の有効牌を1枚引いた後、既存 comparator が選ぶ次打牌後の受け入れを集計した2手先の牌効率 proxy です。これも期待値や和了率ではありません。

## lookahead

`bot-scenario --lookahead` は通常打牌候補ごとの2手先概要を追加します。`--verbose` と併用すると受け入れ牌ごとの詳細も表示します。

```bash
cargo run -p bot-scenario -- \
  --hand "234m455p789s1123z" \
  --draw "N" \
  --lookahead --verbose
```

lookahead は diagnostics 用で、`act()` の通常経路には追加探索を持ち込みません。diagnostics の有無で最終 decision が変わらないことを tests で固定しています。

## selected と runner-up

候補の `selected: yes` は通常打牌 comparator の選択です。最終 action は Reach や防御 fallback によって変わる場合があります。最終結果は `Final decision` と `Summary` を確認してください。

`Summary.runner-up` は最終選択を除いた場合に次に選ばれる候補です。候補の比較理由と合わせて、どの比較軸で勝敗が決まったかを調査できます。出力形式は [Structured diagnostics](../diagnostics.md#summary-と-runner-up) を参照してください。
