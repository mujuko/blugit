<script setup lang="ts">
/**
 * A small commit graph that reads left (older) to right (newer).
 *
 *   <CommitGraph
 *     :commits="[
 *       { id: 'A' }, { id: 'B' },
 *       { id: 'C', lane: 1 }, { id: 'D', lane: 1 },
 *       { id: 'M', parents: ['B', 'D'] },
 *     ]"
 *     :branches="{ main: 'M', idea: 'D' }"
 *     head="main"
 *     caption="idea を main に合流させた"
 *   />
 *
 * Each commit gets its own column in array order. `lane` 0 is the top row.
 * Without `parents`, a commit follows the previous commit in its lane, or,
 * if it is the first in its lane, the previous commit overall (the fork point).
 * A branch can be { at, from } to draw its label moving from one commit to another.
 * Names containing "/" (origin/main) are drawn as remote-tracking labels.
 */
import { computed } from 'vue'

type Commit = { id: string; lane?: number; parents?: string[]; dim?: boolean }
type Branch = string | { at: string; from?: string }

const props = defineProps<{
  commits: Commit[]
  branches?: Record<string, Branch>
  head?: string
  caption?: string
  /** Draw larger than natural size, e.g. on a slide. */
  scale?: number
  /** Show a "古い → 新しい" axis under the graph: left-to-right is this figure's convention, not Git's. */
  axis?: boolean
  /** Keep the main line going past its last commit, so branches read as leaving a trunk. */
  trunk?: boolean
}>()

const COL = 58
const LANE = 62
const PAD_X = 34
const LABEL_H = 22
const LABEL_GAP = 4
const R = 12

const nodes = computed(() => {
  const lastInLane = new Map<number, string>()
  return props.commits.map((commit, index) => {
    const lane = commit.lane ?? 0
    const previous = index > 0 ? props.commits[index - 1].id : undefined
    const parents = commit.parents ?? [lastInLane.get(lane) ?? previous].filter((p): p is string => !!p)
    lastInLane.set(lane, commit.id)
    return { ...commit, lane, parents, column: index }
  })
})

const byId = computed(() => new Map(nodes.value.map((node) => [node.id, node])))

type Label = { name: string; commit: string; ghost: boolean; head: boolean; remote: boolean }

const labels = computed<Label[]>(() => {
  const result: Label[] = []
  for (const [name, value] of Object.entries(props.branches ?? {})) {
    const at = typeof value === 'string' ? value : value.at
    const from = typeof value === 'string' ? undefined : value.from
    const flags = { head: props.head === name, remote: name.includes('/') }
    if (from) result.push({ name, commit: from, ghost: true, ...flags, head: false })
    result.push({ name, commit: at, ghost: false, ...flags })
  }
  return result
})

const lanes = computed(() => Math.max(0, ...nodes.value.map((node) => node.lane)) + 1)

// Labels go above lane-0 commits and below the others; several on one commit stack up.
const stacks = computed(() => {
  const count = new Map<string, number>()
  return labels.value.map((label) => {
    const k = count.get(label.commit) ?? 0
    count.set(label.commit, k + 1)
    return k
  })
})

const maxStack = (above: boolean) =>
  Math.max(
    0,
    ...labels.value.map((label, i) => {
      const lane = byId.value.get(label.commit)?.lane ?? 0
      return (lane === 0) === above ? stacks.value[i] + 1 : 0
    }),
  )

const texts = computed(() =>
  labels.value.map((label) => {
    const text = label.head ? `HEAD → ${label.name}` : label.name
    return { text, w: Math.max(40, measure(text) + 18) }
  }),
)

// Widen the columns when two labels share a row, so pills (and the arrow between a
// moved label and its old spot) never overlap.
const col = computed(() => {
  let needed = COL
  labels.value.forEach((a, i) => {
    labels.value.forEach((b, j) => {
      const na = byId.value.get(a.commit)
      const nb = byId.value.get(b.commit)
      if (!na || !nb || na.column >= nb.column) return
      // Labels sit above lane 0 and below other lanes, so a shared row means a shared lane and stack level.
      if (na.lane !== nb.lane || stacks.value[i] !== stacks.value[j]) return
      const gap = a.ghost || b.ghost ? 26 : 8
      needed = Math.max(needed, ((texts.value[i].w + texts.value[j].w) / 2 + gap) / (nb.column - na.column))
    })
  })
  return Math.ceil(needed)
})

const AXIS_H = 44

