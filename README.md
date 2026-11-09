# blugit

近くのPC同士で、Bluetoothを使ってGitリポジトリを共有するCLIです。
LANやインターネットがない場所でも、 `git clone` / `git push` / `git pull` でチーム開発ができます。

## しくみ

リポジトリを公開するPCを**ホスト**、接続するPCを**ゲスト**と呼びます。

1. ホストが共有を始めると、12文字の接続コードが表示されます。
2. ゲストは接続コードで登録を申請し、ホストの画面に出た6桁の確認番号を入力します。登録は初回だけです。
3. 以後は `git clone blugit://登録名` のように、普段のGitコマンドで操作できます。

登録後の通信は、互いに保存した公開鍵によるSSHで認証・暗号化されます。
SSHはアプリに組み込まれているため、アカウント作成やOSのSSHサーバー設定は不要です。

## 動作環境

- Windows（x64）、macOS（Intel / Apple Silicon）、Linux（x64、glibc 2.35以降）
- Bluetooth Classic（RFCOMM）対応アダプター（BLE専用アダプターは使えません）
- Git

Linuxは実機での動作確認がまだ済んでいません。動かなかったらすみません。

## インストール

インストールにはインターネット接続が必要です。ホストとゲストの両方のPCで実行してください。

Windows（PowerShell）:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/mujuko/blugit/releases/latest/download/blugit-installer.ps1 | iex"
```

macOS・Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/mujuko/blugit/releases/latest/download/blugit-installer.sh | sh
```

Linuxでは、実行に必要なBlueZとD-Busのライブラリも入れてください（Debian / Ubuntuの例）。

```sh
sudo apt-get update
sudo apt-get install -y bluez libdbus-1-3
```

ターミナルを開き直し、次の2つで同じバージョンが表示されればインストール完了です。
`git-remote-blugit` は、Gitが `blugit://` に接続するときに自動で呼び出すプログラムです。
更新するときも、同じインストールコマンドを実行してください。

```sh
blugit --version
git-remote-blugit --version
```

### ソースからビルドする

Rust stableと、OSごとのビルド環境を用意してください。

| OS | ビルド環境 |
|---|---|
| Windows | RustのMSVCツールチェーン、Visual Studio C++ Build Tools |
| macOS | Xcode Command Line Tools |
| Linux | BlueZ、`pkg-config`、`libdbus-1-dev`（Debian / Ubuntuの場合） |

このリポジトリのディレクトリで実行します。
コマンドが見つからない場合は、ホームディレクトリの `.cargo/bin` をPATHに追加してください。

```sh
cargo install --path . --locked --force
```

## 使い方

両PCのBluetoothを有効にしてから始めてください。
OSからBluetoothの利用許可を求められたら、許可してください。

### 1. ホストで共有を始める

共有には、作業ファイルを持たない**bareリポジトリ**を使います。同時に共有できるのは1つです。

```sh
git init --bare --initial-branch=main demo.git
blugit host ./demo.git
```

既存のプロジェクトを共有するなら、`git init` の代わりに `git clone --bare ./project demo.git` で作ってください。
元のリポジトリとは別のコピーになるため、以後の変更はpush／pullで受け渡します。

表示された接続コード（例：`00MX-8433-A77D`）をゲストに伝え、ホストは起動したままにします。

### 2. ゲストを登録する（初回のみ）

ホストから聞いた接続コードと、このPCで使う登録名を指定します。

```sh
blugit join 00MX-8433-A77D demo
```

ホストの画面に `JOIN ... CODE=123456` のように、申請した端末名と6桁の確認番号が表示されます。
ホスト側は端末名が相手のものか確かめてから、番号を伝えてください。対面での確認をおすすめします。
ゲスト側が `Six-digit code:` に番号を入力し、`Registered with ...` と表示されたら完了です。

登録名（ここでは `demo`）は、`blugit://demo` として使う接続先の名前です。
英数字・`-`・`_` で32文字まで、先頭は英数字にしてください。

### 3. Gitで操作する

登録名を使って、普段どおりGitを使えます。コードや番号の再入力は不要です。

```sh
git clone blugit://demo work
cd work
git push -u origin main   # commitした変更を送る
git pull --ff-only        # 他の人の変更を取り込む
```

