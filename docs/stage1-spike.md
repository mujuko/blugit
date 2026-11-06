# 段階1：端末登録・暗号化Git・近隣探索の技術検証

## 目的と範囲

`prototype` ブランチの技術検証（現在 `0.3.2`、初版 `0.3.0`）で、以下の成立条件を確認する。

1. 申請時にホストだけに出る6桁番号で、ゲスト端末を初回登録する。
2. 次回以降は番号入力なしで双方の登録済み鍵を確認し、通常の `git clone/pull/push` を行う。
3. 近隣の広告有効チームを探索する。広告無効でも直接指定の参加は可能にする。

チームは固定名 `prototype` の1つ、登録対象はホスト起動時に指定するbareリポジトリ1つ。
アカウント、恒久パスワード、チーム作成・削除、キック、脱退は今回実装しない。
段階2から `main` に新しい構成を作る。旧実装との後方互換性は本実装の要件に含めない。
比較用の旧コマンド `blugit host` は残しているが、検証時は必ず `blugit spike host` を使う。

最新の利用者実機報告：初回登録、登録済み鍵でのclone/push/pull、単独のチャネル指定なし
`spike probe` は成功。一括の `spike discover` はホストを候補に含むものの接続に失敗している。
単独probeは `0.3.0` の時点でも成功していたため、「SDPが常に壊れている」とは判断しない。

## 実行環境とソース

最新の `0.3.2` のソースを使う。未コミットの作業をMacで試す場合は、
この作業で作る `blugit-stage1-0.3.2-source.zip` をUSB等でコピーして展開する。
その場合は `git pull` ではなく、ZIP展開先でインストールする。
公開リモートに今回の変更が反映されたとは限らない。

Git・Rust stable・OS別のビルドツールは [README](../README.md) と同じ。
依存クレート取得とインストールはオンラインで済ませ、その後Wi-Fi/Ethernetを切って試す。

```sh
cargo install --path . --locked --force
blugit --version
git-remote-blugit --version
```

インストールした両バイナリが `0.3.2` であること。実機には `--features test-transport` を付けない。
今回の探索修正だけを試す場合、Mac側だけを更新し、Windowsホストは `0.3.1` のままでもよい。
認証・Git・広告応答のプロトコルと状態形式は `0.3.1` から変更していない。
旧Python環境よりCargoのbinがPATHの先にあることも確認する。

## 手順A：Windowsホスト → Macゲスト

### 1. Windowsでホスト起動

検証用の作業ディレクトリを作り、PowerShellで実行する。

```powershell
git init --bare --initial-branch=main demo.git
blugit spike identity --name Windows-PC
blugit spike host ./demo.git
```

ホストは `Join`、`Clone after joining`、`Direct channel URL` を表示する。
以降の `AA-BB-CC-DD-EE-FF` は、このホストが表示したアドレスに置き換える。
ホストは起動したままにする。

### 2. Macで直接指定の初回登録

```sh
blugit spike identity --name MacBook
blugit spike join 'blugit://AA-BB-CC-DD-EE-FF?secure=1'
```

ホストのターミナルに、チーム名・申請端末名・短い端末IDと `CODE=123456` のような番号が出る。
ホスト操作者が申請端末を対面で確認し、その番号をゲストに伝える。
ゲストは `Six-digit code:` に入力する。通常のターミナル入力で、入力した番号は表示される。
成功は `Joined prototype ...`。ホスト側で別の承認キーを押す操作はない。

番号は毎申請生成し、1申請につき照合1回、期限180秒。同時に待てる申請は1つ。
新規発行はホスト全体で10秒以上間隔を空ける。番号失敗・取消後は、新しく `join` して
新しい番号を使う。ゲストのCtrl+Cで申請を取り消せる。ホスト再起動で待機中の申請は失効する。
既登録端末による再申請でも、新しい番号が必要。

