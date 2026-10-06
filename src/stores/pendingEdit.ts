import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useChatStore } from '@/stores/chat'
import type { EditRequest } from '@/types'

export const usePendingEditStore = defineStore('pendingEdit', () => {
  const request = ref<EditRequest | null>(null)
  let cancelFn: (() => void) | null = null
  let appendFn: ((text: string) => void) | null = null

  function registerCallbacks(cancel: () => void, append: (text: string) => void) {
    cancelFn = cancel
    appendFn = append
  }

  function set(req: EditRequest) {
    request.value = req
  }

  function clear() {
    request.value = null
  }

  async function resolve(approved: boolean) {
    if (!request.value) return
    const { toolUseId, filePath } = request.value
    request.value = null
    await invoke('confirm_write', { toolUseId, approved }).catch(() => {})
    if (!approved) {
      appendFn?.(`*Proposed edit to \`${filePath}\` — rejected.*`)
      // Mark the message so it's excluded from future API context
      const chat = useChatStore()
      if (chat.activeId) chat.markLastMessageCancelled(chat.activeId)
      cancelFn?.()
      cancelFn = null
      appendFn = null
    }
  }

  return { request, set, clear, resolve, registerCallbacks }
})
