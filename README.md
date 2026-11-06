# blugit prototype — Rust

Windows / macOS / Linux の PC 間で、Bluetooth Classic (RFCOMM) を使い、
オフラインで通常の `git clone` / `git push` / `git pull` を行うプロトタイプです。
ホストは 1 つの bare リポジトリを公開します。従来の `blugit host` は認証・チーム管理なしです。

`0.3.0` では段階1の技術検証用に `blugit spike` を追加しました。
6桁の使い捨て番号による端末登録、登録済み鍵での再接続、暗号化した Git 転送、
近隣探索と広告無効を検証します。手順と検証済み／未検証の境界は
[段階1の技術検証](docs/stage1-spike.md) を参照してください。
本実装のチーム管理はまだ追加していません。段階2以降は `main` で新しく開発する予定です。

修正版 `0.3.1` は、登録完了の最終応答待ち、MacのSDPキャッシュ利用、探索の開始待ちと診断を改善します。
利用者の実機では、直接チャネルと `secure=1` を指定したMac→Windowsの登録済み鍵認証・pushが成功しました。
近隣探索とチャネルなし接続の再確認は継続中です。

`0.3.2` は探索候補への接続を単独の `probe` と共通化し、探索だけの5秒打ち切りを廃止します。
Macでは問い合わせ補助プロセスの終了を待ってから接続し、`discover --address <アドレス>` で
探索候補の1台だけを確認できます。実機のclone/push/pullと単独probeは成功、
一括探索の修正後の動作確認は継続中です。

Python 版で実機転送が成功した構成を Rust に移植しています。サービス UUID と通信フレームは
同じで、Python 版と相互接続できます。旧実装は [reference/python](reference/python) に保存しました。
この移植には、先に検討した SSH・公開鍵認証はまだ含めていません。

## 準備（両端末）

Git と Rust の stable ツールチェーンが必要です。Windows は Rust の MSVC ツールチェーンと
Visual Studio C++ Build Tools、Mac は Xcode Command Line Tools が必要です。
Bluetooth Classic / RFCOMM 対応アダプターを使います（BLE 専用アダプターは不可）。

Linux（Ubuntu / Debian）はビルド用パッケージも入れます。

```sh
sudo apt-get install build-essential pkg-config libdbus-1-dev libbluetooth-dev bluez
```

オンラインのうちにソースと依存関係を用意し、**両端末で**インストールします。

```sh
git clone --branch prototype https://github.com/mujuko/blugit.git
cd blugit
cargo install --path . --locked
blugit --help
```

`blugit` と `git-remote-blugit` の 2 つが Cargo の bin ディレクトリに入ります。
既存の Python 版を使っていた場合は、その `.venv` より先に Cargo の bin を PATH に入れてください。
PowerShell では次のように設定できます。

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Get-Command blugit,git-remote-blugit
```

Mac / Linux:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
command -v blugit git-remote-blugit
```

セットアップ後の実行に Python・uv・LAN・インターネットは不要です。
両端末の Bluetooth は有効にします。OS での事前ペアリングを接続手順の前提にはしませんが、
OS が表示する Bluetooth 利用許可や接続確認には対応してください。

## ホスト

```sh
git init --bare --initial-branch=main demo.git
blugit host ./demo.git
```

待ち受けが始まると、自分の Bluetooth アドレスとゲスト用コマンドを表示します。

```text
Serving ... (RFCOMM channel 7)
Bluetooth address: AA:BB:CC:DD:EE:FF
On the guest: git -c init.defaultBranch=main clone blugit://AA-BB-CC-DD-EE-FF demo-work
Direct channel URL (skip service lookup): blugit://AA-BB-CC-DD-EE-FF?channel=7
```

ホストはこのまま起動しておきます。終了は Ctrl+C。
アドレスだけ確認するには `blugit address` を使います。
表示用アドレスを手動指定する `blugit host ./demo.git --address AA-BB-CC-DD-EE-FF` も使えます。
このオプションはアダプターを切り替えません。

## ゲスト

ホストの `On the guest:` のコマンドをコピーして実行します。
以下のアドレスを実際のものに置き換えてください。

```sh
git -c init.defaultBranch=main clone blugit://AA-BB-CC-DD-EE-FF demo-work
cd demo-work
```

空のリポジトリの場合は警告が出ますが正常です。初期ブランチは `main` に揃えています。
Git の `user.name` / `user.email` を設定し、ファイルを作ってコミットします。

```sh
git add .
git commit -m "First Bluetooth commit"
git push -u origin main
```

以降は通常のコマンドを使えます。

```sh
git pull --ff-only
git push
```

サービス検索で接続できない場合は、ホストの **Direct channel URL** を使えます。
URL を引用符で囲み、チャネル番号も実際に表示されたものに置き換えます。

```sh
git -c init.defaultBranch=main clone 'blugit://AA-BB-CC-DD-EE-FF?channel=7' demo-work
```

直接チャネルを指定した場合、ホスト再起動で番号が変わったら、既存の作業コピーでも
`git remote set-url origin '新しいDirect channel URL'` が必要です。
チャネルなしの URL はサービス UUID で接続先チャネルを検索します。

