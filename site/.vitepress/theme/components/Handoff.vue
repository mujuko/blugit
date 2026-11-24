<script setup lang="ts">
/**
 * Something one person passes to the other between two screens:
 * <Handoff from="host" to="guest">6桁の番号を口頭で伝える</Handoff>
 */
import Role from './Role.vue'

defineProps<{ from: 'host' | 'guest'; to: 'host' | 'guest' }>()
</script>

<template>
  <div class="handoff">
    <span class="handoff__arrow" aria-hidden="true">↓</span>
    <span class="handoff__body">
      <Role :who="from" />
      <span class="handoff__to" aria-label="から">→</span>
      <Role :who="to" />
      <span class="handoff__text"><slot /></span>
    </span>
  </div>
</template>

<style scoped>
.handoff {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  margin: 12px 0;
}

.handoff__arrow {
  color: var(--vp-c-text-3);
  font-size: 1.25rem;
}

.handoff__body {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  border: 1px dashed var(--vp-c-divider);
  border-radius: 999px;
  background: var(--vp-c-bg-soft);
}

.handoff__to {
  color: var(--vp-c-text-3);
}

.handoff__text {
  margin-left: 4px;
  font-weight: 600;
}
</style>
