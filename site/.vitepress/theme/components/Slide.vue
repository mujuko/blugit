<script setup lang="ts">
/**
 * One slide inside <SlideDeck>. Leave blank lines around the markdown inside it.
 *   cover   — the title slide
 *   section — a chapter divider
 *   id      — a name for the URL hash (#map), so other pages can link to this slide
 */
import { inject, type Ref } from 'vue'

const props = defineProps<{ id?: string; cover?: boolean; section?: boolean }>()

const deck = inject<{
  register: (id?: string) => number
  open: (index: number) => void
  current: Ref<number>
  mode: Ref<'slides' | 'list'>
}>('slide-deck')

if (!deck) throw new Error('<Slide> must be placed inside <SlideDeck>')
const index = deck.register(props.id)

// In the list, clicking a slide (or Enter on it) opens it in slide view.
// Links and buttons inside keep working, and selecting text does not count as a click.
function onClick(event: MouseEvent) {
  if (deck!.mode.value !== 'list') return
  if ((event.target as HTMLElement).closest('a, button, input, textarea, select')) return
  if (window.getSelection()?.toString()) return
  deck!.open(index)
}

function onKey(event: KeyboardEvent) {
  if (deck!.mode.value !== 'list' || event.key !== 'Enter' || event.target !== event.currentTarget) return
  event.preventDefault()
  deck!.open(index)
}
</script>

<template>
  <section
    v-show="deck!.mode.value === 'list' || deck!.current.value === index"
    :id="id"
    class="slide vp-doc"
    :class="{ 'slide--cover': cover, 'slide--section': section }"
    :data-slide-index="index"
    :aria-label="`スライド ${index + 1}`"
    :tabindex="deck!.mode.value === 'list' ? 0 : undefined"
    :title="deck!.mode.value === 'list' ? 'クリックすると、このスライドを大きく表示します' : undefined"
    @click="onClick"
    @keydown="onKey"
  >
    <div class="slide__body">
      <slot />
    </div>
    <span class="slide__number" aria-hidden="true">{{ index + 1 }}</span>
  </section>
</template>

<style>
/* Unscoped so the markdown inside the slot is styled too. Sizes are for the 1280×720 stage. */
.slide {
  position: relative;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: center;
  overflow: hidden;
  background: var(--vp-c-bg);
}

.deck--slides .slide {
  width: 1280px;
  height: 720px;
  padding: 64px 96px 56px;
  font-size: 28px;
  line-height: 1.6;
}

/* Content slides start at the top, so the title sits in the same place on every slide.
   Only the cover and chapter dividers are centered. */
.deck--slides .slide:not(.slide--cover):not(.slide--section) {
  justify-content: flex-start;
}

