<script setup lang="ts">
/**
 * The motivating example: two PCs on a Wi-Fi that blocks device-to-device traffic.
 *   <WifiScenario variant="before" />  — only the employee PC reaches GitHub; the PCs cannot reach each other
 *   <WifiScenario variant="after" />   — blugit links them over Bluetooth
 *
 * Every Wi-Fi route is an arrow that bends through the access point, so "via Wi-Fi" reads at a glance.
 */
import { computed } from 'vue'

const props = withDefaults(defineProps<{ variant?: 'before' | 'after' }>(), { variant: 'before' })
const after = computed(() => props.variant === 'after')

const label = computed(() =>
  after.value
    ? '社員PCとゲストPCは、Wi-Fiを経由しては通信できないが、Bluetoothで直接つながっている。社員PCがホスト、ゲストPCがゲスト。'
    : '社員PCとゲストPCが同じ社内Wi-Fiにつながっている。社員PCはWi-Fiを経由して社内GitHubを使えるが、ゲストPCは使えない。Wi-Fiを経由したPC同士の通信もできない。',
)

type Point = readonly [number, number]

const HEAD = 11

/** A filled arrowhead at `tip`, pointing away from `from`. The line should stop at `base`. */
function head(tip: Point, from: Point) {
  const length = Math.hypot(tip[0] - from[0], tip[1] - from[1])
  const ux = (tip[0] - from[0]) / length
  const uy = (tip[1] - from[1]) / length
  const bx = tip[0] - ux * HEAD
  const by = tip[1] - uy * HEAD
  const w = HEAD * 0.5
  const corners: Point[] = [tip, [bx - uy * w, by + ux * w], [bx + uy * w, by - ux * w]]
  return {
    base: `${bx.toFixed(1)} ${by.toFixed(1)}`,
    points: corners.map((p) => p.map((n) => n.toFixed(1)).join(',')).join(' '),
  }
}

// Employee PC → Wi-Fi → GitHub, and its mirror image for the guest PC.
const toGithubFromEmployee = head([196, 57], [196, 92])
const toGithubFromGuest = head([244, 57], [244, 92])
const githubRoutes = {
  employee: `M100 222 C140 200 184 184 186 150 C188 116 196 92 ${toGithubFromEmployee.base}`,
  guest: `M340 222 C300 200 256 184 254 150 C252 116 244 92 ${toGithubFromGuest.base}`,
}

// Employee PC ↔ Wi-Fi ↔ guest PC: an arch that dips under the access point.
const toEmployee = head([152, 222], [176, 178])
const toGuest = head([288, 222], [264, 178])
const peerRoute = `M${toEmployee.base} C176 178 264 178 ${toGuest.base}`

// Bluetooth: straight between the two PCs.
const btToEmployee = head([164, 248], [276, 248])
const btToGuest = head([276, 248], [164, 248])
const btRoute = `M${btToEmployee.base} L${btToGuest.base}`

// The usual Wi-Fi symbol: a dot with three arcs fanning out above it.
const WIFI = { x: 220, y: 162 }
const wifiArcs = [9, 16.5, 24]
  .map((r) => {
    const k = r * Math.SQRT1_2
    const y = (WIFI.y - k).toFixed(2)
    return `M${(WIFI.x - k).toFixed(2)} ${y} A${r} ${r} 0 0 1 ${(WIFI.x + k).toFixed(2)} ${y}`
  })
  .join(' ')
</script>

