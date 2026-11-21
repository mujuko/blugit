<script setup lang="ts">
/**
 * The motivating example: two PCs on a Wi-Fi that blocks device-to-device traffic.
 *   <WifiScenario variant="before" />  — they cannot reach each other
 *   <WifiScenario variant="after" />   — blugit links them over Bluetooth
 */
import { computed } from 'vue'

const props = withDefaults(defineProps<{ variant?: 'before' | 'after' }>(), { variant: 'before' })
const after = computed(() => props.variant === 'after')

const label = computed(() =>
  after.value
    ? '社員PCとゲストPCが同じWi-Fiにつながっている。Wi-Fiでは直接通信できないが、Bluetoothでblugitを使ってつながっている。社員PCがホスト、ゲストPCがゲスト。'
    : '社員PCとゲストPCが同じWi-Fiにつながっている。社員PCは社内GitHubを使えるが、ゲストPCは使えない。2台のPCは直接通信できない。',
)
</script>

<template>
  <figure class="ws" :class="{ 'is-after': after }">
    <svg viewBox="0 0 440 330" role="img" :aria-label="label">
      <!-- Wi-Fi links -->
      <path class="ws__wifi" d="M87 216 L203 146" />
      <path class="ws__wifi" d="M353 216 L237 146" />
      <path class="ws__wifi" d="M220 106 V62" />

      <!-- Internal GitHub -->
      <g class="ws__node">
        <rect x="145" y="12" width="150" height="48" rx="10" />
        <text class="ws__name" x="220" y="42">社内GitHub</text>
      </g>

      <!-- Access point -->
      <g class="ws__ap">
        <circle cx="220" cy="128" r="22" />
        <path d="M209 128 a11 11 0 0 1 22 0 M213 128 a7 7 0 0 1 14 0" />
        <circle class="ws__dot" cx="220" cy="131" r="2.5" />
        <text class="ws__small" x="252" y="133">社内Wi-Fi</text>
      </g>

      <!-- PCs -->
      <g class="ws__pc" :class="{ 'is-host': after }">
        <rect x="12" y="216" width="150" height="68" rx="10" />
        <text class="ws__name" x="87" y="243">{{ after ? '社員PC（ホスト）' : '社員PC' }}</text>
        <text class="ws__small" x="87" y="267">社内GitHub <tspan class="ws__ok">○ 使える</tspan></text>
      </g>
      <g class="ws__pc" :class="{ 'is-guest': after }">
        <rect x="278" y="216" width="150" height="68" rx="10" />
        <text class="ws__name" x="353" y="243">{{ after ? 'ゲストPC（ゲスト）' : 'ゲストPC' }}</text>
        <text class="ws__small" x="353" y="267">社内GitHub <tspan class="ws__ng">× 使えない</tspan></text>
      </g>

      <!-- Device to device -->
      <template v-if="!after">
        <path class="ws__blocked" d="M166 250 H274" />
        <circle class="ws__badge-ng" cx="220" cy="250" r="12" />
        <path class="ws__cross" d="M214 244 L226 256 M226 244 L214 256" />
        <text class="ws__note" x="220" y="314">同じWi-Fiでも、PC同士は直接つながらない</text>
      </template>
      <template v-else>
        <path class="ws__bt" d="M166 250 H274" />
        <text class="ws__bt-label" x="220" y="238">Bluetooth</text>
        <text class="ws__note" x="220" y="314">blugitで、PC同士がBluetoothで直接つながる</text>
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

.ws__ap .ws__small {
  text-anchor: start;
}

.ws__note {
  font-size: 13px;
  font-weight: 600;
}

.ws__ok {
  fill: var(--blugit-ok);
  font-weight: 700;
}

.ws__ng {
  fill: var(--blugit-ng);
  font-weight: 700;
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

.ws__wifi {
  fill: none;
  stroke: var(--vp-c-text-3);
  stroke-width: 2;
  stroke-dasharray: 2 5;
  stroke-linecap: round;
}

.ws__ap circle:first-child {
  fill: var(--vp-c-bg);
  stroke: var(--vp-c-text-3);
  stroke-width: 1.5;
}

.ws__ap path {
  fill: none;
  stroke: var(--vp-c-text-2);
  stroke-width: 2;
  stroke-linecap: round;
}

.ws__dot {
  fill: var(--vp-c-text-2);
}

.ws__blocked {
  stroke: var(--blugit-ng);
  stroke-width: 2;
  stroke-dasharray: 5 5;
}

.ws__badge-ng {
  fill: var(--vp-c-bg);
  stroke: var(--blugit-ng);
  stroke-width: 2;
}

.ws__cross {
  stroke: var(--blugit-ng);
  stroke-width: 2.5;
  stroke-linecap: round;
}

.ws__bt {
  stroke: var(--vp-c-brand-1);
  stroke-width: 5;
  stroke-linecap: round;
}

.ws__bt-label {
  fill: var(--vp-c-brand-1);
  font-size: 12px;
  font-weight: 700;
}

.is-after .ws__wifi {
  opacity: 0.5;
}
</style>