接続先チャネルの検索で失敗した場合は、ホストの `Direct channel URL` を使う。
例：`'blugit://AA-BB-CC-DD-EE-FF?channel=7&secure=1'`。
クエリに `&` があるため、URLは必ず引用符で囲む。

### 3. Macからclone・push

```sh
git -c init.defaultBranch=main clone 'blugit://AA-BB-CC-DD-EE-FF?secure=1' guest-work
cd guest-work
git config user.name 'Stage1 Mac'
git config user.email 'stage1-mac@example.invalid'
printf 'from Mac\n' > from-mac.txt
git add from-mac.txt
git commit -m 'Stage1 Mac push'
git push -u origin main
```

成功条件：番号の再入力なしでpushでき、Windows側で次のコミットが見える。

```powershell
git --git-dir=demo.git log --oneline --all
```

### 4. Windowsで変更してMacからpull

Windowsの別ターミナルで、`demo.git` のあるディレクトリから実行する。

```powershell
git clone ./demo.git host-work
cd host-work
git config user.name 'Stage1 Windows'
git config user.email 'stage1-windows@example.invalid'
Set-Content from-windows.txt 'from Windows'
git add from-windows.txt
git commit -m 'Stage1 Windows update'
git push
```

Macの `guest-work` で実行する。

```sh
git pull --ff-only
cat from-windows.txt
```

### 5. ホスト再起動・端末名変更

WindowsホストをCtrl+Cで停止し、同じ作業ディレクトリで `blugit spike host ./demo.git` を再実行。
Macでは `blugit spike identity --name MacBook-Renamed` の後、`guest-work` で `git pull --ff-only`。
成功条件：番号なしで再接続する。名前変更で端末IDが変わらない。
直接チャネルURLでcloneした場合、再起動後のチャネル変更は `git remote set-url origin '新しいURL'`
で更新する。チャネルなしURLはサービスを再検索する。

## 手順B：探索と広告無効

### 探索と直接指定を分けて確認する

ホストPCをOSのBluetooth設定で検出可能にする。アプリはOS全体の検出可能設定を自動変更しない。
Windowsでは「その他のBluetooth設定」等にある「BluetoothデバイスによるこのPCの検出」を確認する。
macOSではBluetooth設定を開いた状態など、使用中のOSで検出可能になる条件を実機で確認する。
これはアプリのチーム広告設定とは別の条件であり、設定画面を開けば必ず見えるとは未確認。

ゲスト側で実行する。

```sh
blugit spike discover --seconds 8 --verbose
```

未ペアリング端末も対象にClassic Bluetoothの問い合わせを行い、候補端末に接続して
blugitの探索応答を確認する。候補にはOSのキャッシュが混ざる場合もある。
検証では候補を重複排除して最大32台に制限している。
接続は単独の `probe` と同じOS別タイムアウト（通常30秒、Macの権限・初期化待ちは別途最大60秒）、
接続後の広告応答は最大15秒。探索だけで接続を5秒で中断する処理は `0.3.2` で廃止した。
`--seconds` は問い合わせ期間で、コマンド全体の終了時間ではない。
MacではOSによる開始待ちを最大45秒、開始後は問い合わせ期間＋10秒まで待つ。
待ち時間と実探索時間を分けてログに出し、完了せず0件の場合はエラーにする。
途中で得た候補がある場合は、部分結果であることを表示してプローブする。
Macでは停止の戻り値を確認し、問い合わせ補助プロセスが自然終了するまで待ってから、
さらに1秒空けて接続を始める。これによるOS側の状態の回復は実機確認が必要。
探索に出た名前・端末IDは、番号で初回登録するまでは未確認の情報。

探索と接続先の問題を切り分けるには、既知のURLを直接プローブする。

```sh
blugit spike probe 'blugit://AA-BB-CC-DD-EE-FF?secure=1'
```

ホストを停止し、今度は広告無効で再起動する。

