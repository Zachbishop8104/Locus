<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { MessageSquare, MessageSquarePlus } from '@lucide/vue'
import { useBoardStore } from '@/stores/board'

/** Opens a chat linked to a board card, or starts one. */
const props = defineProps<{ taskKey: string }>()
const emit = defineEmits<{ start: []; open: [id: string] }>()

const board = useBoardStore()

const chats = computed(() => board.chatsFor(props.taskKey))
const open = ref(false)
const root = ref<HTMLElement | null>(null)

function onDocClick(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) close()
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}
function close() {
  open.value = false
  document.removeEventListener('mousedown', onDocClick)
  document.removeEventListener('keydown', onKey)
}
onBeforeUnmount(close)

function onClick() {
  // No chats yet: start one straight away.
  if (!chats.value.length) return emit('start')
  if (open.value) return close()
  open.value = true
  document.addEventListener('mousedown', onDocClick)
  document.addEventListener('keydown', onKey)
}

function openChat(id: string) {
  close()
  emit('open', id)
}

function startNew() {
  close()
  emit('start')
}
</script>

<template>
  <div ref="root" class="relative" @click.stop>
    <button
      class="flex items-center gap-1 px-1.5 py-0.5 rounded-md text-[11px] font-medium transition-colors cursor-pointer"
      :class="chats.length
        ? 'bg-accent/10 text-accent-fg hover:bg-accent/20'
        : 'text-fg-subtle hover:text-fg hover:bg-elevated'"
      :title="chats.length ? `${chats.length} linked chat${chats.length > 1 ? 's' : ''}` : 'Start a chat about this'"
      @click="onClick"
    >
      <MessageSquare v-if="chats.length" :size="12" />
      <MessageSquarePlus v-else :size="12" />
      <span v-if="chats.length">{{ chats.length }}</span>
      <span v-else>Chat</span>
    </button>

    <div
      v-if="open"
      class="absolute right-0 top-full mt-1 w-64 rounded-xl border border-border-strong bg-surface shadow-xl z-30 p-1.5"
    >
      <p class="px-2 pt-1 pb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Linked chats</p>
      <button
        v-for="c in chats"
        :key="c.id"
        class="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-sm text-fg hover:bg-elevated transition-colors cursor-pointer text-left"
        @click="openChat(c.id)"
      >
        <MessageSquare :size="13" class="shrink-0 text-fg-subtle" />
        <span class="flex-1 truncate">{{ c.title }}</span>
        <span class="shrink-0 text-[11px] text-fg-subtle">{{ c.messages.length ? `${c.messages.length} msgs` : 'empty' }}</span>
      </button>
      <div class="mt-1 pt-1 border-t border-border">
        <button
          class="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-sm text-accent-fg hover:bg-elevated transition-colors cursor-pointer"
          @click="startNew"
        >
          <MessageSquarePlus :size="13" class="shrink-0" />
          New chat
        </button>
      </div>
    </div>
  </div>
</template>