.deck--list .slide {
  scroll-margin-top: 64px;
  padding: 32px 28px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 12px;
  font-size: 17px;
  line-height: 1.75;
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.deck--list .slide:hover,
.deck--list .slide:focus-visible {
  border-color: var(--vp-c-brand-1);
  box-shadow: 0 0 0 3px var(--vp-c-brand-soft);
  outline: none;
}

.slide__number {
  position: absolute;
  right: 28px;
  bottom: 18px;
  color: var(--vp-c-text-3);
  font-size: 18px;
}

.deck--list .slide__number {
  top: 12px;
  bottom: auto;
  right: 16px;
  font-size: 13px;
}

/* Headings: no doc-style rules or extra space on a slide. */
.slide.vp-doc h1,
.slide.vp-doc h2,
.slide.vp-doc h3 {
  margin: 0 0 0.6em;
  padding: 0;
  border: 0;
  letter-spacing: -0.01em;
  line-height: 1.3;
}

.deck--slides .slide.vp-doc h1 { font-size: 68px; }
.deck--slides .slide.vp-doc h2 { font-size: 46px; }
.deck--slides .slide.vp-doc h3 { font-size: 32px; }
.deck--list .slide.vp-doc h1 { font-size: 30px; }
.deck--list .slide.vp-doc h2 { font-size: 24px; }
.deck--list .slide.vp-doc h3 { font-size: 19px; }

.slide.vp-doc .header-anchor {
  display: none;
}

.slide.vp-doc p,
.slide.vp-doc ul,
.slide.vp-doc ol {
  margin: 0.4em 0;
  line-height: inherit;
}

.slide.vp-doc li + li {
  margin-top: 0.3em;
}

.slide.vp-doc table {
  margin: 0.4em 0;
}

.deck--slides .slide.vp-doc th,
.deck--slides .slide.vp-doc td {
  padding: 14px 20px;
  font-size: 24px;
  line-height: 1.45;
}

.deck--slides .slide.vp-doc :not(pre) > code {
  font-size: 0.85em;
}

.slide__body > :first-child {
  margin-top: 0;
}

.slide__body > :last-child {
  margin-bottom: 0;
}

/* Cover and chapter dividers */
.slide--cover,
.slide--section {
  text-align: center;
}

.slide--cover {
  background:
    radial-gradient(circle at 15% 20%, var(--vp-c-brand-soft), transparent 45%),
    radial-gradient(circle at 85% 80%, var(--vp-c-brand-soft), transparent 45%),
    var(--vp-c-bg);
}

.slide--section {
  background: var(--vp-c-brand-soft);
}

.slide.vp-doc .lead {
  color: var(--vp-c-text-2);
}

.deck--slides .slide.vp-doc .lead {
  font-size: 32px;
}

/* Chapter number at the start of a section heading: ## <span class="chapter-no">1</span>… */
.slide.vp-doc .chapter-no {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.5em;
  height: 1.5em;
  margin-right: 0.45em;
  border-radius: 50%;
  background: var(--vp-c-brand-1);
  color: var(--vp-c-white);
  font-size: 0.8em;
  vertical-align: 0.08em;
}

.slide.vp-doc .kicker {
  margin: 0 0 0.3em;
  color: var(--vp-c-brand-1);
  font-weight: 700;
  letter-spacing: 0.08em;
}

/* Bigger figures on the slide stage. The stage is always 1280px wide, so the map
   keeps its wide layout even when the window itself is narrow. */
.deck--slides .slide .gm__svg--wide {
  display: block !important;
  max-width: 1040px;
  margin: 0 auto;
}

.deck--slides .slide .gm__svg--tall {
  display: none !important;
}

.deck--slides .slide .sf svg {
  max-width: 700px;
}

.deck--slides .slide .gm,
.deck--slides .slide .sf,
.deck--slides .slide .cg,
.deck--slides .slide .chat {
  margin: 16px 0 0;
}

.deck--slides .slide .gm figcaption,
.deck--slides .slide .cg figcaption {
  font-size: 22px;
}

/* Two columns */
.slide .cols {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 32px;
}

.deck--list .slide .cols {
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 16px;
}

/* Text on the left, a tall figure on the right. */
.deck--slides .slide .cols--figure {
  grid-template-columns: 1fr 500px;
  align-items: center;
}

.deck--slides .slide .ht svg {
  width: auto;
  max-width: 100%;
  height: 490px;
}

.slide .card {
  padding: 0.8em 1em;
  border: 2px solid var(--vp-c-divider);
  border-radius: 16px;
  background: var(--vp-c-bg-soft);
}

.slide .card > :first-child {
  margin-top: 0;
}

.slide .card--a {
  border-color: var(--blugit-host);
  background: var(--blugit-host-soft);
}

.slide .card--b {
  border-color: var(--vp-c-brand-1);
  background: var(--vp-c-brand-soft);
}

/* A row of steps: <div class="steps"><div><b>add</b>選ぶ</div>…</div> */
.slide .steps {
  display: flex;
  flex-wrap: wrap;
  align-items: stretch;
  justify-content: center;
  gap: 0.6em;
  margin: 0.8em 0;
}

.slide .steps > div {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-width: 5.2em;
  padding: 0.5em 0.8em;
  border: 2px solid var(--vp-c-brand-1);
  border-radius: 14px;
  background: var(--vp-c-bg);
  line-height: 1.3;
  text-align: center;
}

.slide .steps > div b {
  color: var(--vp-c-brand-1);
  font-family: var(--vp-font-family-mono);
  font-size: 1.1em;
}

.slide .steps > div small {
  color: var(--vp-c-text-2);
  font-size: 0.7em;
}

.slide .steps > .arrow {
  min-width: 0;
  padding: 0;
  border: 0;
  background: none;
  color: var(--vp-c-text-3);
}

/* "Choose what to record": a checklist of changed files. */
.slide .pick {
  align-self: center;
  padding: 0.6em 0.8em;
  border: 2px solid var(--vp-c-divider);
  border-radius: 16px;
  background: var(--vp-c-bg-soft);
}

.slide .pick__title {
  margin-bottom: 0.3em;
  color: var(--vp-c-text-2);
  font-size: 0.75em;
  font-weight: 700;
}

.slide .pick__row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.45em;
  padding: 0.2em 0.3em;
  border-radius: 8px;
  color: var(--vp-c-text-3);
  font-family: var(--vp-font-family-mono);
}

.slide .pick__row.is-on {
  color: var(--vp-c-text-1);
  font-weight: 600;
}

.slide .pick__row small {
  margin-left: 0.3em;
  font-family: var(--vp-font-family-base);
  font-size: 0.6em;
  font-weight: 400;
}

.slide .pick__box {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.1em;
  height: 1.1em;
  border: 2px solid var(--vp-c-text-3);
  border-radius: 5px;
  font-size: 0.85em;
  line-height: 1;
}

.slide .pick__row.is-on .pick__box {
  border-color: var(--vp-c-brand-1);
  background: var(--vp-c-brand-1);
  color: var(--vp-c-white);
}

.slide .pick__next {
  margin-top: 0.4em;
  padding-top: 0.4em;
  border-top: 1px dashed var(--vp-c-divider);
  color: var(--vp-c-brand-1);
  font-size: 0.75em;
  font-weight: 700;
}

.slide .big {
  font-size: 1.3em;
  font-weight: 700;
}

.slide .center {
  text-align: center;
}
</style>
