# セキュリティ検査と依存関係の更新

## 自動検査

`.github/workflows/security.yml` はpush、Pull Request、毎日の定期実行で以下を検査します。

- `cargo deny --locked check`：既知の脆弱性、許可したライセンス、依存元、ワイルドカード指定。
- `cargo about`：同梱するライセンス一覧と現在の依存関係の一致。
- Gitleaks：取得できた全Git参照の履歴と、チェックアウトしたファイルの秘密情報。

Gitleaksでは標準ルールに加え、blugitの端末秘密鍵を保存した32バイトのJSON配列を検出します。
検出値はログへ出さないようマスクします。検出を隠すための一括除外やベースラインは設定しません。
これらの検査は、未知の脆弱性やあらゆる形式の秘密情報を検出できる保証ではありません。

## 依存関係の更新

DependabotはCargoとGitHub Actionsの通常更新を週1回確認し、公開から1日（24時間）経過してから更新候補にします。
PRを自動でマージする設定はしていません。既知の脆弱性を修正するセキュリティ更新には、Dependabotの仕様上この待機期間は適用されません。

この設定はDependabotの通常更新に適用されます。手動の `cargo update` や別ツールによる更新、
更新に伴って追加される推移的依存関係の公開後24時間をCIで一律に強制する設定ではありません。
手動更新時は、追加・変更された依存関係の公開日時と変更内容も確認してください。
通常のビルドでは `Cargo.lock` をコミットし、`--locked` を付けて意図しない再解決を防ぎます。

GitHub側のDependabot alertsとsecurity updatesも有効にします。
設定状態は次のコマンドで確認できます（リポジトリへの管理権限が必要です）。

```sh
gh api repos/mujuko/blugit/vulnerability-alerts --silent
gh api repos/mujuko/blugit/automated-security-fixes
```

## ローカルで再実行する

cargo-deny 0.20.2、cargo-about 0.9.3、Gitleaks 8.30.1を使います。
Rust用の2つは次の方法でも導入できます。

```sh
cargo install cargo-deny --version 0.20.2 --locked
cargo install cargo-about --version 0.9.3 --locked
cargo deny --locked check
cargo about generate --locked --fail -o THIRD_PARTY_LICENSES.html about.hbs
gitleaks git --redact=100 --log-opts=--all .
```

ライセンス一覧を更新した場合は `THIRD_PARTY_LICENSES.html` をコミットしてください。
一覧には配布対象の全OSとビルド・開発用の依存関係を含めます。
`option-ext 0.2.0` のMPL-2.0は、そのバージョンだけに例外として許可しています。
配布物には `LICENSE`、`NOTICE`、一覧を同梱し、一覧に元のライセンス本文とバージョン固定のソース取得先を記載します。
上流のソースを変更した場合は、元の取得先を示すだけで済むとせず、変更部分の公開条件を再確認してください。

## リポジトリ公開時の設定

GitHubのPrivate vulnerability reportingは公開リポジトリ向けの機能です。
非公開の間にこの設定を完了することはできません。公開直後、製品を告知する前に次を実行します。
このコマンド自体はリポジトリを公開に変更しません。

```sh
gh api --method PUT repos/mujuko/blugit/private-vulnerability-reporting
gh api repos/mujuko/blugit/private-vulnerability-reporting
```

`enabled: true` を確認し、`SECURITY.md` の報告フォームが開くことを確認してください。
Secret scanningとpush protectionもGitHubの設定画面で確認してください。
公開範囲の変更で過去のActionsログも公開されるため、公開直前に履歴・ログ・成果物の検査を更新してください。
