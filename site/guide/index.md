---
layout: false
title: blugit でチーム開発
description: Bluetooth でGitのリポジトリを共有する blugit の講義スライド。
---

<SlideDeck title="blugit でチーム開発">

<Slide cover>

<p class="kicker">第2部</p>

# blugit で<br>チーム開発

<p class="lead">Wi-Fiでつながらなくても、<br>Bluetooth でリポジトリを共有しよう</p>

</Slide>

<Slide>

## この講座のゴール

- <Role who="host" />として、リポジトリの共有を始められる
- <Role who="guest" />として登録して、clone できる
- 困ったときに、どこを見ればよいか分かる

</Slide>

<Slide section id="why">

## はじめに：なぜ blugit？

</Slide>

<Slide>

## たとえば、こんな場面

<div class="cols cols--figure cols--figure-wide">
<div>

- 社員のPCは、社内のGitHubを使える
- ゲストのPCは、社内のGitHubを使えない
- 同じWi-Fiにいるのに、**PC同士は直接つながらない**

</div>
<div>

<WifiScenario variant="before" />

</div>
</div>

</Slide>

<Slide>

## blugit を使うと

<div class="cols cols--figure cols--figure-wide">
<div>

- 2台のPCを **Bluetooth で直接**つなぐ
- あとは、いつもの clone・push・pull
- アカウントもサーバーの設定もいらない

</div>
<div>

<WifiScenario variant="after" />

</div>
</div>

</Slide>

<Slide>

## 向いている場面・向かない場面

| 向いている | 向かない |
|---|---|
| 同じ部屋にいる数人での共有 | 離れた場所にいる相手との共有 |
| コード中心の、小さめのリポジトリ | 画像や動画など、大きなファイルが多いリポジトリ |
| ネットワークがない・使えない場所 | 機密情報を扱う業務 |

Bluetooth は Wi-Fi よりゆっくり。最初の clone は 6 MiB で約50秒、その後の push・pull は1秒ほどです。

</Slide>

<Slide section id="how">

## <span class="chapter-no">1</span>しくみ：ホストとゲスト

</Slide>

<Slide>

## ホストとゲスト

| 役割 | どんなPC | 使うコマンド |
|---|---|---|
| <Role who="host" /> | リポジトリを公開するPC | `blugit host` で共有を始める |
| <Role who="guest" /> | ホストに接続するPC | `blugit join` で登録して、`git clone` |

どちらがホストでもかまいません。ホストは、共有している間PCを起動したままにします。

</Slide>

<Slide>

## ゲストから見ると

<GitMap
  highlight="remote push pull"
  local-label="ゲストのPC"
  remote-label="ホストのPC"
  remote-caption="Bluetooth の先"
  caption=""
/>

Git講座の地図のリモートが、**Bluetooth の先にあるホストのPC**になるだけです。

</Slide>

<Slide>

## 住所・合言葉・合鍵

| たとえ | 実際のもの | ポイント |
|---|---|---|
| 住所 | 接続コード（`00MX-8433-A77D`） | 秘密ではない。知られても登録はできない |
| その場限りの合言葉 | 6桁の確認番号 | 1回限り・180秒で無効。ホストの人から直接聞く |
| 合鍵 | 登録のときに保存される鍵 | 2回目からは自動で確かめ合う。取り消せる |

</Slide>

<Slide>

## 全体の流れ

<div class="steps">
<div><b>blugit host</b><small>ホストが共有を始める</small></div>
<div class="arrow">→</div>
<div><b>blugit join</b><small>ゲストが登録（初回だけ）</small></div>
<div class="arrow">→</div>
<div><b>git clone</b><small>あとはいつものGit</small></div>
</div>

<p class="center">登録は最初の1回だけ。2回目からは、接続コードも確認番号もいりません。</p>

</Slide>

<Slide section id="first-share">

## <span class="chapter-no">2</span>はじめての共有

</Slide>

<Slide>

## 始める前に

- 両方のPCに、Git と blugit をインストールしておく（[インストール](/guide/install)）
- 両方のPCで、Bluetooth をオンにする
- OS から Bluetooth の利用許可を求められたら、許可する
- ターミナルが初めてなら：[ターミナルの基本](/guide/terminal)

