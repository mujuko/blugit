# Prototype wire protocol

この文書は従来の認証なしモードの仕様です。`blugit spike` の端末登録・認証・
暗号化レイヤーは [段階1の技術検証](stage1-spike.md) を参照してください。

Rust と `reference/python` は同じプロトコルを使います。
この段階では暗号化・認証・バージョン交渉を追加しません。

- RFCOMM サービス UUID: `2d508ec7-69a4-47f2-a90c-b54df08c3cf5`
- URL: `blugit://AA-BB-CC-DD-EE-FF`（SDP 検索）
- 直接指定: `blugit://AA-BB-CC-DD-EE-FF?channel=7`
- 1 接続で 1 回の Git サービスを実行します。

各フレームは、種類 1 byte + payload 長 4 bytes（unsigned / big endian）+ payload です。
長さは最大 65536 bytes。データ転送は通常 32768 bytes ごとに区切ります。
RFCOMM やローカルソケットの read / write 境界はフレーム境界と一致しません。

| 値 | 種類 | 方向 | 内容 |
|---|---|---|---|
| 1 | REQUEST | guest → host | `git-upload-pack` または `git-receive-pack` |
| 2 | READY | host → guest | 空。Git プロセス起動後に返す |
| 3 | DATA | 双方向 | Git の stdin / stdout のバイナリデータ |
| 4 | EOF | guest → host | 空。ホストの Git stdin を閉じる |
| 5 | STDERR | host → guest | Git の stderr |
| 6 | EXIT | host → guest | Git の終了コードを ASCII の整数で送る |
| 7 | ERROR | host → guest | 接続を実行できない理由 |
| 8 | ACK | guest → host | 空。EXIT と、それ以前の出力を受け取ったことを確認する |

REQUEST → READY の後に Git の双方向転送を開始します。拒否時は ERROR を返します。
ホストは stdout / stderr を送り終えて Git プロセスが終了したら EXIT を送信します。
ゲストが ACK を返すまでチャネルを保持し、未転送の出力を残したまま切断しないようにします。
ゲストの EOF を表すために、RFCOMM チャネル自体を先に閉じてはいけません。

初回 REQUEST / READY は 15 秒、接続は 30 秒、終了時 ACK は 10 秒のタイムアウトがあります。
大きな履歴を転送できるように、Git のデータ転送全体には固定の時間制限を設けません。
Mac の権限確認・ホスト初期化は別途最大 60 秒待機します。

Git Remote Helper は `connect` capability を広告します。`connect git-upload-pack` /
`connect git-receive-pack` に対し、Bluetooth 側の READY 後に空行を返し、その後の
標準入出力をバイナリ転送へ切り替えます。診断は stderr にだけ出力します。
切断エラーには、初回 REQUEST / READY、Git データ転送、最終 ACK の段階と、
読み取り中だったフレームの種類・長さ・payload の情報を付けます。

Rust の通常ビルドに IP 通信のフォールバックはありません。`test-transport` を明示した
テスト用ビルドだけが `BLUGIT_TEST_ADDR` のループバック接続と `__test-host` を使えます。

## ソース構成

- `src/git.rs`: bare リポジトリ検証、Git 子プロセス、フレーム転送・終了処理。
- `src/helper.rs`: Remote Helper のテキスト制御とバイナリ relay。
- `src/wire.rs`: フレームのエンコード・デコード。
- `src/endpoint.rs`: アドレス / URL の検証。
- `src/platform/`: OS ごとの RFCOMM ストリーム。
- `native/macos.m`: 権限を持つヘルパーの起動、IOBluetooth の run loop とコールバック。
- `native/linux.c`: libbluetooth による SDP の UUID → チャネル検索。

Mac では `blugit` 本体のコピーを最小 `.app` に置き、ネイティブヘルパーモードで起動します。
権限の責任主体を端末アプリから切り離すため、Apple の responsibility spawn 属性を利用します。
この部分は OS 依存であり、各 macOS バージョンで実機確認が必要です。
