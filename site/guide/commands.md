# コマンドリファレンス

## 一覧

| コマンド | 実行するPC | 内容 |
|---|---|---|
| `blugit host <bareリポジトリ> [--channel <番号>]` | <Role who="host" /> | リポジトリの共有を始める。Ctrl+Cで停止 |
| `blugit join <接続コード> <登録名>` | <Role who="guest" /> | ホストに端末を登録し、登録名で接続先を保存する |
| `blugit peers <bareリポジトリ>` | <Role who="host" /> | 登録済みの端末を一覧する |
| `blugit revoke <bareリポジトリ> <端末ID>` | <Role who="host" /> | 端末のアクセス許可を取り消す |
| `blugit device [--name <端末名>]` | 両方 | この端末の名前・ID・保存先を表示する。`--name` で名前を変更 |
| `git clone blugit://<登録名>` | <Role who="guest" /> | 登録したホストから clone する。以後の push／pull も同じ接続先を使う |

`[ ]` で囲んだ部分は、省略できます。各コマンドの詳しい使い方は、`blugit <コマンド> --help` でも確かめられます。

## blugit host

<AnnotatedCommand
  who="host"
  command="blugit host {<bareリポジトリ>} {[--channel <番号>]}"
  :notes="[
    ['bareリポジトリ', '共有する bare リポジトリの場所です。同時に共有できるのは1つだけです'],
    ['--channel（省略可）', '接続口の番号を 1〜30 で固定します。省略すると、空いている番号が自動で選ばれます'],
  ]"
/>

Ctrl+C を1回押すと、実行中の転送が終わるのを待ってから止まります。もう一度押すと、すぐに止まります。→ [毎日の使い方](./daily)

## blugit join

<AnnotatedCommand
  who="guest"
  command="blugit join {<接続コード>} {<登録名>}"
  :notes="[
    ['接続コード', 'ホストの画面に表示された12文字です。ハイフンは省略でき、大文字・小文字は区別しません'],
    ['登録名', 'このPCでの接続先の名前です。英数字・-・_ で32文字まで、先頭は英数字。git clone blugit://登録名 として使います'],
  ]"
/>

実行すると `Six-digit code:` と表示されるので、ホストから聞いた6桁の確認番号を入力します。→ [はじめての共有](./first-share)

## blugit peers

<AnnotatedCommand
  who="host"
  command="blugit peers {<bareリポジトリ>}"
  :notes="[
    ['bareリポジトリ', '登録済みの端末を見たいリポジトリの場所です'],
  ]"
/>

1行に1台ずつ、端末IDと端末名を表示します。→ [端末とアクセスの管理](./devices)

## blugit revoke

<AnnotatedCommand
  who="host"
  command="blugit revoke {<bareリポジトリ>} {<端末ID>}"
  :notes="[
    ['bareリポジトリ', 'アクセスを取り消すリポジトリの場所です'],
    ['端末ID', 'blugit peers に出る64桁の端末ID、またはその先頭8桁以上です'],
  ]"
/>

→ [端末とアクセスの管理](./devices)

## blugit device

<AnnotatedCommand
  command="blugit device {[--name <端末名>]}"
  :notes="[
    ['--name（省略可）', 'この端末の名前を変えます。登録を申請したときに、相手の画面に表示される名前です'],
  ]"
/>

`--name` を付けなければ、端末名・端末ID・保存先を表示するだけです。→ [端末とアクセスの管理](./devices)

## git clone blugit://

<AnnotatedCommand
  who="guest"
  command="git clone {blugit://}{<登録名>} {[<作業フォルダ>]}"
  :notes="[
    ['blugit://', 'blugit を通して接続する、という印です'],
    ['登録名', 'blugit join で決めた登録名です'],
    ['作業フォルダ（省略可）', '作られる作業フォルダの名前です'],
  ]"
/>

clone した後の commit・push・pull は、ふつうのGitと同じです。AI に頼んでもかまいません。→ [Git講座](../git/)

## 共通

| 指定 | 内容 |
|---|---|
| `blugit --help` | 使い方の一覧を表示する |
| `blugit --version` | バージョンを表示する |
| 環境変数 `BLUGIT_HOME` | 鍵と登録情報の保存先を変える |
