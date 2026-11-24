<script setup lang="ts">
/**
 * Breaks a connection code into its parts:
 * characters 1–10 carry the host's Bluetooth address, the 11th the channel,
 * and the 12th is a check character (see src/connection_code.rs).
 */
import { computed, ref } from 'vue'

const props = withDefaults(defineProps<{ code?: string }>(), { code: '00MX-8433-A77D' })

const chars = computed(() => props.code.replace(/-/g, '').split(''))

const groups = computed(() => {
  const c = chars.value
  return [
    { parts: [c.slice(0, 4), c.slice(4, 8), c.slice(8, 10)], label: 'ホストのBluetoothアドレス', note: '最初の10文字。ホストのPCのBluetoothの「住所」です。同じPCなら変わりません' },
    { parts: [c.slice(10, 11)], label: '接続口の番号（チャネル）', note: `11文字目。この例の「${c[10]}」は${channel(c[10])}番です。ホストを起動し直すと変わることがあります` },
    { parts: [c.slice(11, 12)], label: 'チェック文字', note: '12文字目。前の11文字から計算した1文字で、打ち間違いがあると接続する前に気づけます' },
  ]
})

function channel(ch: string) {
  return '0123456789ABCDEFGHJKMNPQRSTVWXYZ'.indexOf(ch.toUpperCase())
}

const active = ref<number | null>(null)
</script>

<template>
  <figure class="cc">
    <div class="cc__code" role="img" :aria-label="`接続コード ${code} の内訳`">
      <span
        v-for="(group, g) in groups"
        :key="g"
        class="cc__group"
        :class="[`n${g + 1}`, { 'is-active': active === g }]"
        @mouseenter="active = g"
        @mouseleave="active = null"
      >
        <template v-for="(part, p) in group.parts" :key="p">
          <span v-if="p > 0" class="cc__dash">-</span>
          <span v-for="(ch, i) in part" :key="i" class="cc__tile">{{ ch }}</span>
        </template>
        <span class="cc__badge" aria-hidden="true">{{ g + 1 }}</span>
      </span>
    </div>
    <ol class="cc__notes">
      <li
        v-for="(group, g) in groups"
        :key="g"
        :class="[`n${g + 1}`, { 'is-active': active === g }]"
        @mouseenter="active = g"
        @mouseleave="active = null"
      >
        <span class="cc__badge" aria-hidden="true">{{ g + 1 }}</span>
        <span><strong>{{ group.label }}</strong><span class="cc__sep">：</span>{{ group.note }}</span>
      </li>
    </ol>
  </figure>
</template>

<style scoped>
.cc {
  margin: 24px 0;
}

.cc__code {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 6px;
  padding: 20px 12px 34px;
  border-radius: 8px;
  background: var(--blugit-term-bg);
}

.cc__group {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding-bottom: 6px;
  border-bottom: 3px solid var(--c);
  transition: background-color 0.15s;
}

.cc__group.is-active {
  background: color-mix(in srgb, var(--c) 25%, transparent);
}

.cc__tile {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 34px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.08);
  color: var(--blugit-term-fg);
  font-family: var(--vp-font-family-mono);
  font-size: 20px;
  font-weight: 700;
}

.cc__dash {
  color: var(--blugit-term-muted);
  font-family: var(--vp-font-family-mono);
  font-size: 18px;
}

.cc__group > .cc__badge {
  position: absolute;
  top: 100%;
  left: 50%;
  transform: translate(-50%, 6px);
}

.cc__badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--c);
  color: var(--blugit-note-ink);
  font-size: 11px;
  font-weight: 700;
  line-height: 1;
}

.vp-doc .cc__notes {
  margin: 10px 0 0;
  padding: 0;
  list-style: none;
}

.vp-doc .cc__notes li {
  display: flex;
  gap: 10px;
  align-items: baseline;
  margin: 0;
  padding: 6px 10px;
  border-radius: 6px;
  transition: background-color 0.15s;
}

.vp-doc .cc__notes li + li {
  margin-top: 2px;
}

.cc__notes .cc__badge {
  transform: translateY(3px);
}

.vp-doc .cc__notes li.is-active {
  background: color-mix(in srgb, var(--c) 18%, transparent);
}

.cc__sep {
  color: var(--vp-c-text-3);
}

.n1 { --c: var(--blugit-note-1); }
.n2 { --c: var(--blugit-note-2); }
.n3 { --c: var(--blugit-note-3); }
</style>