<template>
  <figure class="ws" :class="{ 'is-after': after }">
    <svg viewBox="0 0 440 322" role="img" :aria-label="label">
      <!-- Internal GitHub -->
      <g class="ws__node">
        <rect x="140" y="10" width="160" height="46" rx="10" />
        <text class="ws__name" x="220" y="38">社内GitHub</text>
      </g>

      <!-- Access point -->
      <circle class="ws__halo" cx="220" cy="150" r="46" />
      <g class="ws__icon">
        <path :d="wifiArcs" />
        <circle :cx="WIFI.x" :cy="WIFI.y" r="3.6" />
      </g>
      <text class="ws__small ws__start" x="272" y="155">社内Wi-Fi</text>

      <!-- PCs -->
      <g class="ws__pc" :class="{ 'is-host': after }">
        <rect x="12" y="222" width="150" height="52" rx="10" />
        <text class="ws__name" x="87" y="253">{{ after ? '社員PC（ホスト）' : '社員PC' }}</text>
      </g>
      <g class="ws__pc" :class="{ 'is-guest': after }">
        <rect x="278" y="222" width="150" height="52" rx="10" />
        <text class="ws__name" x="353" y="253">{{ after ? 'ゲストPC（ゲスト）' : 'ゲストPC' }}</text>
      </g>

      <!-- Routes through the Wi-Fi -->
      <g class="ws__routes">
        <path class="ws__route is-ok" :d="githubRoutes.employee" />
        <polygon class="ws__head is-ok" :points="toGithubFromEmployee.points" />
        <text class="ws__label is-ok ws__end" x="182" y="109">使える</text>

        <path class="ws__route is-ng" :d="githubRoutes.guest" />
        <polygon class="ws__head is-ng" :points="toGithubFromGuest.points" />
        <g class="ws__badge" transform="translate(248 105)">
          <circle r="11" />
          <path d="M-5 -5 L5 5 M5 -5 L-5 5" />
        </g>
        <text class="ws__label is-ng ws__start" x="265" y="109">使えない</text>

        <path class="ws__route is-ng" :d="peerRoute" />
        <polygon class="ws__head is-ng" :points="toEmployee.points" />
        <polygon class="ws__head is-ng" :points="toGuest.points" />
        <g class="ws__badge" transform="translate(220 189)">
          <circle r="11" />
          <path d="M-5 -5 L5 5 M5 -5 L-5 5" />
        </g>
        <text class="ws__label is-ng" x="220" y="216">通信できない</text>
      </g>

      <text v-if="!after" class="ws__note" x="220" y="306">同じWi-Fiでも、PC同士は直接つながらない</text>
      <template v-else>
        <path class="ws__bt" :d="btRoute" />
        <polygon class="ws__bt-head" :points="btToEmployee.points" />
        <polygon class="ws__bt-head" :points="btToGuest.points" />
        <text class="ws__bt-label" x="220" y="240">Bluetooth</text>
        <text class="ws__note" x="220" y="306">blugitで、PC同士がBluetoothで直接つながる</text>
      </template>
    </svg>
  </figure>
</template>

<style scoped>
.ws {
  margin: 24px 0;
}

svg {
  display: block;
  width: 100%;
  max-width: 520px;
  height: auto;
  margin: 0 auto;
  font-family: var(--vp-font-family-base);
}

text {
  text-anchor: middle;
  fill: var(--vp-c-text-1);
}

.ws__name {
  font-size: 14px;
  font-weight: 700;
}

.ws__small {
  font-size: 12px;
  fill: var(--vp-c-text-2);
}

/* CSS wins over the text-anchor attribute, so alignment goes through classes. */
.ws__start {
  text-anchor: start;
}

.ws__end {
  text-anchor: end;
}

.ws__note {
  font-size: 13px;
  font-weight: 600;
}

.ws__node rect,
.ws__pc rect {
  fill: var(--vp-c-bg-soft);
  stroke: var(--vp-c-border);
  stroke-width: 1.5;
}

.ws__pc.is-host rect {
  fill: var(--blugit-host-soft);
  stroke: var(--blugit-host);
  stroke-width: 2.5;
}

.ws__pc.is-guest rect {
  fill: var(--blugit-guest-soft);
  stroke: var(--blugit-guest);
  stroke-width: 2.5;
}

.ws__halo {
  fill: var(--vp-c-default-soft);
}

.ws__icon path {
  fill: none;
  stroke: var(--vp-c-text-1);
  stroke-width: 4;
  stroke-linecap: round;
}

.ws__icon circle {
  fill: var(--vp-c-text-1);
}

.ws__route {
  fill: none;
  stroke-width: 3;
}

.ws__route.is-ok {
  stroke: var(--blugit-ok);
}

.ws__route.is-ng {
  stroke: var(--blugit-ng);
  stroke-dasharray: 7 5;
}

.ws__head.is-ok {
  fill: var(--blugit-ok);
}

.ws__head.is-ng {
  fill: var(--blugit-ng);
}

.ws__label {
  font-size: 12px;
  font-weight: 700;
}

.ws__label.is-ok {
  fill: var(--blugit-ok);
}

.ws__label.is-ng {
  fill: var(--blugit-ng);
}

.ws__badge circle {
  fill: var(--vp-c-bg);
  stroke: var(--blugit-ng);
  stroke-width: 2;
}

.ws__badge path {
  stroke: var(--blugit-ng);
  stroke-width: 2.5;
  stroke-linecap: round;
}

.ws__bt {
  stroke: var(--vp-c-brand-1);
  stroke-width: 5;
}

.ws__bt-head {
  fill: var(--vp-c-brand-1);
}

.ws__bt-label {
  fill: var(--vp-c-brand-1);
  font-size: 12px;
  font-weight: 700;
}

.is-after .ws__routes {
  opacity: 0.3;
}
</style>
