<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { AlertTriangle, ClipboardList } from '@lucide/vue'
import MessageList from '@/components/chat/MessageList.vue'
import MessageInput from '@/components/chat/MessageInput.vue'
import { useChatStore } from '@/stores/chat'
import { useSettingsStore } from '@/stores/settings'
import { useProjectsStore } from '@/stores/projects'
import { usePendingEditStore } from '@/stores/pendingEdit'
import { useClaudeStream } from '@/composables/useClaudeStream'
import type { PermissionMode, Project } from '@/types'

const router = useRouter()
const chat = useChatStore()
const settings = useSettingsStore()
const projects = useProjectsStore()
const pendingEdit = usePendingEditStore()
const { isStreaming, toolActivity, streamMessage, cancel } = useClaudeStream()
const input = ref<InstanceType<typeof MessageInput> | null>(null)

/** Conversation whose last response was a plan waiting for approval. */
const planReadyFor = ref<string | null>(null)

const streamingMessageId = computed(() => {
  if (!isStreaming.value || !chat.activeConversation) return null
  // Always highlight the last message while streaming, regardless of which turn it is
  const msgs = chat.activeConversation.messages
  for (let i = msgs.length - 1; i >= 0; i--) {
    if (msgs[i].role === 'assistant') return msgs[i].id
  }
  return null
})

const activeProject = computed<Project | undefined>(() =>
  chat.activeConversation?.projectId
    ? projects.getById(chat.activeConversation.projectId)
    : undefined
)

const showPlanApproval = computed(
  () => !isStreaming.value && !!planReadyFor.value && planReadyFor.value === chat.activeId,
)

watch(
  () => settings.loaded,
  (loaded) => {
    if (loaded && !chat.activeId) chat.newConversation()
  },
  { immediate: true },
)

// Mode changes apply to a running response too (the backend reads it live).
// Switching to auto-accept also approves an edit that is waiting.
watch(
  () => settings.mode,
  (mode) => {
    invoke('set_mode', { mode }).catch(() => {})
    if (mode === 'edits' && pendingEdit.request) pendingEdit.resolve(true)
  },
)

function buildSystemPrompt(project: Project): string {
  const lines = [
    `You are helping with the project "${project.name}".`,
  ]
  if (project.description) lines.push(`\n${project.description}`)
  const context: string[] = []
  if (project.localPath) context.push(`Local path: ${project.localPath}`)
  if (project.gitRepo) context.push(`Git repository: ${project.gitRepo}`)
  if (project.jiraProject) context.push(`Jira project: ${project.jiraProject}`)
  if (context.length) lines.push('\n' + context.join('\n'))
  return lines.join('\n')
}

async function autoRenameConversation(convId: string) {
  const conv = chat.conversations.find((c) => c.id === convId)
  if (!conv || conv.messages.length !== 2 || !settings.canChat) return
  try {
    const title = await invoke<string>('generate_title', {
      provider: settings.backendProvider,
      apiKey: settings.apiKey,
      model: settings.activeModelId,
      userMessage: conv.messages[0].content,
      assistantMessage: conv.messages[1].content,
    })
    if (title) chat.renameConversation(convId, title)
  } catch {
    // keep the truncated fallback title
  }
}

async function sendMessage(text: string) {
  if (!settings.canChat) {
    router.push('/settings')
    return
  }

  let convId = chat.activeId
  if (!convId) convId = chat.newConversation()
  planReadyFor.value = null
  const mode = settings.mode

  chat.addMessage(convId, { role: 'user', content: text, timestamp: new Date().toISOString() })
  chat.addMessage(convId, { role: 'assistant', content: '', timestamp: new Date().toISOString() })

  const messages = chat.activeConversation!.messages
    .slice(0, -1)
    .filter((m) => !m.cancelled && !m.error && m.content.trim())
    .map((m) => ({ role: m.role, content: m.content }))

  await streamMessage({
    provider: settings.backendProvider,
    apiKey: settings.apiKey,
    model: settings.activeModelId,
    effort: settings.activeEffort,
    mode,
    messages,
    system: activeProject.value ? buildSystemPrompt(activeProject.value) : undefined,
    projectPath: activeProject.value?.localPath,
    dbConnectionString: activeProject.value?.dbConnectionString,
    onDelta: (delta) => chat.appendToLastMessage(convId!, delta),
    onDone: () => {
      autoRenameConversation(convId!)
      if (mode === 'plan' && settings.mode === 'plan') planReadyFor.value = convId
    },
    onError: (err) => {
      chat.appendToLastMessage(convId!, err)
      chat.markLastMessageError(convId!)
    },
    onNewTurn: () => {
      // Only open a new bubble if the current one has content.
      // If the model went straight to a tool call the bubble is empty — reuse it.
      const msgs = chat.activeConversation?.messages ?? []
      const last = msgs[msgs.length - 1]
      if (last?.role === 'assistant' && last.content.trim()) {
        chat.addMessage(convId!, { role: 'assistant', content: '', timestamp: new Date().toISOString() })
      }
    },
  })
}