`blugit devices` は **ペアリング済み端末の一覧**です。近くの PC を探索するコマンドではありません。
ホストが一覧に出なくても、ホスト自身が表示する URL で接続先を指定できます。

ホスト自身が編集する場合は、別ターミナルで `git clone ./demo.git host-work` して作業し、
ローカルの bare リポジトリへ push してください。

## OS ごとの構成

| OS | Bluetooth の実装 | 補足 |
|---|---|---|
| Windows | Rust + Winsock RFCOMM / SDP、Win32 のアドレス取得 | SDP での UUID 指定とチャネル直接指定の両方に対応 |
| macOS | Rust + 小さな Objective-C / IOBluetooth ブリッジ | ヘルパーのメインループと権限を `.app` 内に分離 |
| Linux | Rust / BlueZ（bluer）+ SDP 検索用の小さな C ブリッジ | bluetoothd と、その D-Bus へのアクセス権限が必要 |

Mac は初回利用時に `~/Library/Caches/blugit/` にヘルパー `.app` を生成します。
Bluetooth 利用許可は `blugit` に与えてください。IOBluetooth のイベントループと RFCOMM の
読み書きはヘルパーで処理し、Rust とはローカルの Unix socket で通信します。IP ポートは開きません。
ビルドには Objective-C コンパイラーを使いますが、実行時の Python 依存はありません。

Mac でホストの停止・起動を繰り返すと、OS に古いチャネルが残る場合があります。
接続できなくなった場合は Bluetooth をオフ・オンし、ホストを起動し直してください。
同じホストへの Git 操作は順番に実行します。

Rust 版 `0.2.0` の Mac ブリッジには、データ待ちを切断と誤認する不具合がありました。
Mac から Windows への push で両端に `unexpected end of file` が出る報告を受けて調査し、
`dispatch_io_create` が `dup()` 元とも共有される `O_NONBLOCK` を設定するのに、
受信側が `EAGAIN` / `EWOULDBLOCK` を EOF と同じ扱いにしていたことを確認しました。
`0.2.1` では読み取り可能になるまで待ち、EOF・一時的なデータ待ち・実際の I/O エラーを区別します。
この fd の動作は [Apple の API 仕様](https://developer.apple.com/documentation/dispatch/dispatch_io_create)
にも記載されています。

修正後のソースを Mac に反映し、`cargo install --path . --locked --force` で入れ直してから
`git push` を再実行してください。`blugit --version` と `git-remote-blugit --version` が
ともに `0.2.1` であることを確認します。ヘルパー `.app` は新しいバイナリから自動生成されます。
OS の再起動は不要です。Windows も更新し、起動中のホストを停止・再起動すると、
REQUEST 受信前・Git 転送中・最終 ACK 待ちのどこで切れたかをログで確認できます。

Linux は BlueZ の既定アダプターを使います。OS の Bluetooth 設定などで有効にしてから起動します。
アプリは OS の認証・認可ポリシーを変更しません。

## 実機確認

1. 両端末でインストールし、Bluetooth を有効にする。
2. Wi-Fi / Ethernet を切ってホストを起動する。
3. 表示された URL でゲストが clone → コミット → push する。
4. ホストで `git --git-dir=demo.git log --oneline --all` を確認する。
5. ホストで `git clone ./demo.git host-work` し、別ファイルをコミットして push する。
6. ゲストで `git pull --ff-only` してファイルを確認する。
7. ホストとゲストを入れ替えて確認する。

Python 版の clone / push / pull は利用者の実機で成功しています。
Rust 版は Windows 上で Git の往復・Python 版との双方向互換性をテストしています。
Mac / Linux のネイティブビルドと無線相互接続は各 OS での確認が必要です。
CI は Windows / macOS / Linux のビルドと無線部分を置き換えたテストを実行する構成です。

## 開発・テスト

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --features test-transport
cargo build --release --locked
```

`test-transport` はテスト専用のループバック通信を有効にする機能です。
実際の Git CLI と Remote Helper で、空の clone、バイナリ転送、push / pull、
non-fast-forward 拒否、rebase 後の push、オブジェクト整合性、Git エラー伝達を検証します。
**配布・実機利用のビルドには `test-transport` を付けません。**

Mac では実際の Dispatch I/O と Unix socket を使い、初回送信を遅延させる回帰テストも実行します。
このテストは Bluetooth API を呼ばないため、Bluetooth 機器や利用許可がなくても実行できます。

```sh
cargo test --features test-transport dispatch_io_nonblocking_duplicate_waits_for_delayed_data
```

Python 版との互換性テスト（開発時のみ uv を使用）:

```sh
uv sync --project reference/python --locked
uv run --project reference/python pytest -q reference/python/tests
```

さらに `BLUGIT_RUST_HELPER` に Rust の `git-remote-blugit` の絶対パスを指定すると
「Rust ゲスト → Python ホスト」、`BLUGIT_RUST_HOST` に Rust の `blugit` の絶対パスを
指定すると「Python ゲスト → Rust ホスト」を、同じ Python の Git 往復テストで確認できます。
対象は `reference/python/tests/test_git.py` です。Rust 側は先に
`cargo build --features test-transport --bins` でテスト用にビルドしてください。

通信仕様は [docs/protocol.md](docs/protocol.md) を参照してください。
