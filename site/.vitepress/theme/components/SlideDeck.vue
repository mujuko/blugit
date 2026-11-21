<script setup lang="ts">
/**
 * A lecture deck on a page with `layout: false`. Each <Slide> inside is one 16:9 slide.
 *
 *   <SlideDeck title="AIと使う はじめてのGit">
 *   <Slide cover>
 *
 *   # タイトル
 *
 *   </Slide>
 *   </SlideDeck>
 *
 * ← → / Space / PageUp・PageDown move between slides, F toggles full screen.
 * The URL hash keeps the current slide (#3, or a slide's `id`), so a slide can be linked.
 * "一覧" shows every slide in one scrolling column, which is also the default on phones.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, watch } from 'vue'
import { withBase } from 'vitepress'

const props = withDefaults(defineProps<{ title?: string; back?: string; backLabel?: string }>(), {
  title: '',
  back: '/',
  backLabel: 'ガイドのトップへ',
})

const WIDTH = 1280
const HEIGHT = 720

const ids = ref<(string | undefined)[]>([])
const current = ref(0)
const mode = ref<'slides' | 'list'>('slides')
const scale = ref(1)
const root = ref<HTMLElement>()
const viewport = ref<HTMLElement>()

provide('slide-deck', {
  register(id?: string) {
    ids.value.push(id)
    return ids.value.length - 1
  },
  open,
  current,
  mode,
})

// From the list: show this slide in slide view.
function open(index: number) {
  setMode('slides')
  go(index)
  const id = ids.value[current.value] ?? String(current.value + 1)
  history.replaceState(history.state, '', `#${id}`)
}

const total = computed(() => ids.value.length)

function go(index: number) {
  current.value = Math.min(Math.max(index, 0), Math.max(total.value - 1, 0))
}

// Keep the hash in step with the slide, so reloading or sharing the URL lands on it.
watch(current, (index) => {
  if (mode.value !== 'slides') return
  const id = ids.value[index] ?? String(index + 1)
  history.replaceState(history.state, '', `#${id}`)
})

function indexFromHash(): number | undefined {
  const hash = decodeURIComponent(location.hash.slice(1))
  if (!hash) return undefined
  const byId = ids.value.indexOf(hash)
  if (byId >= 0) return byId
  if (/^\d+$/.test(hash)) return Number(hash) - 1
  // A heading id (from site search): use the slide that contains it.
  const slide = document.getElementById(hash)?.closest<HTMLElement>('[data-slide-index]')
  return slide ? Number(slide.dataset.slideIndex) : undefined
}

async function fromHash() {
  const index = indexFromHash()
  if (index === undefined) return
  go(index)
  if (mode.value === 'list') {
    // Wait until every slide is shown, and until VitePress has done its own
    // scroll-to-hash on page load (which runs while the slide is still hidden).
    await nextTick()
    setTimeout(() => {
      document.querySelector(`[data-slide-index="${current.value}"]`)?.scrollIntoView()
    }, 100)
  }
}

function fit() {
  const el = viewport.value
  if (!el) return
  scale.value = Math.min(el.clientWidth / WIDTH, el.clientHeight / HEIGHT)
}

function onKey(event: KeyboardEvent) {
  if (mode.value !== 'slides' || event.altKey || event.ctrlKey || event.metaKey) return
  const target = event.target as HTMLElement | null
  if (target?.closest('input, textarea, select, [contenteditable]')) return
  if (['ArrowRight', 'PageDown', ' '].includes(event.key)) go(current.value + 1)
  else if (['ArrowLeft', 'PageUp'].includes(event.key)) go(current.value - 1)
  else if (event.key === 'Home') go(0)
  else if (event.key === 'End') go(total.value - 1)
  else if (event.key === 'f' || event.key === 'F') toggleFullscreen()
  else return
  event.preventDefault()
}

let touchX: number | null = null
function onTouchStart(event: TouchEvent) {
  touchX = event.touches[0]?.clientX ?? null
}
function onTouchEnd(event: TouchEvent) {
  if (touchX === null) return
  const dx = (event.changedTouches[0]?.clientX ?? touchX) - touchX
  if (Math.abs(dx) > 50) go(current.value + (dx < 0 ? 1 : -1))
  touchX = null
}

function toggleFullscreen() {
  if (document.fullscreenElement) document.exitFullscreen?.()
  else root.value?.requestFullscreen?.().catch(() => {})
}

function setMode(next: 'slides' | 'list') {
  mode.value = next
  try {
    localStorage.setItem('blugit-deck-mode', next)
  } catch {
    // Storage can be unavailable; the choice just isn't remembered.
  }
  if (next === 'slides') requestAnimationFrame(fit)
}

let observer: ResizeObserver | undefined

onMounted(() => {
  let saved: string | null = null
  try {
    saved = localStorage.getItem('blugit-deck-mode')
  } catch {
    saved = null
  }
  mode.value = saved === 'slides' || saved === 'list' ? saved : window.innerWidth < 768 ? 'list' : 'slides'
  fromHash()
  fit()
  observer = new ResizeObserver(fit)
  if (viewport.value) observer.observe(viewport.value)
  window.addEventListener('keydown', onKey)
  window.addEventListener('hashchange', fromHash)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('hashchange', fromHash)
})
</script>

<template>
  <div ref="root" class="deck" :class="`deck--${mode}`">
    <header class="deck__top">
      <a class="deck__back" :href="withBase(back)">← {{ backLabel }}</a>
      <span class="deck__title">{{ title }}</span>
      <div class="deck__modes" role="group" aria-label="表示方法">
        <button type="button" :aria-pressed="mode === 'slides'" @click="setMode('slides')">スライド</button>
        <button type="button" :aria-pressed="mode === 'list'" @click="setMode('list')">一覧</button>
      </div>
    </header>

    <div
      ref="viewport"
      class="deck__viewport"
      @touchstart.passive="onTouchStart"
      @touchend.passive="onTouchEnd"
    >
      <div class="deck__stage" :style="mode === 'slides' ? { transform: `translate(-50%, -50%) scale(${scale})` } : undefined">
        <slot />
      </div>
    </div>

    <footer v-if="mode === 'slides'" class="deck__bottom">
      <div class="deck__progress" :style="{ width: `${total ? ((current + 1) / total) * 100 : 0}%` }" />
      <button type="button" class="deck__nav" :disabled="current === 0" aria-label="前のスライド" @click="go(current - 1)">←</button>
      <span class="deck__count">{{ current + 1 }} / {{ total }}</span>
      <button type="button" class="deck__nav" :disabled="current >= total - 1" aria-label="次のスライド" @click="go(current + 1)">→</button>
      <button type="button" class="deck__full" @click="toggleFullscreen">全画面</button>
    </footer>
  </div>
</template>

<style>
/* Unscoped: these rules style the markdown inside each <Slide>. */
.deck {
  --deck-accent: var(--vp-c-brand-1);
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  background: var(--vp-c-bg);
  color: var(--vp-c-text-1);
}

