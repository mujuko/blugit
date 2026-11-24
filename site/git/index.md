---
layout: false
title: AIと使う はじめてのGit
description: Gitの操作はAIに任せる人のための、Gitの考え方の講義スライド。
---

<SlideDeck title="AIと使う はじめてのGit">

<Slide cover>

<p class="kicker">第1部</p>

# AIと使う<br>はじめてのGit

<p class="lead">コマンドは覚えなくていい。<br>AIに頼むための「言葉」を知ろう</p>

</Slide>

<Slide>

## この講座のゴール

- AIに「コミットして」「push して」と**頼める**
- AIの報告に出てくる言葉が**分かる**
- Gitのコマンドは**覚えない**（操作はAIに任せる）

</Slide>

<Slide section id="intro">

## はじめに：Gitってなに？

</Slide>

<Slide>

## こんなフォルダ、見覚えありませんか？

<div class="cols">
<div>

- どれが最新？
- 「修正2」で何を変えた？
- 2人で別々に直したら、どうまとめる？

</div>
<div>

```text
レポート.docx
レポート_修正.docx
レポート_修正2.docx
レポート_最終版.docx
レポート_最終版_本当に最終.docx
```

</div>
</div>

</Slide>

<Slide>

## Gitは「変更の記録係」

<div class="cols cols--figure">
<div>

- ファイルの変更を記録して、**履歴**として残す
- 「いつ・誰が・何を」変えたかが分かる
- いつでも過去の状態に戻せる
- 何人で作業しても、変更を1つにまとめられる

</div>
<div>

<HistoryTree />

</div>
</div>

</Slide>

<Slide>

## AIに任せるのに、なぜ知る必要があるの？

操作はAIがやってくれます。でも、**何をしてほしいか決めるのはあなた**です。

<AiChat :messages="[
  ['you', 'いい感じに保存しておいて'],
  ['ai', 'コミットだけしますか？ リモートに push もしますか？ main に直接入れてよいですか？'],
]" />

</Slide>

<Slide id="map">

## Gitの地図

<GitMap caption="" />

</Slide>

<Slide section id="record">

## <span class="chapter-no">1</span>記録する：add と commit

<GitMap highlight="files record commit" caption="" />

</Slide>

<Slide>

## 編集しただけでは、記録されない

ファイルを編集しても、変わるのは「作業中のファイル」だけ。<br>記録に残すには、**選ぶ（add）** と **記録する（commit）** の2つの手順がいります。

<GitMap highlight="files" caption="" />

</Slide>

<Slide>

## add：記録するものを選ぶ

<div class="cols">
<div>

- 変更したファイルの中から、今回の記録に入れるものを選ぶ
- 関係ない変更を混ぜないため
- ふだんはAIが選んでくれる。「〇〇の変更だけコミットして」と頼むこともできる

</div>
<div class="pick" role="img" aria-label="変更した3つのファイルのうち、login.html と style.css を選び、memo.txt は選ばない">
<div class="pick__title">変更したファイル</div>
<div class="pick__row is-on"><span class="pick__box">✓</span>login.html</div>
<div class="pick__row is-on"><span class="pick__box">✓</span>style.css</div>
<div class="pick__row"><span class="pick__box"></span>memo.txt<small>← 今回の作業とは関係ない</small></div>
<div class="pick__next">→ 選んだ2つを commit で記録</div>
</div>
</div>

</Slide>

<Slide>

## commit：記録する

- 選んだ変更に説明（メッセージ）を付けて、1つの記録にする
- この記録を**コミット**と呼ぶ。ゲームのセーブのようなもの
- コミットが積み重なって、**履歴**になる

<CommitGraph :scale="2" axis :commits="[{ id: 'A' }, { id: 'B' }, { id: 'C' }]" caption="丸1つが、1回のコミット" />

</Slide>

<Slide>

## AIへの頼み方：記録する

ひとまとまりの作業が終わるたびに、コミットを頼むのがコツです。

<AiChat :messages="[
  ['you', 'ログイン画面の修正ができたので、コミットして'],
  ['ai', 'login.html と style.css の変更を add して、「ログイン画面のボタン位置を修正」というメッセージでコミットしました'],
]" />

</Slide>

<Slide section id="share">

## <span class="chapter-no">2</span>共有する：ローカルとリモート

<GitMap highlight="record remote push pull" caption="" />

</Slide>

<Slide>

## ローカルとリモート

<div class="cols">
<div class="card card--a">

### ローカル＝自分のPC

- 作業中のファイルと、その記録がある
- **自分専用**。ほかの人からは見えない
- コミットした記録は、まずここに入る

</div>
<div class="card card--b">

### リモート＝みんなの置き場

- みんながアクセスできる場所にある記録
- **みんなと共有**する
- GitHub や、blugit ならホストのPC

</div>
</div>

</Slide>