function approvePlan(mode: PermissionMode) {
  settings.mode = mode
  sendMessage('The plan is approved. Go ahead and implement it.')
}

function keepPlanning() {
  planReadyFor.value = null
  input.value?.focus()
}

function handleCancel() {
  cancel()
  // Remove the partial assistant message so it doesn't get sent as context
  // in the next message — otherwise the model "picks up where it left off".
  if (chat.activeId) chat.removeLastMessage(chat.activeId)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- No API key warning -->
    <div
      v-if="settings.loaded && !settings.canChat"
      class="flex items-center gap-2 mx-6 mt-4 px-4 py-3 rounded-lg bg-amber-50 border border-amber-200 text-amber-700 text-sm"
    >
      <AlertTriangle :size="15" class="shrink-0" />
      <span>
        No model connected.
        <button class="underline hover:no-underline cursor-pointer" @click="router.push('/settings')">
          Connect Claude or a local model in Settings
        </button>
        to start chatting.
      </span>
    </div>

    <!-- Header -->
    <div class="flex items-center justify-between px-6 py-4 border-b border-border/60 shrink-0">
      <div class="flex items-center gap-3 min-w-0">
        <!-- Project badge -->
        <div v-if="activeProject" class="flex items-center gap-1.5 shrink-0">
          <div class="w-2.5 h-2.5 rounded-full" :style="{ background: activeProject.color }" />
          <span class="text-xs font-medium text-fg-subtle">{{ activeProject.name }}</span>
          <span class="text-fg-faint text-xs">/</span>
        </div>
        <h1 class="text-sm font-semibold text-fg truncate">
          {{ chat.activeConversation?.title ?? 'New Chat' }}
        </h1>
      </div>
      <p class="text-xs text-fg-subtle shrink-0 ml-4">
        {{ settings.activeModelLabel }}<span v-if="settings.activeProvider === 'claude'"> · {{ settings.useLocalClaude ? 'Claude Code' : 'API' }}</span>
      </p>
    </div>

    <!-- Messages -->
    <MessageList
      :messages="chat.activeConversation?.messages ?? []"
      :streaming-id="streamingMessageId"
      :tool-activity="toolActivity"
    />

    <!-- Plan approval -->
    <div
      v-if="showPlanApproval"
      class="mx-6 mb-1 mt-2 flex flex-wrap items-center gap-2 px-4 py-3 rounded-xl border border-sky-500/40 bg-sky-50"
    >
      <ClipboardList :size="15" class="text-sky-700 shrink-0" />
      <span class="text-sm font-medium text-sky-900 mr-auto">Ready to implement this plan?</span>
      <button
        class="px-3 py-1.5 rounded-lg text-xs font-medium bg-accent hover:bg-accent-light text-white transition-colors cursor-pointer"
        @click="approvePlan('edits')"
      >
        Yes, auto-accept edits
      </button>
      <button
        class="px-3 py-1.5 rounded-lg text-xs font-medium border border-border-strong bg-surface text-fg hover:bg-elevated transition-colors cursor-pointer"
        @click="approvePlan('ask')"
      >
        Yes, ask before each edit
      </button>
      <button
        class="px-3 py-1.5 rounded-lg text-xs font-medium text-fg-muted hover:text-fg transition-colors cursor-pointer"
        @click="keepPlanning"
      >
        Keep planning
      </button>
    </div>

    <!-- Input -->
    <MessageInput
      ref="input"
      :disabled="!settings.canChat"
      :streaming="isStreaming"
      @submit="sendMessage"
      @cancel="handleCancel"
    />
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
