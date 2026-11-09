# Changelog

## 1.0.0 - 2026-11-10

初回の安定版リリースです。

- Bluetooth Classic（RFCOMM）経由で、1つのbare Gitリポジトリを共有できます。
- 6桁の確認番号で端末を登録し、以後はSSHの公開鍵認証と暗号化でGitを転送します。
- `blugit://登録名` を使い、通常のGitでclone・push・pullできます。
- ホスト再起動でチャネルが変わった場合も、認証して接続先を更新します。
- Windows x64、macOS Intel / Apple Silicon、Linux x64の配布物とインストーラーを提供します。

Linuxの実Bluetooth検証は未実施です。OSごとの確認範囲と既知の制約は
[検証記録](docs/validation.md)と[セキュリティ方針](SECURITY.md)を参照してください。
