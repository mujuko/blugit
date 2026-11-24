<script setup lang="ts">
/**
 * Local vs remote with two people: A pushes to the shared remote, B pulls from it.
 *   <ShareFigure />
 */
</script>

<template>
  <figure class="sf">
    <svg
      viewBox="0 0 440 300"
      role="img"
      aria-label="AさんのPC（ローカル）から push でリモートへ送り、リモートから pull で BさんのPC（ローカル）へ取り込む。ローカルはそれぞれ専用、リモートはみんなで共有する。"
    >
      <g class="sf__box sf__remote">
        <rect x="100" y="14" width="240" height="66" rx="12" />
        <text class="sf__title" x="220" y="43">リモート（みんなの置き場）</text>
        <text class="sf__caption" x="220" y="65">みんなで共有する履歴</text>
      </g>

      <g class="sf__box sf__a">
        <rect x="12" y="200" width="160" height="66" rx="12" />
        <text class="sf__title" x="92" y="229">AさんのPC</text>
        <text class="sf__caption" x="92" y="251">ローカル（Aさん専用）</text>
      </g>

      <g class="sf__box sf__b">
        <rect x="268" y="200" width="160" height="66" rx="12" />
        <text class="sf__title" x="348" y="229">BさんのPC</text>
        <text class="sf__caption" x="348" y="251">ローカル（Bさん専用）</text>
      </g>

      <g class="sf__arrow">
        <path d="M100 198 L170 92" />
        <polygon points="177,82 177,94 166,88" />
        <text class="sf__verb" x="128" y="136" text-anchor="end">push</text>
        <text class="sf__what" x="128" y="154" text-anchor="end">① 送る</text>
      </g>

      <g class="sf__arrow">
        <path d="M270 92 L340 198" />
        <polygon points="347,208 336,203 346,196" />
        <text class="sf__verb" x="312" y="136">pull</text>
        <text class="sf__what" x="312" y="154">② 取り込む</text>
      </g>

      <text class="sf__note" x="220" y="292">Aさんが push した記録を、Bさんが pull で受け取る</text>
    </svg>
  </figure>
</template>

<style scoped>
.sf {
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
  fill: var(--vp-c-text-1);
  text-anchor: middle;
}

.sf__title {
  font-size: 15px;
  font-weight: 700;
}

.sf__caption {
  fill: var(--vp-c-text-2);
  font-size: 12px;
}

.sf__box rect {
  fill: var(--vp-c-bg-soft);
  stroke: var(--vp-c-border);
  stroke-width: 1.5;
}

.sf__remote rect {
  fill: var(--vp-c-brand-soft);
  stroke: var(--vp-c-brand-1);
  stroke-width: 2;
}

.sf__a rect {
  fill: var(--blugit-host-soft);
  stroke: var(--blugit-host);
  stroke-width: 2;
}

.sf__b rect {
  fill: var(--blugit-guest-soft);
  stroke: var(--blugit-guest);
  stroke-width: 2;
}

.sf__arrow path {
  fill: none;
  stroke: var(--vp-c-brand-1);
  stroke-width: 3;
}

.sf__arrow polygon {
  fill: var(--vp-c-brand-1);
}

.sf__verb {
  fill: var(--vp-c-brand-1);
  font-family: var(--vp-font-family-mono);
  font-size: 14px;
  font-weight: 700;
}

.sf__arrow text:not(.sf__verb) {
  font-size: 12px;
}

.sf__what {
  fill: var(--vp-c-text-2);
}

.sf__arrow text[text-anchor='end'] {
  text-anchor: end;
}

.sf__arrow text:not([text-anchor]) {
  text-anchor: start;
}

.sf__note {
  font-size: 13px;
  font-weight: 600;
}
</style>
