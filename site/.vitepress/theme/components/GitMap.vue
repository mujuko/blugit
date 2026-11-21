<script setup lang="ts">
/**
 * The "map" of Git used throughout the guide: two places, ローカル (your PC) and
 * リモート (the shared place), and the three moves between them.
 *
 *   <GitMap highlight="files record commit" />
 *
 * Keys: files record remote commit push pull.
 * The stage is deliberately not drawn; "add で選んで commit で記録" is one arrow.
 * Wide screens get a left-to-right layout, narrow screens a top-to-bottom one.
 * The blugit guide names the concrete PCs:
 *   <GitMap local-label="ゲストのPC" remote-label="ホストのPC" remote-caption="bareリポジトリ" />
 */
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    highlight?: string
    caption?: string
    localLabel?: string
    remoteLabel?: string
    remoteCaption?: string
  }>(),
  {
    highlight: '',
    caption: 'Gitの地図',
    localLabel: '自分のPC',
    remoteLabel: 'みんなの置き場',
    remoteCaption: 'GitHub など',
  },
)

const lit = computed(() => new Set(props.highlight.split(/\s+/).filter(Boolean)))
const state = (key: string) =>
  lit.value.size === 0 ? '' : lit.value.has(key) ? 'is-on' : 'is-dim'

type Zone = { key: 'local' | 'remote'; x: number; y: number; w: number; h: number }
type Box = { key: string; x: number; y: number; w: number; h: number; dots: boolean }
type Arrow = {
  key: string
  path: string
  head: string
  verb: string
  what: string
  at: [number, number]
  whatAt: [number, number]
  anchor?: 'start' | 'middle' | 'end'
}

const right = (x: number, y: number) => `${x},${y} ${x - 9},${y - 5} ${x - 9},${y + 5}`
const left = (x: number, y: number) => `${x},${y} ${x + 9},${y - 5} ${x + 9},${y + 5}`
const up = (x: number, y: number) => `${x},${y} ${x - 5},${y + 9} ${x + 5},${y + 9}`
const down = (x: number, y: number) => `${x},${y} ${x - 5},${y - 9} ${x + 5},${y - 9}`

const text = computed(() => ({
  files: { title: '作業中のファイル', caption: '編集している途中' },
  record: { title: '記録（履歴）', caption: 'コミットの積み重ね' },
  remote: { title: 'みんなの記録', caption: props.remoteCaption },
}))

const layouts = [
  {
    name: 'wide',
    viewBox: '0 0 720 238',
    zones: [
      { key: 'local', x: 10, y: 12, w: 440, h: 168 },
      { key: 'remote', x: 510, y: 12, w: 200, h: 168 },
    ] as Zone[],
    boxes: [
      { key: 'files', x: 30, y: 58, w: 150, h: 104, dots: false },
      { key: 'record', x: 280, y: 58, w: 150, h: 104, dots: true },
      { key: 'remote', x: 535, y: 58, w: 150, h: 104, dots: true },
    ] as Box[],
    arrows: [
      { key: 'commit', path: 'M182 110 H270', head: right(279, 110), verb: 'add・commit', what: '選んで記録', at: [230, 100], whatAt: [230, 128] },
      { key: 'push', path: 'M432 110 H525', head: right(534, 110), verb: 'push', what: '送る', at: [480, 100], whatAt: [480, 128] },
      { key: 'pull', path: 'M610 164 V208 H355 V173', head: up(355, 164), verb: 'pull', what: '取り込む', at: [482, 200], whatAt: [482, 228] },
    ] as Arrow[],
  },
  {
    name: 'tall',
    viewBox: '0 0 340 596',
    zones: [
      { key: 'local', x: 10, y: 10, w: 320, h: 330 },
      { key: 'remote', x: 10, y: 380, w: 320, h: 200 },
    ] as Zone[],
    boxes: [
      { key: 'files', x: 70, y: 52, w: 200, h: 90, dots: false },
      { key: 'record', x: 70, y: 210, w: 200, h: 112, dots: true },
      { key: 'remote', x: 70, y: 444, w: 200, h: 112, dots: true },
    ] as Box[],
    arrows: [
      { key: 'commit', path: 'M170 144 V200', head: down(170, 209), verb: 'add・commit', what: '選んで記録', at: [182, 172], whatAt: [182, 190], anchor: 'start' },
      { key: 'push', path: 'M170 324 V434', head: down(170, 443), verb: 'push', what: '送る', at: [158, 358], whatAt: [158, 376], anchor: 'end' },
      { key: 'pull', path: 'M272 500 H305 V266 H281', head: left(272, 266), verb: 'pull', what: '取り込む', at: [294, 358], whatAt: [294, 376], anchor: 'end' },
    ] as Arrow[],
  },
]

const zoneTitle = (key: 'local' | 'remote') => (key === 'local' ? 'ローカル' : 'リモート')
const zoneWhere = (key: 'local' | 'remote') => (key === 'local' ? props.localLabel : props.remoteLabel)

// A short row of commits drawn inside a "record" box.
const dots = (box: Box) => {
  const cy = box.y + box.h / 2 + 8
  const cx = box.x + box.w / 2
  return { xs: [cx - 24, cx, cx + 24], cy }
}
</script>

