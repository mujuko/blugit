# 端末とアクセスの管理

blugit では、PCのことを**端末**と呼びます。端末ごとに名前とIDがあり、ホストは端末ごとにアクセスを許可します。

## 自分の端末名を確かめる・変える

`blugit device` で、このPCの端末名・ID・保存先を確かめられます。

<TerminalScreen
  command="blugit device"
  :lines="[
    ['Device: GUEST-PC', '端末名です。最初はPCの名前になっています'],
    ['ID: 8c21e0f45a9b7d03c6e2f18a4b5c9d7e0f1a2b3c4d5e6f708192a3b4c5d6e7f8', '端末ID（64桁）です。端末を見分けるために使います'],
    ['State: C:\\Users\\taro\\AppData\\Local\\blugit\\v2', '鍵と登録情報の保存先です'],
  ]"
/>

端末名は、登録を申請したときに相手の画面に表示される名前です。分かりやすい名前に変えたいときは、**申請する前に** `--name` で変更します。

<AnnotatedCommand
  command="blugit device --name {Taro-Laptop}"
  :notes="[
    ['新しい端末名', '相手の画面に表示される名前です。誰のPCか分かる名前にしましょう'],
  ]"
/>

## 登録済みの端末を一覧する

<Role who="host" />は、共有しているリポジトリに登録されている端末を一覧できます。

<AnnotatedCommand
  who="host"
  command="blugit peers {./demo.git}"
  :notes="[
    ['bareリポジトリ', '共有している（していた）リポジトリの場所です'],
  ]"
/>

<TerminalScreen
  who="host"
  command="blugit peers ./demo.git"
  :lines="[
    ['8c21e0f45a9b7d03c6e2f18a4b5c9d7e0f1a2b3c4d5e6f708192a3b4c5d6e7f8  GUEST-PC', '端末ID と 端末名'],
    '5b0e9a7c13d2f4e6a8b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2  Hanako-Mac',
  ]"
/>

まだ誰も登録していなければ、`No registered devices.` と表示されます。

## アクセスを取り消す

登録した端末からのアクセスを止めたいときは、`blugit revoke` で取り消します。

<AnnotatedCommand
  who="host"
  command="blugit revoke {./demo.git} {8c21e0f4}"
  :notes="[
    ['bareリポジトリ', '共有しているリポジトリの場所です'],
    ['端末ID', '一覧に出た64桁の端末ID、またはその先頭8桁以上を指定します'],
  ]"
/>

<TerminalScreen
  who="host"
  command="blugit revoke ./demo.git 8c21e0f4"
  :lines="[
    ['Revoked GUEST-PC [8c21e0f45a9b]. New connections will be refused.', 'GUEST-PC のアクセスを取り消しました'],
  ]"
/>

- 取り消した端末からの新しい接続は、拒否されます。すでに始まっていた転送は、最後まで続くことがあります。
- 相手がすでに取得した履歴は、相手のPCに残ります。取り消しで消えるわけではありません。
- もう一度使えるようにするには、相手にもう一度 `blugit join` で登録してもらいます。

## 鍵と登録情報の保存先

blugit は、端末の鍵と登録情報を次の場所に保存します。

| OS | 保存先 |
|---|---|
| Windows | `C:\Users\<ユーザー名>\AppData\Local\blugit\v2` |
| macOS | `~/Library/Application Support/blugit/v2` |
| Linux | `~/.local/share/blugit/v2` |

::: danger 保存先は人に渡さない
保存先には、この端末の**秘密鍵**が入っています。人に渡したり、Gitのリポジトリに追加したりしないでください。
保存先を消すと端末の鍵も消えるので、それまでの登録はすべて使えなくなり、登録し直しが必要になります。
:::

保存先は、環境変数 `BLUGIT_HOME` で変えられます。

## チャネルを指定する

チャネルは、Bluetooth の接続口の番号です。ふだんは `blugit host` が空いている番号を自動で選ぶので、気にする必要はありません。
番号を固定したいときは、`--channel` で 1〜30 の番号を指定します。

<AnnotatedCommand
  who="host"
  command="blugit host ./demo.git {--channel 8}"
  :notes="[
    ['--channel', '使う接続口の番号を 1〜30 で指定します。その番号が使用中なら、エラーで終了します'],
  ]"
/>
