import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Message } from '@/types'

interface ToolUsePayload {
  name: string
  input: Record<string, string>
}

export function useClaudeStream() {
  const isStreaming = ref(false)
  const toolActivity = ref<string | null>(null)

  let unlisten: UnlistenFn[] = []

  async function cleanup() {
    unlisten.forEach((fn) => fn())
    unlisten = []
    toolActivity.value = null
  }

  async function streamMessage(
    apiKey: string,
    messages: Message[],
    model: string,
    onDelta: (text: string) => void,
    onDone: () => void,
    onError: (err: string) => void,
    system?: string,
    projectPath?: string,
    dbConnectionString?: string,
  ) {
    await cleanup()
    isStreaming.value = true

    unlisten.push(
      await listen<string>('claude:delta', (e) => onDelta(e.payload)),
      await listen<ToolUsePayload>('claude:tool_use', (e) => {
        const { name, input } = e.payload
        if (name === 'read_file') {
          toolActivity.value = `Reading ${input.path}`
        } else if (name === 'list_files') {
          toolActivity.value = `Listing ${input.path ?? '.'}`
        } else if (name === 'search_code') {
          toolActivity.value = `Searching for "${input.pattern}"`
        } else if (name === 'get_database_schema') {
          toolActivity.value = 'Reading database schema'
        } else if (name === 'query_database') {
          toolActivity.value = 'Querying database'
        } else {
          toolActivity.value = name
        }
      }),
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
        projectPath: projectPath ?? null,
        dbConnectionString: dbConnectionString ?? null,
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

  return { isStreaming, toolActivity, streamMessage, cancel }
}
