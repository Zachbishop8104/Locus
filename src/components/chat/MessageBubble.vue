<script setup lang="ts">
import { computed } from 'vue'
import { marked } from 'marked'
import { AlertCircle } from '@lucide/vue'
import type { Message } from '@/types'

const props = defineProps<{
  message: Message
  streaming?: boolean
}>()

const isAssistant = computed(() => props.message.role === 'assistant')

const renderedContent = computed(() => {
  if (!isAssistant.value) return null
  return marked.parse(props.message.content || '') as string
})

function formatTime(date: string | Date) {
  return new Date(date).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}
</script>

<template>
  <div
    class="flex gap-3 px-6 py-3"
    :class="isAssistant ? 'items-start' : 'items-start flex-row-reverse'"
  >
    <!-- Avatar -->
    <div
      class="w-7 h-7 rounded-full shrink-0 flex items-center justify-center text-[10px] font-semibold tracking-wide"
      :class="isAssistant
        ? 'bg-accent/30 text-accent-icon border border-accent/40'
        : 'bg-elevated text-fg-subtle border border-border-strong'"
    >
      {{ isAssistant ? 'AI' : 'me' }}
    </div>

    <!-- Content -->
    <div class="flex flex-col gap-1 max-w-[80%]" :class="isAssistant ? '' : 'items-end'">
      <div
        class="rounded-2xl px-4 py-3 text-sm leading-relaxed"
        :class="[
          isAssistant
            ? 'bg-elevated/80 text-fg rounded-tl-sm'
            : 'bg-accent text-white rounded-tr-sm',
          message.error ? 'border border-red-500/50' : '',
        ]"
      >
        <!-- Error indicator -->
        <div v-if="message.error" class="flex items-center gap-1.5 text-red-400 text-xs mb-1">
          <AlertCircle :size="12" />
          <span>Error</span>
        </div>

        <!-- Assistant: rendered markdown -->
        <div
          v-if="isAssistant"
          class="prose prose-sm prose-invert claude-prose max-w-none"
          :class="streaming ? 'streaming-cursor' : ''"
          v-html="renderedContent"
        />

        <!-- User: plain text -->
        <p v-else class="whitespace-pre-wrap m-0">{{ message.content }}</p>
      </div>

      <span class="text-xs text-fg-subtle px-1">{{ formatTime(message.timestamp) }}</span>
    </div>
  </div>
</template>