.deck--slides {
  position: fixed;
  inset: 0;
}

.deck__top {
  display: flex;
  align-items: center;
  gap: 16px;
  height: 48px;
  padding: 0 16px;
  border-bottom: 1px solid var(--vp-c-divider);
  background: var(--vp-c-bg);
  font-size: 14px;
}

.deck--list .deck__top {
  position: sticky;
  top: 0;
  z-index: 10;
}

.deck__back {
  color: var(--vp-c-text-2);
  text-decoration: none;
  white-space: nowrap;
}

.deck__back:hover {
  color: var(--deck-accent);
}

.deck__title {
  overflow: hidden;
  flex: 1;
  font-weight: 700;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.deck__modes {
  display: inline-flex;
  overflow: hidden;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
}

.deck__modes button {
  padding: 4px 12px;
  color: var(--vp-c-text-2);
  font-size: 13px;
}

.deck__modes button[aria-pressed='true'] {
  background: var(--vp-c-brand-soft);
  color: var(--deck-accent);
  font-weight: 700;
}

.deck__viewport {
  position: relative;
  flex: 1;
  overflow: hidden;
  background: var(--vp-c-bg-alt);
}

.deck--slides .deck__stage {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 1280px;
  height: 720px;
  transform-origin: center;
}

.deck--list .deck__viewport {
  overflow: visible;
  padding: 24px 16px 64px;
}

.deck--list .deck__stage {
  display: flex;
  flex-direction: column;
  gap: 24px;
  max-width: 960px;
  margin: 0 auto;
}

.deck__bottom {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 16px;
  height: 52px;
  border-top: 1px solid var(--vp-c-divider);
  background: var(--vp-c-bg);
}

.deck__progress {
  position: absolute;
  top: -1px;
  left: 0;
  height: 3px;
  background: var(--deck-accent);
  transition: width 0.2s;
}

.deck__nav {
  width: 40px;
  height: 36px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  font-size: 18px;
}

.deck__nav:disabled {
  opacity: 0.3;
}

.deck__count {
  min-width: 64px;
  color: var(--vp-c-text-2);
  font-size: 14px;
  font-variant-numeric: tabular-nums;
  text-align: center;
}

.deck__full {
  position: absolute;
  right: 16px;
  padding: 4px 12px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  color: var(--vp-c-text-2);
  font-size: 13px;
}

@media (max-width: 640px) {
  .deck__title {
    display: none;
  }

  .deck__full {
    display: none;
  }
}
</style>