const top = computed(() => 16 + maxStack(true) * (LABEL_H + LABEL_GAP))
const graphBottom = computed(
  () => top.value + (lanes.value - 1) * LANE + R + 16 + maxStack(false) * (LABEL_H + LABEL_GAP),
)
const height = computed(() => graphBottom.value + (props.axis ? AXIS_H : 0))

const x = (column: number) => PAD_X + column * col.value
const y = (lane: number) => top.value + R + 4 + lane * LANE

const lastColumn = computed(() => Math.max(0, nodes.value.length - 1))

// The main line: solid up to the last column, then a short dashed tail.
const trunkLine = computed(() => {
  if (!props.trunk) return undefined
  const mains = nodes.value.filter((node) => node.lane === 0)
  if (!mains.length) return undefined
  const from = x(mains[mains.length - 1].column)
  const solidTo = x(lastColumn.value)
  const tailTo = solidTo + col.value * 0.6
  const yy = y(0)
  return {
    solid: solidTo > from ? `M${from} ${yy} H${solidTo}` : undefined,
    tail: `M${Math.max(from, solidTo)} ${yy} H${tailTo}`,
    end: tailTo,
  }
})

const rightEdge = computed(() => Math.max(x(lastColumn.value), trunkLine.value?.end ?? 0))

const axisLine = computed(() => {
  if (!props.axis) return undefined
  const yy = graphBottom.value + 12
  const x1 = x(0) - 12
  const x2 = rightEdge.value + 4
  return {
    d: `M${x1} ${yy} H${x2 - 8}`,
    head: `${x2},${yy} ${x2 - 9},${yy - 5} ${x2 - 9},${yy + 5}`,
    x1,
    x2,
    textY: yy + 22,
  }
})

const edges = computed(() =>
  nodes.value.flatMap((node) =>
    node.parents
      .map((id) => byId.value.get(id))
      .filter((parent) => parent !== undefined)
      .map((parent) => {
        const x1 = x(parent.column)
        const y1 = y(parent.lane)
        const x2 = x(node.column)
        const y2 = y(node.lane)
        const mid = (x1 + x2) / 2
        const d =
          y1 === y2 ? `M${x1} ${y1} H${x2}` : `M${x1} ${y1} C${mid} ${y1} ${mid} ${y2} ${x2} ${y2}`
        return { key: `${parent.id}-${node.id}`, d, lane: Math.max(parent.lane, node.lane), dim: node.dim }
      }),
  ),
)

const placed = computed(() =>
  labels.value.map((label, i) => {
    const node = byId.value.get(label.commit)
    const lane = node?.lane ?? 0
    const cx = x(node?.column ?? 0)
    const cy = y(lane)
    const step = (LABEL_H + LABEL_GAP) * stacks.value[i]
    const { text, w } = texts.value[i]
    const ly = lane === 0 ? cy - R - 8 - LABEL_H - step : cy + R + 8 + step
    return { ...label, text, x: cx - w / 2, y: ly, w, cx }
  }),
)

// Fit the drawing to its widest parts, so labels at either end are never cut off.
const bounds = computed(() => {
  const left = Math.min(0, ...placed.value.map((label) => label.x - 4))
  const right = Math.max(
    PAD_X * 2 + Math.max(0, nodes.value.length - 1) * col.value,
    rightEdge.value + 12,
    ...placed.value.map((label) => label.x + label.w + 4),
  )
  return { left, width: right - left }
})

// Moving labels: a dashed arrow from the old position to the new one.
const moves = computed(() =>
  placed.value
    .filter((label) => label.ghost)
    .map((ghost) => {
      const target = placed.value.find((label) => !label.ghost && label.name === ghost.name)
      if (!target) return undefined
      const yy = ghost.y + LABEL_H / 2
      const x1 = ghost.x + ghost.w + 2
      const x2 = target.x - 4
      return { key: ghost.name, d: `M${x1} ${yy} H${x2 - 6}`, head: `${x2},${yy} ${x2 - 7},${yy - 4} ${x2 - 7},${yy + 4}` }
    })
    .filter((move) => move !== undefined),
)

// Rough text width: full-width characters count double. Good enough for pill sizing.
function measure(text: string) {
  let width = 0
  for (const ch of text) width += ch.charCodeAt(0) > 0xff ? 12 : 7
  return width
}

const description = computed(() => {
  const order = nodes.value.map((node) => node.id).join('、')
  const names = placed.value
    .filter((label) => !label.ghost)
    .map((label) => `${label.text} は ${label.commit}`)
    .join('、')
  return `コミットグラフ：左から順に ${order}。${names}。`
})
</script>