ここからは、ホストとゲストの2人で画面を見せ合いながら進めます。

</Slide>

<Slide id="host">

## ホスト①：共有するリポジトリを作る

<AnnotatedCommand
  who="host"
  command="git init {--bare} --initial-branch=main {demo.git}"
  :notes="[
    ['--bare', '作業ファイルを持たない、共有専用のリポジトリとして作ります'],
    ['リポジトリの名前', '作られるフォルダの名前です。末尾を .git にするのが慣例です'],
  ]"
/>

既存のプロジェクトを共有するなら：`git clone --bare ./project demo.git`

</Slide>

<Slide>

## ホスト②：共有を始める

<AnnotatedCommand
  who="host"
  command="blugit host {./demo.git}"
  :notes="[
    ['共有するbareリポジトリ', '①で作ったフォルダの場所です。同時に共有できるのは1つだけです'],
  ]"
/>

ホストは、この後ずっと起動したままにしておきます。止めるときは Ctrl+C です。

</Slide>

<Slide>

## ホストの画面

<TerminalScreen
  who="host"
  command="blugit host ./demo.git"
  :lines="[
    'Sharing C:\\work\\demo.git',
    ['  Device:          HOST-PC [3f9a1c2b7d4e]', 'このPCの名前。ゲストの画面にも表示されます'],
    ['  Channel:         7 (automatic)', '接続口の番号。普段は気にしなくてよい'],
    ['  Connection code: 00MX-8433-A77D', '接続コード。これをゲストに伝えます'],
    '',
    'On the guest, run (you can change the local name):',
    ['  blugit join 00MX-8433-A77D demo', 'ゲストが打つコマンド。そのまま送ってもよい'],
  ]"
/>

</Slide>

<Slide id="code">

## 接続コードを伝える

<Handoff from="host" to="guest">接続コードを伝える（見せる・送る・読み上げる）</Handoff>

<ConnectionCodeAnatomy />

</Slide>

<Slide id="join">

## ゲスト①：登録を申請する

<AnnotatedCommand
  who="guest"
  command="blugit join {00MX-8433-A77D} {demo}"
  :notes="[
    ['接続コード', 'ホストの画面に出た12文字。大文字・小文字は区別しません'],
    ['登録名', 'このPCでの呼び名。自分で決めてよく、blugit://demo として使います'],
  ]"
/>

</Slide>

<Slide>

## 確認番号は、ホストの画面に出る

<div class="screens">

<TerminalScreen
  who="guest"
  :lines="[
    'Registration requested from &quot;HOST-PC&quot;.',
    '',
    'Ask the host operator for the code.',
    ['Six-digit code: ', '番号の入力を待っています'],
  ]"
/>

<TerminalScreen
  who="host"
  :lines="[
    ['! Registration requested from &quot;GUEST-PC&quot;', '申請してきたPCの名前'],
    ['  Six-digit code: 482915', '確認番号'],
    '  One attempt. Expires in 180 seconds.',
  ]"
/>

</div>

<Handoff from="host" to="guest">PCの名前を確かめてから、6桁の番号を伝える</Handoff>

</Slide>

<Slide>

## ゲストが番号を入力して、登録完了

<TerminalScreen
  who="guest"
  :lines="[
    { text: 'Six-digit code: ', input: '482915', note: 'ホストから聞いた番号を入力して Enter' },
    '',
    ['✓ Registered with HOST-PC [3f9a1c2b7d4e] as demo.', 'この表示が出たら登録完了'],
    '',
    'To clone the repository:',
    ['  git clone blugit://demo', '次に打つコマンド'],
  ]"
/>

::: warning 番号を伝える前に
近くの別の人が同じ接続コードで申請することもあります。表示されたPCの名前が目の前の相手のものか、対面で確かめてから伝えましょう。
:::

</Slide>

<Slide id="clone">

## ゲスト②：clone する

<AnnotatedCommand
  who="guest"
  command="git clone {blugit://}{demo} {work}"
  :notes="[
    ['blugit://', 'blugit を通して接続する、という印'],
    ['登録名', 'blugit join で決めた登録名'],
    ['作業フォルダ', '作られる作業フォルダの名前'],
  ]"
