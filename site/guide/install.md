# インストール

blugit は、<Role who="host" />と<Role who="guest" />の**両方のPC**にインストールします。

## 動作環境

| 項目 | 必要なもの |
|---|---|
| OS | Windows（x64）、macOS（Intel / Apple Silicon）、Linux（x64、glibc 2.35以降） |
| Bluetooth | Bluetooth Classic（RFCOMM）に対応したもの。BLE専用のアダプターは使えません |
| Git | インストール済みであること（[下の手順](#gitをインストールする)） |

::: info Linux について
Linux は、実機での動作確認がまだ済んでいません。
:::

ターミナルを使ったことがない人は、先に[ターミナルの基本](./terminal)を読んでおくと安心です。

## Gitをインストールする

Git が入っていなければ、先にインストールします。

::: code-group

```powershell [Windows]
winget install --id Git.Git -e --source winget
```

```sh [macOS]
git --version   # 入っていなければ、インストールを勧める画面が出る
```

```sh [Linux]
sudo apt-get update
sudo apt-get install -y git
```

:::

- **Windows**：`winget` が使えない場合は、[Gitの公式サイト](https://git-scm.com/downloads/win)からインストーラーをダウンロードして実行します。
- **macOS**：「コマンドライン・デベロッパツール」のインストールを勧める画面が出たら、「インストール」を押します。

## blugit をインストールする

インストールにはインターネット接続が必要です。ターミナルで、OSに合わせたコマンドを実行してください。

::: code-group

```powershell [Windows]
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force
irm https://github.com/mujuko/blugit/releases/latest/download/blugit-installer.ps1 | iex
```

```sh [macOS]
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/mujuko/blugit/releases/latest/download/blugit-installer.sh | sh
```

```sh [Linux]
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/mujuko/blugit/releases/latest/download/blugit-installer.sh | sh
sudo apt-get update
sudo apt-get install -y bluez libdbus-1-3
```

:::

### Windows の2行の意味

Windows では、PowerShell のプロンプトで2行を順番に実行します。

<AnnotatedCommand
  command="Set-ExecutionPolicy {-Scope Process} {-ExecutionPolicy Bypass} -Force"
  :notes="[
    ['-Scope Process', '今開いている PowerShell のウィンドウにだけ効く設定にします。ウィンドウを閉じれば元に戻ります'],
    ['-ExecutionPolicy Bypass', 'ダウンロードしたインストール用スクリプトを実行できるようにします'],
  ]"
/>

<AnnotatedCommand
  command="irm {https://github.com/…/blugit-installer.ps1} | {iex}"
  :notes="[
    ['インストーラーのURL', 'blugit の公開ページから、インストール用スクリプトをダウンロードします（irm）'],
    ['iex', 'ダウンロードしたスクリプトを実行します'],
  ]"
/>

### macOS・Linux

`curl` でインストール用スクリプトをダウンロードして、そのまま実行します。
Linux では、Bluetooth を使うために必要な BlueZ と D-Bus のライブラリも入れてください（上は Debian / Ubuntu の例です）。

## インストールできたか確かめる

**ターミナルを一度閉じて開き直し**、次の2つを実行します。

```sh
blugit --version
git-remote-blugit --version
```

<div class="screens">

<TerminalScreen
  command="blugit --version"
  :lines="[
    ['blugit 1.0.2', 'バージョン番号は、そのときの最新版になります'],
  ]"
/>

<TerminalScreen
  command="git-remote-blugit --version"
  :lines="[
    ['git-remote-blugit 1.0.2', '上と同じバージョンなら完了です'],
  ]"
/>

</div>

### git-remote-blugit とは

インストールすると、`blugit` と `git-remote-blugit` の2つのプログラムが入ります。

- `blugit`：共有を始めたり、登録したりするときに、自分で実行するプログラム
- `git-remote-blugit`：Gitが `blugit://` で始まる場所に接続するときに、**Gitが自動で呼び出す**プログラム

`git clone blugit://demo` を実行すると、Gitは `blugit://` の部分を見て `git-remote-blugit` に接続を任せます。
自分で `git-remote-blugit` を実行することは、確認のとき以外ありません。

::: warning コマンドが見つからないとき
「見つからない」というエラーが出たら、まずターミナルを開き直してください。
それでも見つからなければ、インストールのコマンドをもう一度実行してみてください。
:::

## 更新する

新しいバージョンに更新するときも、同じインストールコマンドを実行します。ホストとゲストで、なるべく同じバージョンにそろえておきましょう。

:::: details ソースからビルドする

Rust（stable）と、OSごとのビルド環境を用意してください。

| OS | ビルド環境 |
|---|---|
| Windows | Rust の MSVC ツールチェーン、Visual Studio C++ Build Tools |
| macOS | Xcode Command Line Tools |
| Linux | BlueZ、`pkg-config`、`libdbus-1-dev`（Debian / Ubuntu の場合） |

blugit のリポジトリを clone したフォルダで、次を実行します。

```sh
cargo install --path . --locked --force
```

コマンドが見つからない場合は、ホームフォルダの `.cargo/bin` を PATH に追加してください。

::::
