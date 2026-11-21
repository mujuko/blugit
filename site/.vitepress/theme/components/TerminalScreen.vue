<script setup lang="ts">
/**
 * A terminal window showing what a command prints, with numbered notes on lines.
 *
 *   <TerminalScreen
 *     who="host"
 *     command="blugit host ./demo.git"
 *     :lines="[
 *       'Sharing ./demo.git',
 *       ['  Connection code: 00MX-8433-A77D', 'これをゲストに伝える'],
 *       { text: 'Six-digit code: ', input: '123456', note: '番号を入力' },
 *     ]"
 *   />
 *
 * A line is a string, a [text, note] pair, or { text, input?, note? };
 * `input` is drawn as what the reader types.
 */
import { computed, ref } from 'vue'

type LineInput = string | [string, string] | { text: string; input?: string; note?: string }

const props = defineProps<{
  lines: LineInput[]
  command?: string
  who?: 'host' | 'guest'
  title?: string
}>()

const rows = computed(() => {
  let next = 0
  return props.lines.map((line) => {
    const row =
      typeof line === 'string'
        ? { text: line }
        : Array.isArray(line)
          ? { text: line[0], note: line[1] }
          : line
    return { ...row, index: row.note ? next++ : undefined }
  })
})

const notes = computed(() =>
  rows.value.filter((row) => row.index !== undefined).map((row) => row.note as string),
)

const heading = computed(
  () => props.title ?? (props.who === 'host' ? 'ホストのPC' : props.who === 'guest' ? 'ゲストのPC' : 'ターミナル'),
)

const active = ref<number | null>(null)
const color = (index: number) => `n${(index % 5) + 1}`
</script>

<template>
  <figure class="ts" :class="who && `ts--${who}`">
    <div class="ts__bar">
      <span class="ts__dots" aria-hidden="true"><i /><i /><i /></span>
      <span class="ts__title">{{ heading }}</span>
    </div>
    <div class="ts__body">
      <div class="ts__lines">
        <div v-if="command" class="ts__row ts__command">
          <span class="ts__gutter" />
          <span><span class="ts__prompt" aria-hidden="true">❯ </span>{{ command }}</span>
        </div>
        <!-- Badges sit in a left gutter so they stay visible when long lines scroll sideways. -->
        <div
          v-for="(row, i) in rows"
          :key="i"
          class="ts__row"
          :class="row.index !== undefined && [color(row.index), 'has-note', { 'is-active': active === row.index }]"
          @mouseenter="row.index !== undefined && (active = row.index)"
          @mouseleave="active = null"
        >
          <span class="ts__gutter">
            <span v-if="row.index !== undefined" class="ts__badge" aria-hidden="true">{{ row.index + 1 }}</span>
          </span>
          <span>{{ row.text || ' ' }}<span v-if="row.input" class="ts__input">{{ row.input }}</span></span>
        </div>
      </div>
    </div>
    <ol v-if="notes.length" class="ts__notes">
      <li
        v-for="(note, i) in notes"
        :key="i"
        :class="[color(i), { 'is-active': active === i }]"
        @mouseenter="active = i"
        @mouseleave="active = null"
      >
        <span class="ts__badge" aria-hidden="true">{{ i + 1 }}</span>
        <span>{{ note }}</span>
      </li>
    </ol>
  </figure>
</template>

<style scoped>
.ts {
  margin: 20px 0;
}

.ts__bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  border-radius: 8px 8px 0 0;
  background: var(--blugit-term-bar);
  border-top: 3px solid var(--blugit-term-bar);
}

.ts--host .ts__bar {
  border-top-color: var(--blugit-host);
}

.ts--guest .ts__bar {
  border-top-color: var(--blugit-guest);
}

.ts__dots {
  display: inline-flex;
  gap: 5px;
}

.ts__dots i {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.22);
}

.ts__title {
  color: var(--blugit-term-muted);
  font-size: 12px;
  font-weight: 600;
}

.ts--host .ts__title {
  color: #93c5fd;
}

.ts--guest .ts__title {
  color: #fdba74;
}

.ts__body {
  overflow-x: auto;
  padding: 10px 0 12px;
  border-radius: 0 0 8px 8px;
  background: var(--blugit-term-bg);
}

.ts__lines {
  min-width: max-content;
  color: var(--blugit-term-fg);
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
  line-height: 1.75;
  white-space: pre;
}

.ts__row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 14px 0 8px;
  transition: background-color 0.15s;
}

.ts__row.is-active {
  background: color-mix(in srgb, var(--c) 22%, transparent);
}

.ts__gutter {
  display: inline-flex;
  flex: none;
  width: 18px;
}

.ts__command {
  color: #fff;
  font-weight: 600;
}

.ts__prompt {
  color: var(--blugit-term-prompt);
}

.ts__input {
  color: var(--blugit-note-1);
  text-decoration: underline dotted;
  text-underline-offset: 4px;
}

.ts__badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--c);
  color: var(--blugit-note-ink);
  font-family: var(--vp-font-family-base);
  font-size: 11px;
  font-weight: 700;
  line-height: 1;
}

.vp-doc .ts__notes {
  margin: 10px 0 0;
  padding: 0;
  list-style: none;
}

.vp-doc .ts__notes li {
  display: flex;
  gap: 10px;
  align-items: baseline;
  margin: 0;
  padding: 6px 10px;
  border-radius: 6px;
  transition: background-color 0.15s;
}

.vp-doc .ts__notes li + li {
  margin-top: 2px;
}

.ts__notes .ts__badge {
  transform: translateY(3px);
}

.vp-doc .ts__notes li.is-active {
  background: color-mix(in srgb, var(--c) 18%, transparent);
}

.n1 { --c: var(--blugit-note-1); }
.n2 { --c: var(--blugit-note-2); }
.n3 { --c: var(--blugit-note-3); }
.n4 { --c: var(--blugit-note-4); }
.n5 { --c: var(--blugit-note-5); }
</style>