/>

作ったばかりのリポジトリでは `warning: You appear to have cloned an empty repository.` と出ますが、問題ありません。

</Slide>

<Slide>

## あとは、AIに頼むだけ

リモートが Bluetooth の先にあるだけで、commit・push・pull はいつもどおりです。

<AiChat :messages="[
  ['you', 'work フォルダで作業します。ここまでの変更をコミットして push して'],
  ['ai', '変更をコミットして、リモート（blugit://demo）に push しました'],
]" />

</Slide>

<Slide>

## ホスト自身も作業する

<AnnotatedCommand
  who="host"
  command="git clone {./demo.git} {host-work}"
  :notes="[
    ['共有中のbareリポジトリ', 'blugit host で共有しているフォルダ'],
    ['ホスト用の作業フォルダ', 'ホストはここで作業して、push・pull します'],
  ]"
/>

同じ接続先へのGitの操作は、同時に実行せず1つずつ行ってください。

</Slide>

<Slide section id="daily">

## <span class="chapter-no">3</span>毎日の使い方

</Slide>

<Slide>

## 共有を止める

ホストのターミナルで **Ctrl+C** を押します。

- **1回**：新しい接続を受け付けなくなり、実行中の転送が終わってから止まる
- **2回**：転送の途中でも、すぐに止まる

<div>

<TerminalScreen
  who="host"
  :lines="[
    ['Sharing stopped; waiting for 1 connection(s). Press Ctrl+C again to abort.', '転送が終わるのを待っています'],
  ]"
/>

</div>

</Slide>

<Slide id="restart">

## 共有を再開する

- 止めたときと同じ `blugit host ./demo.git` を実行する
- **接続コードが変わることがある**けれど、登録済みのゲストは登録し直さなくてよい
- 変わるのは11文字目の「接続口の番号」だけ。ゲストは住所（最初の10文字）を覚えているので、自動で探し直す
- 再開直後の最初の接続は、探し直すぶん時間がかかることがある

</Slide>

<Slide>

## ゲストを増やす・別のリポジトリを共有する

- 新しいゲストや別のPCは、それぞれのPCで `blugit join` する（今の接続コードを使う）
- 同じ登録名にしておけば、どのPCでも `blugit://demo` で使える
- ホストが同時に共有できるリポジトリは1つだけ
- 別のリポジトリには、**別の登録名**で `blugit join` する（例：`demo2`）

</Slide>

<Slide id="devices">

## 登録した端末を管理する

<AnnotatedCommand
  who="host"
  command="blugit peers {./demo.git}"
  :notes="[
    ['bareリポジトリ', '登録済みの端末（PC）のIDと名前を一覧します'],
  ]"
/>

<AnnotatedCommand
  who="host"
  command="blugit revoke ./demo.git {8c21e0f4}"
  :notes="[
    ['端末ID', '一覧に出たIDの先頭8桁以上。その端末からの新しい接続を断ります'],
  ]"
/>

</Slide>

<Slide section id="summary">

## まとめ

</Slide>

<Slide>

## コマンドのまとめ

| コマンド | どこで | 内容 |
|---|---|---|
| `blugit host <bareリポジトリ>` | <Role who="host" /> | 共有を始める。Ctrl+C で止める |
| `blugit join <接続コード> <登録名>` | <Role who="guest" /> | ホストに登録する（初回だけ） |
| `git clone blugit://<登録名>` | <Role who="guest" /> | 登録したホストから clone する |
| `blugit peers <bareリポジトリ>` | <Role who="host" /> | 登録した端末を一覧する |
| `blugit revoke <bareリポジトリ> <端末ID>` | <Role who="host" /> | 端末のアクセスを取り消す |

</Slide>

<Slide id="help">

## 困ったときは

- エラーメッセージの意味と対処：[困ったとき](/guide/troubleshooting)
- コマンドの細かいオプション：[コマンドリファレンス](/guide/commands)
- インストールのコマンド：[インストール](/guide/install)
- 中で何が起きているか：[しくみ（読み物）](/guide/internals)

</Slide>

</SlideDeck>
