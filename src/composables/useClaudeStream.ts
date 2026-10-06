import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { usePendingEditStore } from '@/stores/pendingEdit'
import type { EditRequest, BackendProvider, Effort, PermissionMode } from '@/types'

interface ToolUsePayload {
  name: string
  input: Record<string, string>
}

export interface StreamRequest {
  provider: BackendProvider
  apiKey: string
  model: string
  effort: Effort | null
  mode: PermissionMode
  messages: { role: 'user' | 'assistant'; content: string }[]
  system?: string
  projectPath?: string
  dbConnectionString?: string
  onDelta: (text: string) => void
  onDone: () => void
  onError: (err: string) => void
  onNewTurn: () => void
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

  async function streamMessage(req: StreamRequest) {
    const pendingEdit = usePendingEditStore()
    pendingEdit.registerCallbacks(cancel, req.onDelta)
    await cleanup()
    isStreaming.value = true

    unlisten.push(
      await listen<string>('claude:delta', (e) => {
        if (toolActivity.value === 'Thinking…') toolActivity.value = null
        req.onDelta(e.payload)
      }),

      await listen<void>('claude:thinking', () => {
        toolActivity.value = 'Thinking…'
      }),

      await listen<ToolUsePayload>('claude:tool_use', (e) => {
        const { name, input } = e.payload
        if (name === 'read_file') toolActivity.value = `Reading ${input.path}`
        else if (name === 'list_files') toolActivity.value = `Listing ${input.path || '.'}`
        else if (name === 'search_code') toolActivity.value = `Searching for "${input.pattern}"`
        else if (name === 'write_file') toolActivity.value = `Writing ${input.path}`
        else if (name === 'edit_file') toolActivity.value = `Editing ${input.path}`
        else if (name === 'get_database_schema') toolActivity.value = 'Reading database schema'
        else if (name === 'query_database') toolActivity.value = 'Querying database'
        else toolActivity.value = name
      }),

      await listen<{ filePath: string }>('claude:file_written', (e) => {
        req.onDelta(`\n\n*Edited \`${e.payload.filePath}\`*\n\n`)
      }),

      await listen<EditRequest>('claude:edit_request', (e) => {
        toolActivity.value = null
        pendingEdit.set(e.payload)
      }),

      await listen<void>('claude:new_turn', () => {
        req.onNewTurn()
      }),

      await listen<void>('claude:done', async () => {
        isStreaming.value = false
        await cleanup()
        req.onDone()
      }),

      await listen<string>('claude:error', async (e) => {
        isStreaming.value = false
        await cleanup()
        req.onError(e.payload)
      }),
    )

    try {
      await invoke('stream_message', {
        provider: req.provider,
        apiKey: req.apiKey,
        messages: req.messages,
        model: req.model,
        effort: req.effort,
        mode: req.mode,
        system: req.system ?? null,
        projectPath: req.projectPath ?? null,
        dbConnectionString: req.dbConnectionString ?? null,
      })
    } catch (e) {
      // Errors already emitted as claude:error have cleaned up; this covers
      // failures before streaming started (e.g. bad arguments).
      if (isStreaming.value) {
        isStreaming.value = false
        await cleanup()
        req.onError(String(e))
      }
    }
  }

  async function cancel() {
    await cleanup()
    isStreaming.value = false
    usePendingEditStore().clear()
    await invoke('cancel_stream').catch(() => {})
  }

  return { isStreaming, toolActivity, streamMessage, cancel }
}
