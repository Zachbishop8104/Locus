<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { AlertTriangle } from '@lucide/vue'
import MessageList from '@/components/chat/MessageList.vue'
import MessageInput from '@/components/chat/MessageInput.vue'
import EditApprovalCard from '@/components/chat/EditApprovalCard.vue'
import { useChatStore } from '@/stores/chat'
import { useSettingsStore } from '@/stores/settings'
import { useProjectsStore } from '@/stores/projects'
import { useClaudeStream } from '@/composables/useClaudeStream'
import type { Project, EditRequest } from '@/types'

const router = useRouter()
const chat = useChatStore()
const settings = useSettingsStore()
const projects = useProjectsStore()
const { isStreaming, toolActivity, streamMessage, cancel } = useClaudeStream()

const editRequest = ref<EditRequest | null>(null)
let resolveEdit: ((approved: boolean) => void) | null = null

const streamingMessageId = computed(() => {
  if (!isStreaming.value || !chat.activeConversation) return null
  const msgs = chat.activeConversation.messages
  const last = msgs[msgs.length - 1]
  return last?.role === 'assistant' ? last.id : null
})

const activeProject = computed<Project | undefined>(() =>
  chat.activeConversation?.projectId
    ? projects.getById(chat.activeConversation.projectId)
    : undefined
)

watch(
  () => settings.loaded,
  (loaded) => {
    if (loaded && !chat.activeId) chat.newConversation()
  },
  { immediate: true },
)

function buildSystemPrompt(project: Project): string {
  const lines = [
    `You are a developer assistant helping with the project "${project.name}".`,
  ]
  if (project.description) lines.push(`\n${project.description}`)
  const context: string[] = []
  if (project.localPath) context.push(`Local path: ${project.localPath}`)
  if (project.gitRepo) context.push(`Git repository: ${project.gitRepo}`)
  if (project.jiraProject) context.push(`Jira project: ${project.jiraProject}`)
  if (context.length) lines.push('\n' + context.join('\n'))
  if (project.localPath && !settings.useLocalClaude) {
    lines.push('\nWhen making any file change, call write_file immediately — never describe the change in text or say "go ahead and approve". The UI handles approval automatically.')
  }
  return lines.join('\n')
}

async function autoRenameConversation(convId: string) {
  const conv = chat.conversations.find((c) => c.id === convId)
  if (!conv || conv.messages.length !== 2 || !settings.apiKey) return
  try {
    const title = await invoke<string>('generate_title', {
      apiKey: settings.apiKey,
      userMessage: conv.messages[0].content,
      assistantMessage: conv.messages[1].content,
    })
    if (title) chat.renameConversation(convId, title)
  } catch {
    // keep the truncated fallback title
  }
}

function handleEditRequest(req: EditRequest): Promise<boolean> {
  if (settings.autoApproveEdits) return Promise.resolve(true)
  return new Promise((resolve) => {
    editRequest.value = req
    resolveEdit = resolve
  })
}

function approveEdit() {
  editRequest.value = null
  resolveEdit?.(true)
  resolveEdit = null
}

function denyEdit() {
  editRequest.value = null
  resolveEdit?.(false)
  resolveEdit = null
}

async function sendMessage(text: string) {
  if (!settings.apiKey) {
    router.push('/settings')
    return
  }

  let convId = chat.activeId
  if (!convId) convId = chat.newConversation()

  chat.addMessage(convId, { role: 'user', content: text, timestamp: new Date().toISOString() })
  chat.addMessage(convId, { role: 'assistant', content: '', timestamp: new Date().toISOString() })

  const messages = chat.activeConversation!.messages
    .slice(0, -1)
    .map((m) => ({ role: m.role, content: m.content }))

  const system = activeProject.value ? buildSystemPrompt(activeProject.value) : undefined

  await streamMessage(
    settings.apiKey,
    messages as Parameters<typeof streamMessage>[1],
    settings.model,
    (delta) => chat.appendToLastMessage(convId!, delta),
    () => autoRenameConversation(convId!),
    (err) => {
      chat.appendToLastMessage(convId!, err)
      chat.markLastMessageError(convId!)
    },
    handleEditRequest,
    system,
    activeProject.value?.localPath,
    activeProject.value?.dbConnectionString,
  )
}

function handleCancel() {
  denyEdit()
  cancel()
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- No API key warning -->
    <div
      v-if="settings.loaded && !settings.apiKey"
      class="flex items-center gap-2 mx-6 mt-4 px-4 py-3 rounded-lg bg-amber-50 border border-amber-200 text-amber-700 text-sm"
    >
      <AlertTriangle :size="15" class="shrink-0" />
      <span>
        No API key configured.
        <button class="underline hover:no-underline cursor-pointer" @click="router.push('/settings')">
          Add one in Settings
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
      <div class="flex items-center gap-3 shrink-0 ml-4">
        <Transition name="fade">
          <span v-if="toolActivity" class="flex items-center gap-1.5 text-xs text-fg-subtle">
            <span class="inline-block w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
            {{ toolActivity }}
          </span>
        </Transition>
        <p class="text-xs text-fg-subtle">{{ settings.model }}</p>
      </div>
    </div>

    <!-- Messages -->
    <MessageList
      :messages="chat.activeConversation?.messages ?? []"
      :streaming-id="streamingMessageId"
    />

    <!-- Inline edit approval card -->
    <Transition name="card-slide">
      <EditApprovalCard
        v-if="editRequest"
        :request="editRequest"
        @approve="approveEdit"
        @deny="denyEdit"
      />
    </Transition>

    <!-- Input -->
    <MessageInput
      :disabled="!settings.apiKey"
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

.card-slide-enter-active,
.card-slide-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.card-slide-enter-from,
.card-slide-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>
