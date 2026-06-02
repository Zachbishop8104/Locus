<script setup lang="ts">
import { ref, watch, onMounted, nextTick } from 'vue'
import MessageBubble from './MessageBubble.vue'
import type { Message } from '@/types'

const props = defineProps<{
  messages: Message[]
  streamingId?: string | null
  toolActivity?: string | null
}>()

const scrollEl = ref<HTMLElement | null>(null)

function scrollToBottom() {
  if (scrollEl.value) scrollEl.value.scrollTop = scrollEl.value.scrollHeight
}

onMounted(async () => {
  await nextTick()
  scrollToBottom()
})

watch(
  () => [props.messages.length, props.messages[props.messages.length - 1]?.content],
  async () => {
    await nextTick()
    scrollToBottom()
  },
)
</script>

<template>
  <div ref="scrollEl" class="flex-1 overflow-y-auto py-4">
    <div v-if="messages.length === 0" class="flex flex-col items-center justify-center h-full gap-4">
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
      :tool-activity="msg.id === streamingId ? toolActivity : null"
    />
  </div>
</template>