ホスト自身も編集するなら、`git clone ./demo.git host-work` で作業コピーを作ってください。
同じ接続先へのGit操作は、同時に実行せず1つずつ行ってください。

## 共有を止める・再開する

ホストでCtrl+Cを押すと、実行中の転送が終わるのを待ってから停止します。すぐ止めるにはもう一度Ctrl+Cを押します。
再開するには、同じ `blugit host ./demo.git` を実行します。
接続コードが変わることがありますが、登録済みのゲストは再登録なしでそのまま使えます。

## 端末を管理する

登録申請のときに相手へ表示される端末名は、PC名です。
分かりやすい名前にするなら、登録前に `blugit device --name MacBook` のように変更してください。

ホストの別のターミナルで、登録済みの端末を一覧し、アクセスを取り消せます。
`DEVICE-ID` には、一覧に出る64桁のID、または先頭8桁以上を指定します。

```sh
blugit peers ./demo.git
blugit revoke ./demo.git DEVICE-ID
```

取り消した端末からの新しい接続は拒否されます。ただし、相手が取得済みの履歴は残ります。
`blugit device` で表示される保存先には秘密鍵が含まれるため、他人に渡したりGitに追加したりしないでください。

## コマンド一覧

| コマンド | 実行するPC | 内容 |
|---|---|---|
| `blugit host <bareリポジトリ> [--channel <番号>]` | ホスト | リポジトリの共有を始める。Ctrl+Cで停止 |
| `blugit join <接続コード> <登録名>` | ゲスト | ホストに端末を登録し、登録名で接続先を保存する |
| `blugit peers <bareリポジトリ>` | ホスト | 登録済みの端末を一覧する |
| `blugit revoke <bareリポジトリ> <端末ID>` | ホスト | 端末のアクセス許可を取り消す |
| `blugit device [--name <端末名>]` | 両方 | この端末の名前・ID・保存先を表示する。`--name` で名前を変更 |
| `git clone blugit://<登録名>` | ゲスト | 登録したホストからcloneする。以後のpush／pullも同じ接続先を使う |

各コマンドの詳しい使い方は、`blugit <コマンド> --help` で確認できます。
鍵と登録情報の保存先は、環境変数 `BLUGIT_HOME` で変更できます。

## 困ったとき

### 接続できない

ホストで `blugit host` が起動中か、両PCのBluetoothが有効かを確認してください。
macOSでは、初回や更新後にBluetoothの利用許可を求められることがあります。

### 確認番号を間違えた・期限が切れた

番号は1回限りで、180秒で失効します。同じ `blugit join` を実行すれば再申請できます。
新しい番号の発行には、前回から10秒以上の間隔が必要です。

### 別のPCからも使いたい

PCごとに `blugit join` で登録してください。同じ登録名にすれば、同じ `blugit://demo` を使えます。

### ホストが別のリポジトリを共有し始めた

登録名は登録時のリポジトリと結び付いています。新しいリポジトリには、別の登録名で `join` してください。

### チャネルを指定したい

チャネル（Bluetooth上の接続口の番号）は自動で選ばれます。
`blugit host ./demo.git --channel 8` のように1〜30で固定することもでき、使用中ならエラーで終了します。

## 開発に参加する方へ

- [設計](docs/architecture.md)：認証方式、処理の流れ、モジュール構成
- [チャネル自動解決](docs/channel-resolution.md)：ホスト再起動後の再接続のしくみ
- [検証記録](docs/validation.md)：自動テストと実機での確認範囲
- [リリース手順](docs/releasing.md)：配布設定と公開手順
- [依存関係とセキュリティ検査](docs/security-maintenance.md)：検査・更新・ライセンス一覧の再生成

自動テストは `cargo test --locked --features test-transport` で実行します。
`test-transport` はBluetoothをIP接続で代替するテスト専用機能のため、通常のビルドでは有効にしないでください。

## セキュリティ

機密情報・高い保証を必要とする業務用途は対象外です。
脆弱性の報告方法と保護の範囲は[セキュリティ方針](SECURITY.md)を参照してください。

## ライセンス

[Apache License 2.0](LICENSE)で公開しています。
依存ライブラリのライセンス・著作権表示は[第三者ライセンス一覧](THIRD_PARTY_LICENSES.html)に記載しています。
