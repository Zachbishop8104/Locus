import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { Conversation, Message } from '@/types'

export const useChatStore = defineStore('chat', () => {
  const conversations = ref<Conversation[]>([])
  const activeId = ref<string | null>(null)

  const activeConversation = computed(
    () => conversations.value.find((c) => c.id === activeId.value) ?? null,
  )

  function newConversation(projectId?: string): string {
    const id = crypto.randomUUID()
    conversations.value.unshift({
      id,
      title: 'New Chat',
      messages: [],
      createdAt: new Date().toISOString(),
      projectId,
    })
    activeId.value = id
    return id
  }

  function selectConversation(id: string) {
    activeId.value = id
  }

  function addMessage(conversationId: string, msg: Omit<Message, 'id'>): string {
    const conv = conversations.value.find((c) => c.id === conversationId)
    if (!conv) return ''
    const id = crypto.randomUUID()
    conv.messages.push({ ...msg, id })
    if (conv.title === 'New Chat' && msg.role === 'user') {
      conv.title = msg.content.length > 50 ? msg.content.slice(0, 50) + '…' : msg.content
    }
    return id
  }

  function appendToLastMessage(conversationId: string, text: string) {
    const conv = conversations.value.find((c) => c.id === conversationId)
    if (!conv || conv.messages.length === 0) return
    conv.messages[conv.messages.length - 1].content += text
  }

  function markLastMessageCancelled(conversationId: string) {
    const conv = conversations.value.find((c) => c.id === conversationId)
    if (!conv || conv.messages.length === 0) return
    const last = conv.messages[conv.messages.length - 1]
    if (last.role === 'assistant') last.cancelled = true
  }

  function removeLastMessage(conversationId: string) {
    const conv = conversations.value.find((c) => c.id === conversationId)
    if (!conv || conv.messages.length === 0) return
    if (conv.messages[conv.messages.length - 1].role === 'assistant') {
      conv.messages.pop()
    }
  }

  function markLastMessageError(conversationId: string) {
    const conv = conversations.value.find((c) => c.id === conversationId)
    if (!conv || conv.messages.length === 0) return
    conv.messages[conv.messages.length - 1].error = true
  }

  function renameConversation(id: string, title: string) {
    const conv = conversations.value.find((c) => c.id === id)
    if (conv) conv.title = title
  }

  function deleteConversation(id: string) {
    conversations.value = conversations.value.filter((c) => c.id !== id)
    if (activeId.value === id) {
      activeId.value = conversations.value[0]?.id ?? null
    }
  }

  function conversationsForProject(projectId: string): Conversation[] {
    return conversations.value
      .filter(c => c.projectId === projectId)
      .sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
  }

  return {
    conversations,
    activeId,
    activeConversation,
    newConversation,
    selectConversation,
    addMessage,
    appendToLastMessage,
    removeLastMessage,
    markLastMessageCancelled,
    markLastMessageError,
    renameConversation,
    deleteConversation,
    conversationsForProject,
  }
}, { persist: true })
