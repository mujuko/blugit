<script setup lang="ts">
/** Inline chip that names a role in running text: <Role who="host" />. */
defineProps<{ who: 'host' | 'guest' }>()
</script>

<template>
  <span class="role" :class="`role--${who}`">{{ who === 'host' ? 'ホスト' : 'ゲスト' }}</span>
</template>

<style scoped>
.role {
  display: inline-block;
  padding: 0 0.5em;
  border-radius: 999px;
  font-size: 0.85em;
  font-weight: 600;
  line-height: 1.6;
  white-space: nowrap;
}

.role--host {
  color: var(--blugit-host);
  background: var(--blugit-host-soft);
}

.role--guest {
  color: var(--blugit-guest);
  background: var(--blugit-guest-soft);
}
</style>
