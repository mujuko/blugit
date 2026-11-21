# 困ったとき

## 症状から探す

### blugit や git-remote-blugit が見つからない

- ターミナルを一度閉じて、開き直してください。インストール直後は、開き直すまでコマンドが見つからないことがあります。
- それでも見つからなければ、[インストール](./install)のコマンドをもう一度実行してください。

### 接続できない・時間がかかる

- <Role who="host" />で `blugit host` が起動しているか確かめてください。
- 両方のPCで Bluetooth がオンになっているか確かめてください。
- 2台のPCを近づけてみてください。
- macOS では、初回や更新後に Bluetooth の利用許可を求められることがあります。許可しないと接続できません。
- ホストが共有を再開した直後は、ゲストが接続口を探し直すため、最初の接続に時間がかかることがあります（[共有を再開する](./daily#共有を再開する)）。

### 確認番号を間違えた・期限が切れた

確認番号は、**1回限り**で、**180秒**で無効になります。
間違えたり時間が過ぎたりしたときは、同じ `blugit join` をもう一度実行すれば、新しい番号で申請し直せます。
新しい番号を出してもらうには、前回の申請から10秒以上あけてください。

### clone や push を断られる

<Role who="guest" />で `SSH public key authentication refused` と表示されたときは、次のどれかです。

- ホストが、別のリポジトリを共有している
- ホストが、この端末のアクセスを取り消した
- この端末が、そのリポジトリに登録されていない

ホストが正しいリポジトリを共有しているか確かめて、それでもだめなら、もう一度 `blugit join` で登録してください。

### 登録名が見つからない

`git clone blugit://demo` で `repository not registered; run blugit join first` と表示されたときは、登録名の打ち間違いか、このPCでまだ `blugit join` していないかのどちらかです。

## エラーメッセージから探す

blugit のメッセージは英語で表示されます。表示されたメッセージに、次の文が含まれていないか探してください。

### 接続コード・登録名

| メッセージ | 意味と対処 |
|---|---|
| `connection code checksum mismatch; check with the host` | 接続コードの打ち間違いです。ホストの画面を見直して、入力し直してください |
| `connection code must be 12 letters/digits, optionally XXXX-XXXX-XXXX` | 接続コードの文字数が違います。12文字（ハイフンを入れるなら14文字）で入力してください |
| `invalid character in connection code` | 接続コードに使えない文字が入っています。全角になっていないか確かめてください |
| `name must be 1..32 ASCII letters/digits, hyphens or underscores, starting with a letter/digit` | 登録名のルール違反です。英数字・`-`・`_` で32文字まで、先頭は英数字にしてください |
| `name … is already bound to another host; choose a different name` | その登録名は、別のホストの登録ですでに使っています。別の名前にしてください |
| `name … is already bound to another repository; choose a different name` | その登録名は、別のリポジトリの登録ですでに使っています。別の名前にしてください |

### 確認番号

| メッセージ | 意味と対処 |
|---|---|
| `code verification failed; request a new code` | 確認番号が違いました。`blugit join` からやり直してください |
| `registration expired; request a new code` | 180秒を過ぎて、番号が無効になりました。`blugit join` からやり直してください |
| `host refused the request` | ホストが申請を受け付けませんでした。ほかの申請を処理中か、前回の申請から10秒たっていません。少し待ってからやり直してください |

### 共有・接続

| メッセージ | 意味と対処 |
|---|---|
| `host requires a bare repository: git init --bare <path>` | 共有できるのは bare リポジトリだけです。[はじめての共有](./first-share)の手順で作ってください |
| `another blugit process holds …` | このPCで、すでに別の `blugit host` が動いています。先にそちらを止めてください |
| `bind RFCOMM channel; omit --channel for automatic allocation` | `--channel` で指定した番号が使用中です。別の番号にするか、`--channel` を外してください |
| `Bluetooth is switched off` | Bluetooth がオフになっています（Linux）。オンにしてください |
| `connect to BlueZ; is bluetoothd running?` | Bluetooth のサービスが動いていません（Linux）。BlueZ が入っていて、起動しているか確かめてください |
| `Bluetooth connection failed; check the host address and channel` | ホストにつながりませんでした。ホストが共有中か、Bluetooth がオンか、距離が離れすぎていないか確かめてください |
| `SSH public key authentication refused` | 登録が無効です。[clone や push を断られる](#clone-や-push-を断られる)を見てください |
| `repository not registered; run blugit join first` | その登録名は、このPCに登録されていません |
| `Connection limit reached; try again later` | ホストへの同時接続が上限（16件）に達しています（ホストの画面に表示）。少し待ってからやり直してください |

ここにない問題や、直らない問題は、[GitHub の Issue](https://github.com/mujuko/blugit/issues) で知らせてください。
