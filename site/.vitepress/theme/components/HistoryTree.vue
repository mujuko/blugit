<script setup lang="ts">
/**
 * An example history that grows from the bottom (old) to the top (new):
 * when, who and what for each commit, a merged branch, a branch still in
 * progress, and version tags. Used next to "Gitは変更の記録係".
 *   <HistoryTree />
 */
type Row = {
  lane: 'main' | 'a' | 'b'
  when: string
  who: string
  message: string
  tag?: string
}

// Bottom row first. Same report as the "最終版_本当に最終" example.
const rows: Row[] = [
  { lane: 'main', when: '4/1 10:12', who: '佐藤', message: '最初の版', tag: 'v1.0' },
  { lane: 'main', when: '4/2 09:40', who: '佐藤', message: '表紙を追加' },
  { lane: 'a', when: '4/3 11:05', who: '鈴木', message: 'グラフを追加' },
  { lane: 'main', when: '4/3 15:30', who: '田中', message: '誤字を修正' },
  { lane: 'a', when: '4/4 13:20', who: '鈴木', message: 'グラフの色を修正' },
  { lane: 'b', when: '4/5 10:00', who: '田中', message: '第3章の下書き' },
  { lane: 'main', when: '4/5 16:45', who: '佐藤', message: 'グラフを取り込み', tag: 'v1.1' },
  { lane: 'b', when: '4/6 14:10', who: '田中', message: '第3章を加筆' },
  { lane: 'main', when: '4/7 17:25', who: '鈴木', message: 'まとめを追加', tag: 'v2.0' },
]

const LANE_X = { b: 30, main: 70, a: 110 } as const
const WHEN_X = 152
const WHO_X = 232
const MESSAGE_X = 272
const MESSAGE_SIZE = 15
const rowY = (i: number) => 470 - i * 54
const at = (i: number) => ({ x: LANE_X[rows[i].lane], y: rowY(i) })

// Rough width of a message, so its tag can sit right after it.
const messageWidth = (text: string) =>
  [...text].reduce((w, ch) => w + (ch.charCodeAt(0) > 0xff ? MESSAGE_SIZE : MESSAGE_SIZE * 0.6), 0)

// Smooth curve between two commits on different lanes.
const curve = (from: number, to: number) => {
  const a = at(from)
  const b = at(to)
  const midY = (a.y + b.y) / 2
  return `M${a.x} ${a.y} C${a.x} ${midY} ${b.x} ${midY} ${b.x} ${b.y}`
}
const straight = (from: number, to: number) => {
  const a = at(from)
  const b = at(to)
  return `M${a.x} ${a.y} L${b.x} ${b.y}`
}

const edges = [
  { lane: 'main', d: `M${LANE_X.main} ${rowY(0)} V${rowY(8)}` },
  { lane: 'a', d: curve(1, 2) },
  { lane: 'a', d: straight(2, 4) },
  { lane: 'a', d: curve(4, 6) },
  { lane: 'b', d: curve(3, 5) },
  { lane: 'b', d: straight(5, 7) },
]
</script>

<template>
  <figure class="ht">
    <svg
      viewBox="0 0 460 500"
      role="img"
      aria-label="レポートの履歴の例。下から上へ新しくなる。各記録に日時・名前・内容があり、v1.0・v1.1・v2.0 の印が付いている。鈴木のグラフの枝は佐藤が取り込み済み、田中の第3章の枝はまだ作業中。"
    >
      <!-- The in-progress branch keeps going up. -->
      <path class="ht__edge ht__tail lane-b" :d="`M${LANE_X.b} ${rowY(7)} V${rowY(8) - 16}`" />

      <path v-for="(edge, i) in edges" :key="i" class="ht__edge" :class="`lane-${edge.lane}`" :d="edge.d" />

      <g v-for="(row, i) in rows" :key="i" class="ht__row">
        <path class="ht__leader" :d="`M${at(i).x + 13} ${at(i).y} H${WHEN_X - 8}`" />
        <circle class="ht__dot" :class="`lane-${row.lane}`" :cx="at(i).x" :cy="at(i).y" r="9" />
        <text class="ht__when" :x="WHEN_X" :y="at(i).y + 5">{{ row.when }}</text>
        <text class="ht__who" :x="WHO_X" :y="at(i).y + 5">{{ row.who }}</text>
        <text class="ht__message" :x="MESSAGE_X" :y="at(i).y + 5">{{ row.message }}</text>
        <g v-if="row.tag" class="ht__tag">
          <rect :x="MESSAGE_X + messageWidth(row.message) + 8" :y="at(i).y - 11" width="46" height="22" rx="11" />
          <text :x="MESSAGE_X + messageWidth(row.message) + 31" :y="at(i).y + 5">{{ row.tag }}</text>
        </g>
      </g>

      <text class="ht__note lane-b" :x="LANE_X.b" :y="rowY(8) - 24" text-anchor="middle">作業中</text>
    </svg>
  </figure>
</template>

<style scoped>
.ht {
  margin: 0;
}

svg {
  display: block;
  width: 100%;
  max-width: 460px;
  height: auto;
  margin: 0 auto;
  font-family: var(--vp-font-family-base);
}

.lane-main { --lane: var(--blugit-lane-0); }
.lane-a { --lane: var(--blugit-lane-1); }
.lane-b { --lane: var(--blugit-lane-2); }

.ht__edge {
  fill: none;
  stroke: var(--lane);
  stroke-width: 4;
  stroke-linecap: round;
}

.ht__tail {
  stroke-dasharray: 1 9;
  stroke-width: 5;
}

.ht__leader {
  stroke: var(--vp-c-text-3);
  stroke-width: 1.5;
  stroke-dasharray: 2 4;
}

.ht__dot {
  fill: var(--lane);
  stroke: var(--vp-c-bg);
  stroke-width: 3;
}

.ht__when {
  fill: var(--vp-c-text-2);
  font-family: var(--vp-font-family-mono);
  font-size: 12.5px;
}

.ht__who {
  fill: var(--vp-c-text-2);
  font-size: 13px;
}

.ht__message {
  fill: var(--vp-c-text-1);
  font-size: 15px;
  font-weight: 600;
}

.ht__tag rect {
  fill: var(--vp-c-bg);
  stroke: var(--vp-c-brand-1);
  stroke-width: 1.5;
}

.ht__tag text {
  fill: var(--vp-c-brand-1);
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  font-weight: 700;
  text-anchor: middle;
}

.ht__note {
  fill: var(--lane);
  font-size: 11px;
  font-weight: 700;
}
</style>
