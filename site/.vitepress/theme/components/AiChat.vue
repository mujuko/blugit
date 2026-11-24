<script setup lang="ts">
/**
 * A short exchange with an AI assistant, to show how Git words appear in
 * requests and in the AI's reports:
 *
 *   <AiChat :messages="[
 *     ['you', 'ここまでの変更をコミットして'],
 *     ['ai', 'todo.txt の変更を add して、コミットしました'],
 *   ]" />
 */
defineProps<{ messages: ['you' | 'ai', string][] }>()
</script>

<template>
  <figure class="chat">
    <div v-for="(message, i) in messages" :key="i" class="chat__row" :class="`chat__row--${message[0]}`">
      <span class="chat__who">{{ message[0] === 'you' ? 'あなた' : 'AI' }}</span>
      <p class="chat__bubble">{{ message[1] }}</p>
    </div>
  </figure>
</template>

<style scoped>
.chat {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 20px 0;
  padding: 16px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 12px;
  background: var(--vp-c-bg-soft);
}

.chat__row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.chat__row--you {
  flex-direction: row-reverse;
}

.chat__who {
  flex: none;
  min-width: 3.6em;
  padding-top: 0.7em;
  color: var(--vp-c-text-2);
  font-size: max(12px, 0.6em);
  font-weight: 700;
  text-align: center;
}

.vp-doc .chat__bubble {
  max-width: 80%;
  margin: 0;
  padding: 8px 14px;
  border-radius: 14px;
  line-height: 1.7;
}

.chat__row--you .chat__bubble {
  border-top-right-radius: 4px;
  background: var(--vp-c-brand-1);
  color: var(--vp-c-white);
}

.chat__row--ai .chat__bubble {
  border-top-left-radius: 4px;
  background: var(--vp-c-bg);
  border: 1px solid var(--vp-c-divider);
}
</style>
