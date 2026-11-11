# Changelog

## 1.0.2 - 2026-11-12

- `blugit host` の表示を整理し、接続コードとリポジトリ名を使った `blugit join` のコマンド例を表示します。
- 登録申請、6桁の確認番号、登録後の `git clone` の案内を読みやすくしました。
- 通常のRust CIをPR・`main` へのpush・手動実行に絞り、配布ビルドをリリース前チェックとタグ公開に集約しました。

## 1.0.1 - 2026-11-10

- 依存関係を更新しました（rand 0.10、x25519-dalek 3、dirs 7、uuid 1.29.1、cc 1.11.1）。
- randの新APIに対応し、鍵・確認番号・nonceには引き続きOSの暗号学的乱数を使います。
- リリース用のcheckoutをv7のコミットへ固定し、cargo-distから再生成できる設定にしました。

## 1.0.0 - 2026-11-10

初回の安定版リリースです。

- Bluetooth Classic（RFCOMM）経由で、1つのbare Gitリポジトリを共有できます。
- 6桁の確認番号で端末を登録し、以後はSSHの公開鍵認証と暗号化でGitを転送します。
- `blugit://登録名` を使い、通常のGitでclone・push・pullできます。
- ホスト再起動でチャネルが変わった場合も、認証して接続先を更新します。
- Windows x64、macOS Intel / Apple Silicon、Linux x64の配布物とインストーラーを提供します。

Linuxの実Bluetooth検証は未実施です。OSごとの確認範囲と既知の制約は
[検証記録](docs/validation.md)と[セキュリティ方針](SECURITY.md)を参照してください。
