<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'
import MessageBubble from './MessageBubble.vue'
import type { Message } from '@/types'
import logoUrl from '@/assets/locus-logo.svg'

const props = defineProps<{
  messages: Message[]
  streamingId?: string | null
}>()

const scrollEl = ref<HTMLElement | null>(null)

watch(
  () => [props.messages.length, props.messages[props.messages.length - 1]?.content],
  async () => {
    await nextTick()
    if (scrollEl.value) {
      scrollEl.value.scrollTop = scrollEl.value.scrollHeight
    }
  },
)
</script>

<template>
  <div ref="scrollEl" class="flex-1 overflow-y-auto py-4">
    <div v-if="messages.length === 0" class="flex flex-col items-center justify-center h-full gap-4">
      <img :src="logoUrl" alt="Locus" class="h-10 w-auto opacity-60" />
      <div class="text-center">
        <p class="text-fg-muted font-medium">How can I help you today?</p>
        <p class="text-fg-subtle text-sm mt-1">Ask me anything about your projects</p>
      </div>
    </div>

    <MessageBubble
      v-for="msg in messages"
      :key="msg.id"
      :message="msg"
      :streaming="msg.id === streamingId"
    />
  </div>
</template>