```powershell
blugit spike host ./demo.git --hidden
```

成功条件：`discover` にはチームが出ず、`probe` は `Host reachable; test team is hidden.`。
直接URLによる新しい端末の `join` は可能で、新しい番号を求める。
登録済み端末からのGit操作も可能。Bluetooth端末や共通サービス自体の存在を隠す機能ではない。

## 手順C：逆方向と拒否動作

Macをホスト、Windowsをゲストに入れ替え、別のbareリポジトリで同じ登録→clone→push→pullを試す。
各端末の鍵はホスト役・ゲスト役で共用できるが、相手への所属は方向別なので逆方向でも初回登録する。
Macで同じ相手へのGit操作は順番に実行する。

誤った番号や未登録端末の確認には、新しいゲスト状態を使う。
Macの別ターミナルなら、例えば次を設定し、同じ名前でも別の鍵になることを確認する。

```sh
export BLUGIT_STATE_DIR="$HOME/Library/Application Support/blugit/stage1-negative"
blugit spike identity --name MacBook-Renamed
blugit spike join 'blugit://AA-BB-CC-DD-EE-FF?secure=1'
```

誤った番号なら登録失敗し、その状態でsecure URLをcloneしても拒否されること。
取消・180秒放置でも登録されないこと。元に戻すにはそのターミナルで
`unset BLUGIT_STATE_DIR`、またはターミナルを閉じる。

## 状態の保存

`blugit spike identity` が実際の保存先を表示する。
既定はOSのユーザーデータディレクトリ以下の `blugit/prototype-security`。
Windowsは通常 `%LOCALAPPDATA%\blugit\prototype-security`、macOSは通常
`~/Library/Application Support/blugit/prototype-security`。

- `identity.json`：端末名・X25519公開鍵・秘密鍵。Gitの作者名とは独立。
- `member-<公開鍵hex>.json`：ホストが許可したゲスト端末。
- `host-<Bluetoothアドレスhex>.json`：ゲストが番号確認で登録したホスト。

ファイル更新は一時ファイルからの置換。Unixは保存先0700、状態ファイル0600。
Windowsは保存先のACLを継承する。秘密鍵のOS資格情報ストアへの格納はこの検証の範囲外。
状態ディレクトリを他端末へコピーすると同じ端末IDになるため、ソースZIPには含めない。

`--state-dir` で保存先を変えた場合、Git Remote Helperにも同じ `BLUGIT_STATE_DIR` を設定する。
ホストの鍵が変わった接続先は自動信頼せずエラー。対面で確認してから新しく `join` する。
保存ファイルが壊れている場合も鍵を黙って作り直さない。

## 通信の構成と出典

```text
初回：RFCOMM → 申請・ホストの6桁番号表示 → SPAKE2 → Noise XXpsk3 → 双方の鍵を保存
以後：RFCOMM → 保存済み鍵でNoise KK → 認証済み通知 → 従来のGitフレーム → Gitプロセス
```

初回は `spake2 0.4` のEd25519Groupで共有鍵を導出し、`snow 0.10` の
`Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` のPSKに使う。
両端末の公開鍵・名前・操作種別・ホストのランダムnonceをPAKEの識別情報とNoiseのprologueに含め、
Noiseが確認した相手の静的鍵と申請情報の鍵を照合する。
暗号化した登録確認を受け取って初めてホスト側のメンバーを保存し、
ゲストは暗号化した受理通知を受け取ってホストを保存する。
`0.3.1` はさらにゲストの保存完了通知とホストの最終応答を交換し、最後の送信が
無線の相手に届いたことを確認してからゲストが終了する。下位ストリームのEOFは登録成功条件にしない。
最終通知の受信前などに切断すると片側だけ保存済みになる可能性がある。新しい番号で再登録できる。
番号自体やPAKEの導出鍵は保存・送信しない。

