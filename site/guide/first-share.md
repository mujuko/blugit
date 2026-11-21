# はじめての共有

この章では、<Role who="host" /> と <Role who="guest" /> が順番に操作して、ゲストがリポジトリを clone できるところまで進めます。
図の色は、ホストが青、ゲストがオレンジです。

## 始める前に

- 両方のPCに、Git と blugit をインストールしておきます（[インストール](./install)）。
- 両方のPCで Bluetooth をオンにします。
- 途中で OS から Bluetooth の利用許可を求められたら、許可してください（macOS では初回や更新後に聞かれることがあります）。

ここからは、ホストとゲストの2人で、画面を見せ合いながら進めるとスムーズです。

## 1. ホスト：共有するリポジトリを作る

共有には、作業ファイルを持たない **bareリポジトリ** を使います。

<AnnotatedCommand
  who="host"
  command="git init {--bare} --initial-branch=main {demo.git}"
  :notes="[
    ['--bare', '作業ファイルを持たない、共有専用のリポジトリとして作ります'],
    ['リポジトリの名前', '作られるフォルダの名前です。名前は自由ですが、末尾を .git にするのが慣例です'],
  ]"
/>

::: details 既存のプロジェクトを共有したいとき

<AnnotatedCommand
  who="host"
  command="git clone --bare {./project} {demo.git}"
  :notes="[
    ['元のプロジェクト', '共有したいリポジトリのフォルダです'],
    ['共有用のリポジトリ', '新しく作られるbareリポジトリです'],
  ]"
/>

元のリポジトリとは別のコピーになるため、以後の変更は push／pull で受け渡します。

:::

## 2. ホスト：共有を始める

<AnnotatedCommand
  who="host"
  command="blugit host {./demo.git}"
  :notes="[
    ['共有するbareリポジトリ', '1で作ったフォルダの場所です。同時に共有できるのは1つだけです'],
  ]"
/>

実行すると、次のように表示されます。

<TerminalScreen
  who="host"
  command="blugit host ./demo.git"
  :lines="[
    'Sharing C:\\work\\demo.git',
    ['  Device:          HOST-PC [3f9a1c2b7d4e]', 'このPCの端末名とIDです。ゲストの画面にも同じ名前が表示されます'],
    ['  Channel:         7 (automatic)', 'Bluetooth上の接続口の番号です。自動で選ばれるので、普段は気にしなくて大丈夫です'],
    ['  Connection code: 00MX-8433-A77D', '接続コードです。これをゲストに伝えます'],
    '',
    'On the guest, run (you can change the local name):',
    ['  blugit join 00MX-8433-A77D demo', 'ゲストが実行するコマンドです。この行をそのまま送ってもかまいません'],
    '',
    'Press Ctrl+C to stop sharing. Active transfers may finish.',
  ]"
/>

ホストはこの画面のまま、blugit を起動しておきます。

## 3. 接続コードを伝える

<Handoff from="host" to="guest">接続コードを伝える</Handoff>

画面を見せる、チャットで送る、読み上げるなど、伝え方は自由です。
接続コードは「住所」なので、ほかの人に見られても、それだけで登録されることはありません。

12文字の中身は、次のようになっています。

<ConnectionCodeAnatomy />

::: tip 読み上げで伝えるとき
接続コードは大文字・小文字を区別しません。`O` と `0`、`I`・`L` と `1` は同じ文字として扱われ、ハイフンは省略できます。
もし打ち間違えても、12文字目のチェック文字のおかげで、多くの場合は接続する前にエラーで気づけます。
:::

## 4. ゲスト：登録を申請する

ホストから聞いた接続コードと、このPCで使う登録名を指定します。登録は最初の1回だけです。

<AnnotatedCommand
  who="guest"
  command="blugit join {00MX-8433-A77D} {demo}"
  :notes="[
    ['接続コード', 'ホストの画面に出た12文字です。ホストのBluetoothアドレスと接続口を表す「住所」にあたります'],
    ['登録名', 'このPCでホストを呼ぶときの名前です。自分で決めてよく、あとで blugit://demo として使います。英数字・-・_ で32文字まで、先頭は英数字にします'],
  ]"
