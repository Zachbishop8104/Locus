<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import ChatView from '@/views/ChatView.vue'
import DiffPanel from '@/components/chat/DiffPanel.vue'
import { useChatStore } from '@/stores/chat'
import { usePendingEditStore } from '@/stores/pendingEdit'

/**
 * A board card's chat, over the board. The chat is a normal conversation, so it is
 * also listed in the sidebar and can be opened full-size in Chat.
 */
const props = defineProps<{ conversationId: string }>()
const emit = defineEmits<{ close: [] }>()

const router = useRouter()
const chat = useChatStore()
const pendingEdit = usePendingEditStore()

// ChatView shows the active conversation.
chat.selectConversation(props.conversationId)

function close() {
  // Don't leave unused chats behind in the sidebar.
  const conv = chat.conversations.find((c) => c.id === props.conversationId)
  if (conv && conv.messages.length === 0) chat.deleteConversation(conv.id)
  emit('close')
}

function expand() {
  emit('close')
  router.push('/chat')
}

function onKey(e: KeyboardEvent) {
  // Escape belongs to the edit review while one is open.
  if (e.key === 'Escape' && !pendingEdit.request) close()
}
onMounted(() => document.addEventListener('keydown', onKey))
onBeforeUnmount(() => document.removeEventListener('keydown', onKey))
</script>

<template>
  <div class="fixed inset-0 z-40 flex items-center justify-center p-6 bg-black/40" @mousedown.self="close">
    <div class="relative w-full max-w-4xl h-full max-h-[52rem] flex flex-col rounded-2xl border border-border-strong bg-bg shadow-2xl overflow-hidden">
      <ChatView embedded @close="close" @expand="expand" />
      <!-- The page's own diff panel is under this overlay, so edits are reviewed here. -->
      <DiffPanel />
    </div>
  </div>
</template>
