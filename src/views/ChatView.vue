<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { AlertTriangle } from '@lucide/vue'
import MessageList from '@/components/chat/MessageList.vue'
import MessageInput from '@/components/chat/MessageInput.vue'
import { useChatStore } from '@/stores/chat'
import { useSettingsStore } from '@/stores/settings'
import { useProjectsStore } from '@/stores/projects'
import { useClaudeStream } from '@/composables/useClaudeStream'
import type { Project } from '@/types'

const router = useRouter()
const chat = useChatStore()
const settings = useSettingsStore()
const projects = useProjectsStore()
const { isStreaming, streamMessage, cancel } = useClaudeStream()

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
  return lines.join('\n')
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
    () => {},
    (err) => {
      chat.appendToLastMessage(convId!, err)
      chat.markLastMessageError(convId!)
    },
    system,
  )
}

function handleCancel() {
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
      <p class="text-xs text-fg-subtle shrink-0 ml-4">{{ settings.model }}</p>
    </div>

    <!-- Messages -->
    <MessageList
      :messages="chat.activeConversation?.messages ?? []"
      :streaming-id="streamingMessageId"
    />

    <!-- Input -->
    <MessageInput
      :disabled="!settings.apiKey"
      :streaming="isStreaming"
      @submit="sendMessage"
      @cancel="handleCancel"
    />
  </div>
</template>