/>

<TerminalScreen
  who="guest"
  command="blugit join 00MX-8433-A77D demo"
  :lines="[
    ['Registration requested from &quot;HOST-PC&quot;.', '申請先のホストの端末名です'],
    '',
    'Ask the host operator for the code.',
    { text: 'Six-digit code: ', note: 'ここで止まって、6桁の番号の入力を待ちます' },
  ]"
/>

<Handoff from="guest" to="host">申請がBluetoothで届く</Handoff>

<TerminalScreen
  who="host"
  title="ホストのPC（共有中の画面に追加で表示）"
  :lines="[
    ['! Registration requested from &quot;GUEST-PC&quot; [8c21e0f45a9b]', '申請してきた端末名です。目の前のゲストのPCの名前か確かめます'],
    ['  Six-digit code: 482915', '6桁の確認番号です'],
    ['  One attempt. Expires in 180 seconds.', '入力できるのは1回だけで、180秒で無効になります'],
  ]"
/>

<Handoff from="host" to="guest">端末名を確かめてから、6桁の番号を伝える</Handoff>

::: warning 端末名を確かめてから伝える
ホストは、表示された端末名が相手のPCのものか確かめてから番号を伝えてください。対面で確かめるのがおすすめです。
:::

<TerminalScreen
  who="guest"
  :lines="[
    { text: 'Six-digit code: ', input: '482915', note: 'ホストから聞いた番号を入力して Enter を押します' },
    '',
    ['✓ Registered with HOST-PC [3f9a1c2b7d4e] as demo.', 'この表示が出たら登録完了です'],
    '',
    'To clone the repository:',
    ['  git clone blugit://demo', '次に実行するコマンドです'],
  ]"
/>

## 5. ゲスト：clone して作業する

ここからは、いつものGitコマンドで操作できます。接続コードや番号の入力はもう要りません。

<AnnotatedCommand
  who="guest"
  command="git clone {blugit://}{demo} {work}"
  :notes="[
    ['blugit://', 'blugit を通して接続する、という印です'],
    ['登録名', '4で決めた登録名です'],
    ['作業フォルダ', '作られる作業フォルダの名前です'],
  ]"
/>

作ったばかりのリポジトリを clone すると `warning: You appear to have cloned an empty repository.` と表示されますが、問題ありません。

ここから先は、ふつうのGitのリポジトリと同じです。リモートが Bluetooth の先にあるというだけで、commit・push・pull は AI に頼んでかまいません。

<AiChat :messages="[
  ['you', 'work フォルダで作業します。ここまでの変更をコミットして push して'],
  ['ai', '変更をコミットして、リモート（blugit://demo）に push しました'],
]" />

頼み方は[Git講座のまとめ](../git/#summary)を見てください。

::: details 自分でコマンドを打つ場合

```sh
cd work
# ファイルを作って編集してから
git add <ファイル>
git commit -m "<メッセージ>"
git push -u origin main   # 最初の push。次からは git push だけでよい
git pull --ff-only        # 他の人の変更を取り込む
```

:::

## ホスト自身も編集するには

<AnnotatedCommand
  who="host"
  command="git clone {./demo.git} {host-work}"
  :notes="[
    ['共有中のbareリポジトリ', 'blugit host で共有しているフォルダです'],
    ['ホスト用の作業フォルダ', 'ホストはここで編集し、push／pull します'],
  ]"
/>

ホストも、この `host-work` で作業して push／pull します。ゲストの `work` と同じように、AI に頼んでかまいません。

::: tip 操作は1つずつ
同じ接続先へのGit操作は、同時に実行せず1つずつ行ってください。
:::

## 次に読む

- 共有を止める・再開する、ゲストを増やすなど：[毎日の使い方](./daily)
- うまくいかないとき：[困ったとき](./troubleshooting)
