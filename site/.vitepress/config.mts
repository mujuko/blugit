import { defineConfig } from 'vitepress'

const repository = 'https://github.com/mujuko/blugit'

export default defineConfig({
  lang: 'ja',
  title: 'blugit',
  description: 'Wi-Fiがつながらなくても、BluetoothでGitのチーム開発を。',
  base: '/blugit/',
  cleanUrls: true,
  head: [['meta', { name: 'theme-color', content: '#2563eb' }]],

  markdown: {
    container: {
      tipLabel: 'ヒント',
      infoLabel: 'メモ',
      warningLabel: '注意',
      dangerLabel: '危険',
      detailsLabel: 'くわしく',
    },
  },

  themeConfig: {
    nav: [
      { text: 'Git講座', link: '/git/', activeMatch: '^/git/' },
      { text: 'blugit講座', link: '/guide/', activeMatch: '^/guide/$' },
      { text: '資料', link: '/guide/install', activeMatch: '^/guide/.+' },
    ],

    sidebar: {
      '/guide/': [
        {
          text: '講座（スライド）',
          items: [
            { text: '第1部 Git講座', link: '/git/' },
            { text: '第2部 blugit講座', link: '/guide/' },
          ],
        },
        {
          text: '資料',
          items: [
            { text: 'インストール', link: '/guide/install' },
            { text: 'ターミナルの基本', link: '/guide/terminal' },
            { text: 'コマンドリファレンス', link: '/guide/commands' },
            { text: '困ったとき', link: '/guide/troubleshooting' },
            { text: 'しくみ（読み物）', link: '/guide/internals' },
          ],
        },
      ],
    },

    socialLinks: [{ icon: 'github', link: repository }],

    editLink: {
      pattern: `${repository}/edit/main/site/:path`,
      text: 'このページをGitHubで編集',
    },

    search: {
      provider: 'local',
      options: {
        miniSearch: {
          options: {
            // MiniSearch splits on spaces by default, which leaves Japanese
            // sentences as one token. Must stay self-contained: VitePress
            // serializes this function into the client bundle.
            tokenize: (text: string) =>
              Array.from(
                new Intl.Segmenter('ja', { granularity: 'word' }).segment(text),
              )
                .filter((part) => part.isWordLike)
                .map((part) => part.segment),
          },
        },
        translations: {
          button: { buttonText: '検索', buttonAriaLabel: '検索' },
          modal: {
            displayDetails: '詳細を表示',
            resetButtonTitle: '検索をリセット',
            backButtonTitle: '検索を閉じる',
            noResultsText: '見つかりませんでした',
            footer: {
              selectText: '選択',
              navigateText: '移動',
              closeText: '閉じる',
            },
          },
        },
      },
    },

    outline: { level: [2, 3], label: 'このページの内容' },
    docFooter: { prev: '前のページ', next: '次のページ' },
    darkModeSwitchLabel: '表示モード',
    lightModeSwitchTitle: 'ライトモードにする',
    darkModeSwitchTitle: 'ダークモードにする',
    sidebarMenuLabel: 'メニュー',
    returnToTopLabel: 'ページの先頭へ',
    notFound: {
      title: 'ページが見つかりません',
      quote: 'URLが変わったか、ページが移動した可能性があります。',
      linkText: 'トップページへ',
    },
    footer: {
      message: 'Apache License 2.0',
    },
  },
})