<Slide>

## push で送って、pull で取り込む

<ShareFigure />

</Slide>

<Slide>

## コミットしただけでは、まだ届いていない

<div class="steps">
<div><b>add</b><small>選ぶ</small></div>
<div class="arrow">→</div>
<div><b>commit</b><small>記録する</small></div>
<div class="arrow">→</div>
<div><b>push</b><small>送る</small></div>
</div>

<p class="center">コミットした記録は、まだ<strong>自分のPCの中だけ</strong>。<br>みんなに届けるには、<strong>push</strong> まで頼みましょう。</p>

</Slide>

<Slide>

## AIへの頼み方：共有する

<AiChat :messages="[
  ['you', '作業を始めるから、最新を pull して'],
  ['ai', 'リモートから、新しいコミットを3件取り込みました'],
  ['you', '今日の分をコミットして、push して'],
  ['ai', 'コミットして、リモートに push しました。ほかの人も pull すれば受け取れます'],
]" />

</Slide>

<Slide section id="branch">

## <span class="chapter-no">3</span>枝分かれと合流：ブランチとマージ

</Slide>

<Slide>

## main が「本物」

- **main** は、いつでもちゃんと動く、完成品の履歴
- 作りかけの変更を、main に直接入れない

<CommitGraph :scale="2" axis trunk :commits="[{ id: 'A' }, { id: 'B' }, { id: 'C' }]" :branches="{ main: 'C' }" />

</Slide>

<Slide>

## 枝分かれして開発する：ブランチ

- 新しい機能や修正は、main から枝分かれした**ブランチ**で作る
- ブランチでいくら試しても、main はそのまま。失敗しても安心

<CommitGraph
  :scale="1.4"
  axis
  trunk
  :commits="[
    { id: 'A' }, { id: 'B' },
    { id: 'C', lane: 1 }, { id: 'E', lane: 2, parents: ['B'] },
    { id: 'D', lane: 1 }, { id: 'F', lane: 2 },
  ]"
  :branches="{ main: 'B', 機能A: 'D', 機能B: 'F' }"
/>

</Slide>

<Slide>

## できたら合流する：マージ

- 完成したら、ブランチを main に取り込む。これが**マージ**
- 図では、機能Aが完成して main に合流。機能Bはまだ作業中

<CommitGraph
  :scale="1.4"
  axis
  trunk
  :commits="[
    { id: 'A' }, { id: 'B' },
    { id: 'C', lane: 1 }, { id: 'E', lane: 2, parents: ['B'] },
    { id: 'D', lane: 1 }, { id: 'M', parents: ['B', 'D'] },
    { id: 'F', lane: 2 },
  ]"
  :branches="{ main: 'M', 機能A: 'D', 機能B: 'F' }"
/>

</Slide>

<Slide>

## AIへの頼み方：ブランチとマージ

<AiChat :messages="[
  ['you', '機能Aを作りたい。新しいブランチで作業して'],
  ['ai', '機能A用のブランチを作って作業します。main はそのままです'],
  ['you', 'できたので main にマージして'],
  ['ai', '機能Aのブランチを main にマージしました'],
]" />

</Slide>

<Slide section id="summary">

## まとめ：AIへの頼み方

</Slide>

<Slide>

## 頼み方の早見表

| やりたいこと | AIへの頼み方 | Gitの言葉 |
|---|---|---|
| 作業を記録したい | 「ここまでの変更をコミットして」 | add・commit |
| みんなに届けたい | 「push して」 | push |
| みんなの変更を受け取りたい | 「pull して最新にして」 | pull |
| 新しいことを試したい | 「新しいブランチで作業して」 | ブランチ |
| 完成したので本物に入れたい | 「main にマージして」 | マージ |

</Slide>

<Slide>

## ふだんの流れ

<div class="steps">
<div><b>pull</b><small>最新にする</small></div>
<div class="arrow">→</div>
<div><b>ブランチ</b><small>枝分かれ</small></div>
<div class="arrow">→</div>
<div><b>commit</b><small>記録する</small></div>
<div class="arrow">→</div>
<div><b>マージ</b><small>main に合流</small></div>
<div class="arrow">→</div>
<div><b>push</b><small>みんなに送る</small></div>
</div>

<p class="center">どの手順も、AIに一言頼めば大丈夫です。</p>

</Slide>

<Slide>

## AIの報告でよく出る言葉

| 言葉 | 意味 | どうする？ |
|---|---|---|
| コンフリクト（衝突） | 2人が同じ場所を別々に変えていて、自動ではまとめられない | 「解決して」と頼む。どちらを残すか聞かれたら答える |
| push が拒否された | 先に誰かが push していた | 「pull してから push し直して」と頼む |
| まだ push していません | 記録は自分のPCの中だけ | 共有したいなら「push して」 |

</Slide>

</SlideDeck>
