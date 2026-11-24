<script setup lang="ts">
/**
 * A command with underlined, numbered parts and a note for each part.
 *
 * Wrap each annotated part in braces; notes are matched in order:
 *
 *   <AnnotatedCommand
 *     who="guest"
 *     command="blugit join {00MX-8433-A77D} {demo}"
 *     :notes="[
 *       ['接続コード', 'ホストの画面に出た12文字。'],
 *       ['登録名', 'このPCでの呼び名。'],
 *     ]"
 *   />
 *
 * A part written as <...> is drawn as a slot the reader replaces.
 * The copy button copies the command without braces.
 */
import { computed, ref } from 'vue'

const props = defineProps<{
  command: string
  notes?: [label: string, text: string][]
  who?: 'host' | 'guest'
}>()

type Part = { text: string; index?: number; slot?: boolean }

const parts = computed<Part[]>(() => {
  const result: Part[] = []
  const pattern = /\{([^}]+)\}/g
  let last = 0
  let index = 0
  for (const match of props.command.matchAll(pattern)) {
    if (match.index > last) result.push({ text: props.command.slice(last, match.index) })
    result.push({ text: match[1], index: index++, slot: /^<.+>$/.test(match[1]) })
    last = match.index + match[0].length
  }
  if (last < props.command.length) result.push({ text: props.command.slice(last) })
  return result
})

const plain = computed(() => props.command.replace(/\{([^}]+)\}/g, '$1'))

const active = ref<number | null>(null)
const copied = ref(false)
let copiedTimer: ReturnType<typeof setTimeout> | undefined

async function copy() {
  try {
    await navigator.clipboard.writeText(plain.value)
    copied.value = true
    clearTimeout(copiedTimer)
    copiedTimer = setTimeout(() => (copied.value = false), 1500)
  } catch {
    // Clipboard can be unavailable (insecure origin, denied permission); the text stays selectable.
  }
}

const color = (index: number) => `n${(index % 5) + 1}`
</script>

<template>
  <figure class="ac" :class="who && `ac--${who}`">
    <figcaption v-if="who" class="ac__who">
      {{ who === 'host' ? 'ホストのPCで実行' : 'ゲストのPCで実行' }}
    </figcaption>
    <div class="ac__box">
      <code class="ac__code"><template v-for="(part, i) in parts" :key="i"><span
            v-if="part.index === undefined"
          >{{ part.text }}</span><span
            v-else
            class="ac__mark"
            :class="[color(part.index), { 'is-active': active === part.index, 'is-slot': part.slot }]"
            @mouseenter="active = part.index"
            @mouseleave="active = null"
          >{{ part.text }}<span class="ac__badge" aria-hidden="true">{{ part.index + 1 }}</span></span></template></code>
      <button type="button" class="ac__copy" @click="copy">
        {{ copied ? 'コピーしました' : 'コピー' }}
      </button>
    </div>
    <ol v-if="notes?.length" class="ac__notes">
      <li
        v-for="(note, i) in notes"
        :key="i"
        :class="[color(i), { 'is-active': active === i }]"
        @mouseenter="active = i"
        @mouseleave="active = null"
      >
        <span class="ac__badge" aria-hidden="true">{{ i + 1 }}</span>
        <span><strong>{{ note[0] }}</strong><span class="ac__sep">：</span>{{ note[1] }}</span>
      </li>
    </ol>
  </figure>
</template>

<style scoped>
.ac {
  margin: 20px 0;
}

.ac__who {
  display: inline-block;
  margin-bottom: 6px;
  padding: 0 10px;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 600;
  line-height: 1.8;
}

.ac--host .ac__who {
  color: var(--blugit-host);
  background: var(--blugit-host-soft);
}

.ac--guest .ac__who {
  color: var(--blugit-guest);
  background: var(--blugit-guest-soft);
}

.ac__box {
  position: relative;
  padding: 14px 16px 10px;
  border-radius: 8px;
  background: var(--blugit-term-bg);
  border-left: 4px solid transparent;
}

.ac--host .ac__box {
  border-left-color: var(--blugit-host);
}

.ac--guest .ac__box {
  border-left-color: var(--blugit-guest);
}

.vp-doc .ac__code {
  display: block;
  padding: 0;
  margin-right: 96px;
  background: none;
  color: var(--blugit-term-fg);
  font-size: 15px;
  /* Leaves room under each line for the number badges, also when the command wraps. */
  line-height: 3;
  white-space: pre-wrap;
  word-break: break-all;
}

.ac__mark {
  position: relative;
  display: inline-block;
  line-height: 1.6;
  border-bottom: 3px solid var(--c);
  border-radius: 2px;
  transition: background-color 0.15s;
}

.ac__mark.is-slot {
  padding: 0 2px;
  border: 1px dashed var(--c);
  border-bottom: 3px solid var(--c);
}

.ac__mark.is-active {
  background: color-mix(in srgb, var(--c) 28%, transparent);
}

.ac__mark > .ac__badge {
  position: absolute;
  top: 100%;
  left: 50%;
  transform: translate(-50%, 4px);
}

.ac__badge {
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

.ac__copy {
  position: absolute;
  top: 10px;
  right: 10px;
  padding: 2px 10px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 6px;
  color: var(--blugit-term-muted);
  font-size: 12px;
  line-height: 1.8;
}

.ac__copy:hover {
  color: var(--blugit-term-fg);
  border-color: rgba(255, 255, 255, 0.4);
}

.vp-doc .ac__notes {
  margin: 10px 0 0;
  padding: 0;
  list-style: none;
}

.vp-doc .ac__notes li {
  display: flex;
  gap: 10px;
  align-items: baseline;
  margin: 0;
  padding: 6px 10px;
  border-radius: 6px;
  transition: background-color 0.15s;
}

.vp-doc .ac__notes li + li {
  margin-top: 2px;
}

.ac__notes .ac__badge {
  transform: translateY(3px);
}

.vp-doc .ac__notes li.is-active {
  background: color-mix(in srgb, var(--c) 18%, transparent);
}

.ac__sep {
  color: var(--vp-c-text-3);
}

.n1 { --c: var(--blugit-note-1); }
.n2 { --c: var(--blugit-note-2); }
.n3 { --c: var(--blugit-note-3); }
.n4 { --c: var(--blugit-note-4); }
.n5 { --c: var(--blugit-note-5); }

@media (max-width: 480px) {
  .vp-doc .ac__code {
    margin-right: 0;
    margin-top: 22px;
    font-size: 14px;
  }
}
</style>
