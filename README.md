# blugit prototype

Mac / Windows のホスト・ゲスト間で、Bluetooth Classic (RFCOMM) 経由の
`git clone` / `git push` / `git pull` を試すための最小プロトタイプです。
実行時に既存 LAN・Wi-Fi・インターネットは使いません。

ホストは **1 つの bare リポジトリ** を公開します。認証・チーム管理・端末の自動探索は
実装していません。接続には OS でペアリングした端末の Bluetooth アドレスを指定します。
アプリ独自の認証はありませんが、OS が求めるペアリングや Bluetooth 利用許可は必要です。

## 準備（両方の端末）

必要なものは Git、Python 3.11 以上、[uv](https://docs.astral.sh/uv/)、
Bluetooth Classic / RFCOMM 対応アダプターです。
まずオンラインのうちに、このソースと依存パッケージを両端末へ用意します。

```sh
git clone --branch prototype https://github.com/mujuko/blugit.git
cd blugit
uv sync --locked
```

このブランチがまだリモートにない場合は、ローカルのソース一式をコピーしてください。
オフラインにする前に `uv sync --locked` を済ませておきます。

続いて、同じターミナルの PATH に CLI と Git Remote Helper を追加します。

Windows PowerShell:

```powershell
$env:PATH = "$PWD\.venv\Scripts;$env:PATH"
```

macOS:

```sh
export PATH="$PWD/.venv/bin:$PATH"
```

`blugit --help` が表示できることを確認し、OS の Bluetooth 設定で 2 台をペアリングします。
macOS で Bluetooth 利用許可が表示されたら `blugit` に許可してください。
セットアップ後はネットワークを切って動かせます。

## ホスト

好きな保存先で bare リポジトリを作成して公開します。
Mac / Windows どちらも同じコマンドです。

```sh
git init --bare --initial-branch=main demo.git
blugit host ./demo.git
```

`Serving ... (RFCOMM channel ...)` が出たら待ち受け中です。
このターミナルは開いたままにします。終了は Ctrl+C です。
ホストが指定したリポジトリだけを読み書きし、ゲストはパスや実行コマンドを指定できません。

ホスト自身も作業する場合は、別ターミナルで `git clone ./demo.git host-work` し、
その作業コピーから通常どおりローカルの bare リポジトリへ push します。

## ゲスト

ペアリング済み端末の名前、アドレス、clone URL を表示します。
これは OS が知っている端末の一覧で、blugit が動作中であることまでは確認しません。

```sh
blugit devices
```

ホストの行にある URL を使います。以下のアドレスは実際のものに置き換えます。
URL は `blugit://AA-BB-CC-DD-EE-FF` 形式で、リポジトリ名は付けません。

```sh
git -c init.defaultBranch=main clone blugit://AA-BB-CC-DD-EE-FF demo-work
cd demo-work
```

空のリポジトリでは Git の初期ブランチ設定を使うため、上のコマンドでは `main` を明示しています。
初回の空リポジトリ警告は正常です。

ファイルを作ってコミットしたら push できます。Git の名前とメールアドレスは通常どおり
`git config user.name` / `git config user.email` で設定しておきます。

```sh
git add .
git commit -m "First Bluetooth commit"
git push -u origin main
```

別の作業コピーやホストから更新を push した後は、いつものコマンドで取得します。

```sh
git pull --ff-only
git push
```

競合・non-fast-forward の扱いは通常の Git と同じです。

## 実機での確認

1. 依存関係のセットアップと Bluetooth ペアリングを済ませる。
2. 両端末の Wi-Fi と Ethernet を切り、Bluetooth は有効にしておく。
3. ホストで上記の `blugit host` を起動する。
4. ゲストで clone → ファイルをコミット → push する。
5. ホストの別ターミナルで `git --git-dir=demo.git log --oneline --all` を確認する。
6. ホストで `git clone ./demo.git host-work` し、別のファイルをコミットして push する。
7. ゲストで `git pull --ff-only` し、そのファイルが届くことを確認する。
8. ホストとゲストを入れ替え、別の bare リポジトリでも同じ操作を行う。

## 仕組みと範囲

```text
guest Git <-> git-remote-blugit <-> Bluetooth RFCOMM <-> blugit host
                                                        |
                                            git upload-pack / receive-pack
                                                        |
                                                 bare repository
```

[Git Remote Helper の `connect`](https://git.github.io/htmldocs/gitremote-helpers.html)
で Git 本来の双方向通信をつなぎ、オブジェクト転送・差分・ref 更新は Git に任せます。
Bluetooth は [aio-rfcomm 1.1.0](https://github.com/dlech/aio-rfcomm) に委譲します。
独自フレームはサービス選択、データ、stdin 終了、stderr、終了コード、受信完了確認だけです。
サービス UUID は `2d508ec7-69a4-47f2-a90c-b54df08c3cf5`。SDP でチャネルを引くので、
ホスト再起動時に RFCOMM チャネルが変わっても URL は同じです。

- 同じホストへの Git 操作は順番に実行してください。
- 認証・認可・アプリ独自の暗号化・再接続・転送再開はありません。
- BLE 専用アダプターには対応しません。大きな履歴は Bluetooth の速度に制限されます。
- 切断時は進行中の Git プロセスを終了します。再接続後に Git コマンドを再実行してください。
- Mac / Windows 対応ライブラリを使っていますが、このプロトタイプの端末間実機通信は未検証です。

## 困ったとき

- `git: 'remote-blugit' is not a git command`: ゲストの PATH に `.venv/Scripts`（Windows）
  または `.venv/bin`（Mac）が入っているか確認してください。`uv run git clone ...` でも使えます。
- `Bluetooth unavailable: NO_ADAPTER` / `SWITCHED_OFF`: OS のアダプター状態とスイッチを確認します。
- `NOT_ALLOWED`: OS の Bluetooth 利用許可を確認し、コマンドを起動し直します。
- サービスが見つからない / 接続タイムアウト: ホストが起動しているか、アドレスとペアリング、
  距離を確認します。ホスト再起動後は Bluetooth 接続やサービスキャッシュの更新が必要な場合があります。
- Mac でホストの停止・起動を繰り返した後に接続できなくなった場合は、Bluetooth をオフ・オンして
  ホストを再起動します。依存ライブラリに記載されたチャネル再利用の制約があります。
  詳細は [aio-rfcomm のプラットフォーム別制約](https://dlech.github.io/aio-rfcomm/platforms/) を参照してください。

## 開発・検証

```sh
uv sync --locked
uv run pytest -q
uv run ruff check .
uv run ruff format --check .
```

テストは実際の Git CLI・Remote Helper・ホストの Git プロセスを使用し、無線部分だけを
テスト用ループバックストリームに置き換えます。配布する CLI に TCP モードはありません。
空の clone、バイナリデータの push / clone、pull、更新不要の pull、non-fast-forward 拒否、
rebase 後の push、オブジェクト整合性、Git のエラー伝達、切断時の終了処理を確認します。

実装時の Windows 環境では Bluetooth アダプターが OS 上で `Error`、ライブラリからは
`NO_ADAPTER` だったため、無線での実機検証は行えていません。上の実機手順で確認が必要です。