<template>
  <figure class="cg">
    <div class="cg__scroll">
      <svg
        class="cg__svg"
        :viewBox="`${bounds.left} 0 ${bounds.width} ${height}`"
        :width="bounds.width * (scale ?? 1)"
        :height="height * (scale ?? 1)"
        role="img"
        :aria-label="description"
      >
        <g v-if="trunkLine" class="cg__trunk lane-0">
          <path v-if="trunkLine.solid" :d="trunkLine.solid" class="cg__edge" />
          <path :d="trunkLine.tail" class="cg__edge cg__edge--tail" />
        </g>
        <g v-if="axisLine" class="cg__axis">
          <path :d="axisLine.d" />
          <polygon :points="axisLine.head" />
          <text :x="axisLine.x1" :y="axisLine.textY" text-anchor="start">古い</text>
          <text :x="axisLine.x2" :y="axisLine.textY" text-anchor="end">新しい</text>
        </g>
        <path
          v-for="edge in edges"
          :key="edge.key"
          :d="edge.d"
          class="cg__edge"
          :class="[`lane-${edge.lane % 3}`, { 'is-dim': edge.dim }]"
        />
        <g v-for="move in moves" :key="move.key" class="cg__move">
          <path :d="move.d" />
          <polygon :points="move.head" />
        </g>
        <g
          v-for="node in nodes"
          :key="node.id"
          class="cg__node"
          :class="[`lane-${node.lane % 3}`, { 'is-dim': node.dim }]"
        >
          <circle :cx="x(node.column)" :cy="y(node.lane)" :r="R" />
          <text :x="x(node.column)" :y="y(node.lane) + 4">{{ node.id }}</text>
        </g>
        <g
          v-for="label in placed"
          :key="`${label.name}-${label.commit}-${label.ghost}`"
          class="cg__label"
          :class="{ 'is-ghost': label.ghost, 'is-head': label.head, 'is-remote': label.remote }"
        >
          <rect :x="label.x" :y="label.y" :width="label.w" :height="LABEL_H" rx="11" />
          <text :x="label.cx" :y="label.y + 15">{{ label.text }}</text>
        </g>
      </svg>
    </div>
    <figcaption v-if="caption">{{ caption }}</figcaption>
  </figure>
</template>

<style scoped>
.cg {
  margin: 24px 0;
}

.cg__scroll {
  overflow-x: auto;
}

.cg__svg {
  display: block;
  margin: 0 auto;
  max-width: 100%;
  height: auto;
  font-family: var(--vp-font-family-base);
}

.lane-0 { --lane: var(--blugit-lane-0); }
.lane-1 { --lane: var(--blugit-lane-1); }
.lane-2 { --lane: var(--blugit-lane-2); }

.cg__edge {
  fill: none;
  stroke: var(--lane);
  stroke-width: 4;
  stroke-linecap: round;
}

.cg__edge--tail {
  stroke-dasharray: 2 8;
}

.cg__axis path {
  fill: none;
  stroke: var(--vp-c-text-3);
  stroke-width: 1.5;
}

.cg__axis polygon {
  fill: var(--vp-c-text-3);
}

.cg__axis text {
  fill: var(--vp-c-text-2);
  font-size: 12px;
}

.cg__node circle {
  fill: var(--lane);
  stroke: var(--vp-c-bg);
  stroke-width: 3;
}

.cg__node text {
  fill: #fff;
  font-size: 12px;
  font-weight: 700;
  text-anchor: middle;
}

.is-dim {
  opacity: 0.3;
}

.cg__label rect {
  fill: var(--vp-c-bg);
  stroke: var(--vp-c-text-2);
  stroke-width: 1.5;
}

.cg__label text {
  fill: var(--vp-c-text-1);
  font-size: 12px;
  font-weight: 600;
  text-anchor: middle;
}

.cg__label.is-head rect {
  fill: var(--vp-c-text-1);
  stroke: var(--vp-c-text-1);
}

.cg__label.is-head text {
  fill: var(--vp-c-bg);
}

.cg__label.is-remote rect {
  stroke-dasharray: 4 3;
}

.cg__label.is-ghost {
  opacity: 0.4;
}

.cg__label.is-ghost rect {
  stroke-dasharray: 3 3;
}

.cg__move path {
  fill: none;
  stroke: var(--vp-c-text-3);
  stroke-width: 1.5;
  stroke-dasharray: 4 3;
}

.cg__move polygon {
  fill: var(--vp-c-text-3);
}

figcaption {
  margin-top: 6px;
  color: var(--vp-c-text-2);
  font-size: 0.85rem;
  text-align: center;
}
</style>
