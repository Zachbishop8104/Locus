import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Message } from '@/types'

export function useClaudeStream() {
  const isStreaming = ref(false)

  let unlisten: UnlistenFn[] = []

  async function cleanup() {
    unlisten.forEach((fn) => fn())
    unlisten = []
  }

  async function streamMessage(
    apiKey: string,
    messages: Message[],
    model: string,
    onDelta: (text: string) => void,
    onDone: () => void,
    onError: (err: string) => void,
    system?: string,
  ) {
    await cleanup()
    isStreaming.value = true

    unlisten.push(
      await listen<string>('claude:delta', (e) => onDelta(e.payload)),
      await listen<void>('claude:done', async () => {
        isStreaming.value = false
        await cleanup()
        onDone()
      }),
      await listen<string>('claude:error', async (e) => {
        isStreaming.value = false
        await cleanup()
        onError(e.payload)
      }),
    )

    try {
      await invoke('stream_message', {
        apiKey,
        messages: messages.map((m) => ({ role: m.role, content: m.content })),
        model,
        system: system ?? null,
      })
    } catch (e) {
      isStreaming.value = false
      await cleanup()
      onError(String(e))
    }
  }

  async function cancel() {
    await cleanup()
    isStreaming.value = false
  }

  return { isStreaming, streamMessage, cancel }
}