再接続は `Noise_KK_25519_ChaChaPoly_BLAKE2s`。
保存済みホスト鍵の照合、メンバー鍵の登録確認、双方の鍵所持の確認が済むまでGitを起動しない。
汎用シェル・任意コマンド実行は提供しない。SSHは初期候補だったが、この検証ではNoiseを採用した。
組み合わせたアプリプロトコルの独立したセキュリティレビューは未実施で、本実装への採否は段階1で判断する。
`spake2 0.4` 自体も独立監査未実施・タイミング攻撃への保護未保証と公式に記載されている。
今回の使い捨て番号による技術検証と、本実装でのライブラリ選定・安全性評価は区別する。

`?secure=1` はこの技術検証のURL識別。認証失敗時の平文フォールバックはない。
secureホストは旧プロトタイプの平文Git要求も拒否する。
従来のGitフレームを、最大16384バイトずつNoiseの認証付き暗号化レコードで包む。
レコードは2バイトbig endian長＋ciphertext。暗号化payloadの先頭0はデータ、1は明示的な終了。
改ざん・同じレコードの再送・明示終了前の切断を拒否／エラーとして扱う。
Git転送の正常終了自体は既存のEXIT/ACKで判断する。

一次資料：

- [RustCrypto SPAKE2](https://docs.rs/spake2/0.4.0/spake2/)：1回の実行で可能になるオンライン推測と、相互の鍵確認の必要性。
- [Snow Builder](https://docs.rs/snow/0.10.0/snow/struct.Builder.html)、[Noise仕様](https://noiseprotocol.org/noise.html)：PSK、prologue、静的鍵、KK/XXとトランスポート。
- [Windows探索パラメーター](https://learn.microsoft.com/en-us/windows/win32/api/bluetoothapis/ns-bluetoothapis-bluetooth_device_search_params)：未登録端末を含む問い合わせ、1.28秒単位の期間。
- [Apple IOBluetoothDeviceInquiry](https://developer.apple.com/documentation/iobluetooth/iobluetoothdeviceinquiry)：近隣探索。
- [Apple getServiceRecord](https://developer.apple.com/documentation/iobluetooth/iobluetoothdevice/getservicerecord(for:))、[performSDPQuery](https://developer.apple.com/documentation/iobluetooth/iobluetoothdevice/performsdpquery(_:uuids:))：前者はキャッシュ検索。`0.3.1` はbaseband接続後のキャッシュを先に利用し、未取得の場合だけ問い合わせる。
- [BlueZ Adapter API](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Adapter.rst)：BR/EDR探索と検出可能設定。

## 確認済みと未確認

2026-11-06、Windows上で以下を確認。無線部分を置き換えたテストと実機観測を区別する。

| 対象 | 結果・証拠の範囲 |
|---|---|
| 初回登録、双方の鍵保存、名前変更・状態再読込後の再認証 | 自動テストで確認。無線はループバック／duplexに置換 |
| 誤番号・未登録鍵・ホスト鍵変更・期限切れ・取消・発行制限・平文要求拒否 | 自動テストで確認 |
| バイナリ・短いread/write・バックプレッシャー・暗号文の改ざん／再送／切断 | 自動テストで確認 |
| 通常のGit CLIとRemote Helperによるsecure clone/push/pull | 自動テストで確認。バイナリ、競合push拒否、rebase、fsckを含む |
| 広告無効の非表示と、直接指定での登録 | プロトコルを自動テストで確認。無線探索は未確認 |
| Windowsのネイティブ問い合わせ | release版で `spike discover --seconds 2 --verbose` を実行。候補6件（キャッシュを含み得る）、プローブはタイムアウトまたは接続先サービス未解決、広告有効チーム0件。相互探索の成功証拠にはしない |
| macOS／Linuxのネイティブビルド・探索 | このWindows作業環境では未確認。既存CIの3 OSチェック対象 |
| Windows ↔ Macの無線上での番号登録・暗号化Git・再接続 | 利用者の手元のMacと合わせて手順A〜Cで確認する。未完了 |

再現用：

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --features test-transport
cargo build --release --locked
```

Windows上では上記4コマンドが成功。Rustのテストは単体17件・Git統合3件の計20件が成功。
Git統合では実際の `spike join` CLIへ番号を入力し、登録済み状態から別プロセスの
Remote Helperで接続する。release版の両バイナリの表示は `0.3.0`。

段階1の完了条件は、手順A〜Cの実機結果が揃い、探索が成立するOS設定・権限・制約を記録できること。
探索で見えなくても直接指定で通信できるかを分けて記録する。
`main`での段階2の開発は、それを踏まえて始める。

## 2026-11-07：利用者の実機結果と0.3.1での修正

Windowsホスト／macOS 27.2ゲスト、`0.3.0` の実行ログを利用者から受領した。
ここは利用者の観測であり、このWindows作業環境からMacを操作して追試した結果ではない。

| 項目 | 観測・判断 |
|---|---|
| 6桁番号での初回登録 | Macに `Joined prototype ...`。その出力までの実装経路で双方の鍵の確認・保存が成功している |
| 登録後のホストログ | `encrypted stream truncated before authenticated close`。登録保存後、末尾の終了データを待つ処理でエラー。登録失敗とは区別する |
| 近隣探索 | MacのIOBluetooth問い合わせがタイムアウト。探索が完了して0件だったという証拠ではない |
| チャネルなし接続 | MacのSDP問い合わせがタイムアウト。Git処理や再認証より前で停止 |
| 直接チャネル＋secure URLでのpush | 利用者がoriginを `?channel=4&secure=1` に変更すると `main -> main` が成功。保存済み鍵での相互認証と暗号化したGit pushを実機で確認 |
| 元のorigin | `secure=1` なしの旧モードURL。SDP問題の解消に加え、認証付きURLへの変更も必要だった |

認識の修正：初版では「明示的なSDP問い合わせを必ず行えば改善できる」と実装したが、
APIの成功戻り値は開始の受理であり、利用環境で完了通知が来ることまでは確認できていなかった。
[aio-rfcomm開発元の観測](https://dlech.github.io/aio-rfcomm/known-bugs/#asking-a-device-for-its-services-does-not-ask-it)
にもmacOS 26.6.2でキャッシュ利用やコールバック未到達の事例がある。
macOS 27.2で今回の原因が同一とは未確定。

`0.3.1` の変更：

- 登録後の終了を、ゲストACK→ホスト最終応答に変更。Macの補助プロセスを切る前に、最後の送信が届いたことを確認する。
- ローカル書き込みを別タスクで遅延転送し、親ストリームの破棄で未送信データが失われる回帰テストを追加。
- MacのSDPは接続後のキャッシュを優先し、必要な場合だけ問い合わせ、完了通知待ち中もレコードの取得を確認する。
- Macの探索は開始待ちと実探索の時間を分離し、Core Foundationのメインrun loopで待つ。
  [Appleの開始通知仕様](https://developer.apple.com/documentation/iobluetooth/iobluetoothdeviceinquirydelegate/deviceinquirystarted(_:))は、OSの抑制によって開始通知が遅れる場合があるとしている。
- ヘルパーの具体的なエラーをRustへ渡す。探索タイムアウトを一律に権限・アダプター問題として表示しない。
- ホストに `Registered ...` と `Authenticated ...` を表示し、登録と再認証を見分けられるようにする。

状態ファイルの形式は変更しない。両端末を更新しても既存の登録は維持でき、pushの再試験に再登録は不要。
新しい登録終了処理を試す場合は両端末を `0.3.1` に揃える。
修正後のMacの探索・SDP・登録終了と、pull・逆方向転送はまだ実機未確認。

Windowsでの修正版の検証は、テスト18件＋Git統合3件の計21件が成功。
フォーマット、Clippy、releaseビルドも成功。Macのネイティブ部分はこの環境ではビルドしていない。

再試験は両端末のソースを更新して `cargo install --path . --locked --force` を実行し、
両バイナリが `0.3.1` と表示されることを確認して、Windowsホストを再起動する。
最初は直接チャネル付きURLのまま登録済み鍵でpushできることを確認する。
次にMacで、ホストのアドレスを指定して以下を実行する。

```sh
blugit spike probe 'blugit://AA-BB-CC-DD-EE-FF?secure=1'
blugit spike discover --seconds 8 --verbose
```

前者はSDP解決、後者は近隣問い合わせ＋SDP解決を含む。
チャネル番号はホスト再起動で変わることがあるため、直接URLは現在表示された値を使う。
登録終了のエラー解消を確認する場合だけ、現在の直接URLでもう一度 `spike join` する。

## 2026-11-07：追加実機結果と0.3.2での修正

利用者からclone/push/pullの成功報告と、以下の探索ログを受領した。

- `0.3.1` の問い合わせは開始通知を受け取らず、45秒後に部分結果5件を使った。
- 部分結果にはWindowsホストのアドレスが含まれていた。新規の無線問い合わせで発見したのか、
  OS内部の既存情報によるものかはこのログだけでは確定しない。
- 先行する候補3件は各5秒で中断され、その後のホストへの接続は `0xe00002bc` で失敗。
  これはRFCOMMの接続段階のエラーで、広告応答が空だったという結果ではない。
- 単独の `spike probe 'blugit://<ホストアドレス>?secure=1'` は `prototype` とホスト情報を返した。
  利用者によれば `0.3.0` の時点でも成功していた。

認識の修正：チャネル指定なしの単独probeに成功経路がある。従って「SDP検索が常に失敗する」
とは言えず、`0.3.1` によって初めて成功するようになったとも言えない。
チームの広告情報応答は動いており、現在の焦点は問い合わせの完了通知と、一括探索時の接続条件。

コード上で確定した問題は、`discover` だけが通常の接続＋広告応答全体を5秒で打ち切る点。
Macではその打ち切りで補助プロセスも終了するため、単独probeより先に接続を中断していた。
先行する接続の中断、問い合わせ終了直後の状態、古いSDPキャッシュのいずれが今回のホスト接続エラーに
関係したかは未確定。`0xe00002bc` だけで原因を一つに決めない。

`0.3.2` の変更：

- 単独probeと一括探索から同じ接続処理を呼び、探索だけの5秒中断を削除。
- 接続が6秒かかった後に実際の広告プロトコルを交換できる回帰テストを追加。
- 問い合わせの停止成功を確認し、ENDを読むだけで補助プロセスを即終了せず、自然終了を待つ。
- `discover --address <アドレス>` で、問い合わせに含まれた対象1台だけをプローブできる。
  問い合わせに含まれなかった端末を発見済みとして追加する機能ではない。
- `--verbose` に接続開始・所要時間・接続／広告応答の失敗段階を表示。
- MacのRFCOMMエラーに、接続先チャネルと直接指定／SDP由来の区別を表示。

更新後、まずMacで次を試す。アドレスは現在のホストのものに置き換える。

```sh
blugit spike discover --address AA-BB-CC-DD-EE-FF --verbose
```

成功すれば、他の候補を先に試す条件を除いた問い合わせ→接続→広告応答の成功になる。
失敗した場合は、表示されたチャネルとWindowsホスト起動時の `Direct channel URL` を比較し、
続けて単独probeの成否を記録する。修正後のMac実機結果はまだ未確認。

Windows側では単体20件＋Git統合3件の計23件が成功。
フォーマット、Clippy、releaseビルドも成功し、両バイナリのバージョンは `0.3.2`。
この結果はMacのネイティブビルドや今回の無線エラーの解消を確認したものではない。