<template>
  <figure class="gm">
    <svg
      v-for="layout in layouts"
      :key="layout.name"
      :class="`gm__svg gm__svg--${layout.name}`"
      :viewBox="layout.viewBox"
      role="img"
      :aria-label="`${caption || 'Gitの地図'}：ローカル（${localLabel}）の作業中のファイルを add・commit で記録し、push でリモート（${remoteLabel}）へ送る。pull でリモートの記録をローカルへ取り込む。`"
    >
      <g v-for="zone in layout.zones" :key="zone.key" class="gm__zone" :class="`gm__zone--${zone.key}`">
        <rect :x="zone.x" :y="zone.y" :width="zone.w" :height="zone.h" rx="14" />
        <text :x="zone.x + 16" :y="zone.y + 26">
          <tspan class="gm__zone-title">{{ zoneTitle(zone.key) }}</tspan>
          <tspan class="gm__zone-where" dx="4">（{{ zoneWhere(zone.key) }}）</tspan>
        </text>
      </g>

      <g v-for="item in layout.boxes" :key="item.key" class="gm__box" :class="state(item.key)">
        <rect :x="item.x" :y="item.y" :width="item.w" :height="item.h" rx="10" />
        <text class="gm__title" :x="item.x + item.w / 2" :y="item.y + (item.dots ? 30 : item.h / 2 - 2)">
          {{ text[item.key as keyof typeof text].title }}
        </text>
        <g v-if="item.dots" class="gm__dots">
          <path :d="`M${dots(item).xs[0]} ${dots(item).cy} H${dots(item).xs[2]}`" />
          <circle v-for="x in dots(item).xs" :key="x" :cx="x" :cy="dots(item).cy" r="6" />
        </g>
        <text class="gm__caption" :x="item.x + item.w / 2" :y="item.y + item.h - (item.dots ? 16 : item.h / 2 - 22)">
          {{ text[item.key as keyof typeof text].caption }}
        </text>
      </g>

      <g v-for="arrow in layout.arrows" :key="arrow.key" class="gm__arrow" :class="state(arrow.key)">
        <path :d="arrow.path" />
        <polygon :points="arrow.head" />
        <text class="gm__verb" :x="arrow.at[0]" :y="arrow.at[1]" :text-anchor="arrow.anchor ?? 'middle'">{{ arrow.verb }}</text>
        <text class="gm__what" :x="arrow.whatAt[0]" :y="arrow.whatAt[1]" :text-anchor="arrow.anchor ?? 'middle'">{{ arrow.what }}</text>
      </g>
    </svg>
    <figcaption v-if="caption">{{ caption }}</figcaption>
  </figure>
</template>

<style scoped>
.gm {
  margin: 24px 0;
}

.gm__svg {
  display: block;
  width: 100%;
  height: auto;
  font-family: var(--vp-font-family-base);
}

.gm__svg--tall {
  display: none;
  max-width: 360px;
  margin: 0 auto;
}

@media (max-width: 560px) {
  .gm__svg--wide {
    display: none;
  }

  .gm__svg--tall {
    display: block;
  }
}

.gm__zone rect {
  stroke-width: 1.5;
  stroke-dasharray: 6 5;
}

.gm__zone--local rect {
  fill: color-mix(in srgb, var(--blugit-host) 5%, transparent);
  stroke: var(--blugit-host);
}

.gm__zone--remote rect {
  fill: color-mix(in srgb, var(--vp-c-brand-1) 6%, transparent);
  stroke: var(--vp-c-brand-1);
}

.gm__zone-title {
  fill: var(--vp-c-text-1);
  font-size: 15px;
  font-weight: 700;
}

.gm__zone-where {
  fill: var(--vp-c-text-2);
  font-size: 13px;
}

.gm__box rect {
  fill: var(--vp-c-bg);
  stroke: var(--vp-c-border);
  stroke-width: 1.5;
}

.gm__box text {
  text-anchor: middle;
}

.gm__title {
  fill: var(--vp-c-text-1);
  font-size: 15px;
  font-weight: 700;
}

.gm__caption {
  fill: var(--vp-c-text-2);
  font-size: 11.5px;
}

.gm__dots path {
  stroke: var(--blugit-lane-0);
  stroke-width: 3;
}

.gm__dots circle {
  fill: var(--blugit-lane-0);
}

.gm__arrow path {
  fill: none;
  stroke: var(--vp-c-text-3);
  stroke-width: 2;
}

.gm__arrow polygon {
  fill: var(--vp-c-text-3);
}

.gm__verb {
  fill: var(--vp-c-text-1);
  font-family: var(--vp-font-family-mono);
  font-size: 12.5px;
  font-weight: 700;
}

.gm__what {
  fill: var(--vp-c-text-2);
  font-size: 11.5px;
}

.gm__box.is-on rect {
  fill: var(--vp-c-brand-soft);
  stroke: var(--vp-c-brand-1);
  stroke-width: 2.5;
}

.gm__arrow.is-on path {
  stroke: var(--vp-c-brand-1);
  stroke-width: 3;
}

.gm__arrow.is-on polygon,
.gm__arrow.is-on .gm__verb {
  fill: var(--vp-c-brand-1);
}

.is-dim {
  opacity: 0.3;
}

figcaption {
  margin-top: 6px;
  color: var(--vp-c-text-2);
  font-size: 0.85rem;
  text-align: center;
}
</style>
