import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import AiChat from './components/AiChat.vue'
import AnnotatedCommand from './components/AnnotatedCommand.vue'
import CommitGraph from './components/CommitGraph.vue'
import ConnectionCodeAnatomy from './components/ConnectionCodeAnatomy.vue'
import GitMap from './components/GitMap.vue'
import Handoff from './components/Handoff.vue'
import HistoryTree from './components/HistoryTree.vue'
import Role from './components/Role.vue'
import ShareFigure from './components/ShareFigure.vue'
import Slide from './components/Slide.vue'
import SlideDeck from './components/SlideDeck.vue'
import TerminalScreen from './components/TerminalScreen.vue'
import WifiScenario from './components/WifiScenario.vue'
import './custom.css'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('AiChat', AiChat)
    app.component('AnnotatedCommand', AnnotatedCommand)
    app.component('CommitGraph', CommitGraph)
    app.component('ConnectionCodeAnatomy', ConnectionCodeAnatomy)
    app.component('GitMap', GitMap)
    app.component('Handoff', Handoff)
    app.component('HistoryTree', HistoryTree)
    app.component('Role', Role)
    app.component('ShareFigure', ShareFigure)
    app.component('Slide', Slide)
    app.component('SlideDeck', SlideDeck)
    app.component('TerminalScreen', TerminalScreen)
    app.component('WifiScenario', WifiScenario)
  },
} satisfies Theme
