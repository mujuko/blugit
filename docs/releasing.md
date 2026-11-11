# リリース手順

配布には[cargo-dist](https://axodotdev.github.io/cargo-dist/) 0.34.0を使います。
設定は `dist-workspace.toml`、生成されるGitHub Actionsは `.github/workflows/release.yml` です。
Windows x64、macOS Intel / Apple Silicon、Linux x64向けに、`blugit` と `git-remote-blugit` を同じアーカイブに収めます。
PowerShell / shellインストーラーも両方の実行ファイルをインストールします。

配布ビルドでは `test-transport` を有効にしません。
LinuxはUbuntu 22.04上でビルドし、ビルド時に `pkg-config` と `libdbus-1-dev` をインストールします。
利用者にはBlueZ、D-Busの実行時ライブラリ、glibc 2.35以降が必要です。
macOSのBluetoothヘルパーは実行ファイルに埋め込まれるため、別途配布する必要はありません。

## 設定を変更・検証する

[公式インストール手順](https://github.com/axodotdev/cargo-dist/releases/tag/v0.34.0)でcargo-dist 0.34.0を導入してください。
実行コマンド名は `dist` です。ローカルでの検証にはRustと、READMEに記載したOSごとのビルド環境も必要です。

設定を変更したら、ワークフローを再生成します。生成済みのYAMLを直接編集せず、`dist-workspace.toml` を変更してください。

```sh
dist generate
dist generate --check
dist plan
dist build
```

`dist plan` は全対象の配布計画を確認し、`dist build` は現在のPC向けのアーカイブとインストーラーを `target/distrib/` に生成します。
ローカルで生成したインストーラーもGitHub Releasesから取得するため、公開前はそのまま実行できません。
Pull Requestでは配布計画と生成ファイルの整合性を検証し、公開は行いません。

## GitHub Releasesへ公開する

初回のリポジトリ公開時は、[セキュリティ検査と公開時の設定](security-maintenance.md)も実施してください。
依存関係やバージョンを変更したら、ライセンス一覧を再生成してコミットし、`Security` ワークフローの成功も確認します。
配布アーカイブには `LICENSE`、`NOTICE`、`THIRD_PARTY_LICENSES.html` を含めます。

1. `Cargo.toml` のバージョンを決め、変更した場合は `cargo check --locked` で確認します。ロックファイルの更新が必要なら `cargo check` で更新し、`Cargo.lock` もコミットしてください。
2. `dist generate --check` と `dist plan` の成功を確認し、リリース対象のコミットをGitHubへpushします。対象の `Rust` ワークフローの成功を確認してください。`main` へのpushやPRで起動していなければ、対象ブランチで手動実行します。
3. Actionsの `Release check` を対象コミットのブランチで手動実行し、全対象の配布物とインストーラーの生成が成功することを確認します。このワークフローはGitHub Releaseを作成しません。
4. 初回はリポジトリを公開し、Private vulnerability reportingの有効化と公開設定の確認まで済ませます。
5. 最後に、そのコミットへ `v` + バージョンのタグを付けてpushします。`1.0.0` の例は以下です。

```sh
git tag v1.0.0
git push origin v1.0.0
```

`Release` ワークフローが対象OSのビルド、アーカイブ・インストーラー・チェックサムの生成、GitHub Releaseへのアップロードを行います。
標準の `GITHUB_TOKEN` を使用するため、配布用のシークレットを別途登録する必要はありません。

公開後は、[リリース一覧](https://github.com/mujuko/blugit/releases)で成果物を確認し、READMEのインストールコマンドで両方の実行ファイルが導入されることを確認してください。
`releases/latest` のインストールURLは最新の安定版を指します。プレリリースは各リリースページに表示されるコマンドを使ってください。
