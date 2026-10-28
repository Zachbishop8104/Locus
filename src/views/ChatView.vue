<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { AlertTriangle, Check, ClipboardList, FolderOpen, GitPullRequest, ListTodo, Maximize2, Ticket, X } from '@lucide/vue'
import MessageList from '@/components/chat/MessageList.vue'
import MessageInput from '@/components/chat/MessageInput.vue'
import { useChatStore } from '@/stores/chat'
import { useSettingsStore } from '@/stores/settings'
import { useProjectsStore } from '@/stores/projects'
import { usePendingEditStore } from '@/stores/pendingEdit'
import { useClaudeStream } from '@/composables/useClaudeStream'
import type { PermissionMode, Project, Skill, TaskRef } from '@/types'

/** `embedded`: shown in a modal over another page (the board) rather than as its own page. */
defineProps<{ embedded?: boolean }>()
const emit = defineEmits<{ close: []; expand: [] }>()

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

/** Clone folders configured for the active project. */
const projectPaths = computed<string[]>(() =>
  (activeProject.value?.localPaths ?? []).filter((p) => p.trim()),
)

/**
 * The folders this chat works in: the ones saved on the conversation that are
 * still in the project, otherwise the project's first folder. Without project
 * folders (e.g. a chat started from a task-master folder on the board), the
 * conversation's own folders are used.
 */
const activeFolders = computed<string[]>(() => {
  const paths = projectPaths.value
  if (!paths.length) return (chat.activeConversation?.localPaths ?? []).filter((p) => p.trim())
  const saved = (chat.activeConversation?.localPaths ?? []).filter((p) => paths.includes(p))
  return saved.length ? saved : [paths[0]]
})

/** Add or remove a folder from this chat. At least one stays selected. */
function toggleFolder(path: string) {
  const conv = chat.activeConversation
  if (!conv) return
  const current = activeFolders.value
  if (current.includes(path)) {
    if (current.length === 1) return
    conv.localPaths = current.filter((p) => p !== path)
  } else {
    // Keep the project's folder order.
    conv.localPaths = projectPaths.value.filter((p) => p === path || current.includes(p))
  }
}

// ── Skills (the / menu) ──

const skills = ref<Skill[]>([])
watch(
  () => activeFolders.value.join('|'),
  async () => {
    try {
      skills.value = await invoke<Skill[]>('list_skills', { projectPaths: activeFolders.value })
    } catch {
      skills.value = []
    }
  },
  { immediate: true },
)

/**
 * `/name args` → what the model receives: a command's text with `$ARGUMENTS` filled in,
 * or a skill's instructions followed by the request. Undefined when it isn't a known skill.
 */
async function expandSkill(text: string): Promise<string | undefined> {
  const m = text.match(/^\/(\S+)(?:\s+([\s\S]*))?$/)
  const skill = m && skills.value.find((s) => s.name.toLowerCase() === m[1].toLowerCase())
  if (!m || !skill) return undefined
  const args = (m[2] ?? '').trim()
  let body: string
  try {
    body = await invoke<string>('load_skill', { path: skill.path })
  } catch {
    return undefined
  }
  if (skill.kind === 'command') {
    if (body.includes('$ARGUMENTS')) return body.split('$ARGUMENTS').join(args)
    return args ? `${body}\n\nARGUMENTS: ${args}` : body
  }
  const dir = skill.path.replace(/[\/]SKILL\.md$/i, '')
  return [
    `Use the "${skill.name}" skill for this request. Follow its instructions; files it mentions are relative to ${dir}.`,
    '',
    `<skill name="${skill.name}">`,
    body,
    '</skill>',
    '',
    args ? `Request: ${args}` : 'No further details were given — follow the skill as written.',
  ].join('\n')
}

function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

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

const TASK_KIND: Record<TaskRef['source'], string> = {
  jira: 'Jira issue',
  taskmaster: 'task-master task',
  pr: 'pull request',
}

function buildSystemPrompt(project: Project | undefined, localPaths: string[], task: TaskRef | undefined): string | undefined {
  const lines: string[] = []
  if (project) {
    lines.push(`You are helping with the project "${project.name}".`)
    if (project.description) lines.push(`\n${project.description}`)
    const context: string[] = []
    if (localPaths.length === 1) context.push(`Local path: ${localPaths[0]}`)
    if (project.gitRepo?.trim()) context.push(`Git repository: ${project.gitRepo.trim()}`)
    if (project.jiraProject) context.push(`Jira project: ${project.jiraProject}`)
    if (context.length) lines.push('\n' + context.join('\n'))
  } else if (localPaths.length === 1) {
    lines.push(`Local path: ${localPaths[0]}`)
  }
  if (task) {
    lines.push(`\nThis chat is about ${TASK_KIND[task.source]} ${task.label}: "${task.title}"${task.url ? ` (${task.url})` : ''}.`)
  }
  return lines.length ? lines.join('\n') : undefined
}

// A chat started from the board arrives with its first message drafted.
// Also watches the input, which isn't mounted yet when arriving from the board.
watch(
  [() => chat.activeConversation?.draft, input],
  ([draft, box]) => {
    const conv = chat.activeConversation
    if (!draft || !conv || !box) return
    box.setText(draft)
    delete conv.draft
  },
  { immediate: true, flush: 'post' },
)

function openTaskUrl(url: string | undefined) {
  if (url) invoke('open_url', { url }).catch(() => {})
}

async function autoRenameConversation(convId: string) {
  const conv = chat.conversations.find((c) => c.id === convId)
  // Chats started from a board card keep the card's title.
  if (!conv || conv.task || conv.messages.length !== 2 || !settings.canChat) return
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

  const sent = await expandSkill(text)
  chat.addMessage(convId, { role: 'user', content: text, sent, timestamp: new Date().toISOString() })
  chat.addMessage(convId, { role: 'assistant', content: '', timestamp: new Date().toISOString() })

  const messages = chat.activeConversation!.messages
    .slice(0, -1)
    .filter((m) => !m.cancelled && !m.error && m.content.trim())
    .map((m) => ({ role: m.role, content: m.sent ?? m.content }))

  await streamMessage({
    provider: settings.backendProvider,
    apiKey: settings.apiKey,
    model: settings.activeModelId,
    effort: settings.activeEffort,
    mode,
    messages,
    system: buildSystemPrompt(activeProject.value, activeFolders.value, chat.activeConversation?.task),
    projectPaths: activeFolders.value,
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
        <!-- Linked board card -->
        <button
          v-if="chat.activeConversation?.task"
          class="flex items-center gap-1 shrink-0 px-2 py-0.5 rounded-full border border-border text-[11px] font-medium text-fg-muted transition-colors"
          :class="chat.activeConversation.task.url ? 'hover:text-fg hover:border-border-strong cursor-pointer' : 'cursor-default'"
          :title="chat.activeConversation.task.url ? `Open ${chat.activeConversation.task.label}` : chat.activeConversation.task.title"
          @click="openTaskUrl(chat.activeConversation.task.url)"
        >
          <Ticket v-if="chat.activeConversation.task.source === 'jira'" :size="11" class="text-sky-600" />
          <GitPullRequest v-else-if="chat.activeConversation.task.source === 'pr'" :size="11" class="text-accent-icon" />
          <ListTodo v-else :size="11" class="text-accent-icon" />
          {{ chat.activeConversation.task.label }}
        </button>
      </div>
      <div class="flex items-center gap-1 shrink-0 ml-4">
        <p class="text-xs text-fg-subtle">
          {{ settings.activeModelLabel }}<span v-if="settings.activeProvider === 'claude'"> · {{ settings.useLocalClaude ? 'Claude Code' : 'API' }}</span>
        </p>
        <template v-if="embedded">
          <button
            class="ml-2 p-1.5 rounded-lg text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
            title="Open in Chat"
            @click="emit('expand')"
          >
            <Maximize2 :size="14" />
          </button>
          <button
            class="p-1.5 rounded-lg text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
            title="Close (Esc)"
            @click="emit('close')"
          >
            <X :size="15" />
          </button>
        </template>
      </div>
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

    <!-- Folder picker -->
    <div
      v-if="projectPaths.length > 1"
      class="flex flex-wrap items-center gap-1.5 px-6 pt-3"
    >
      <span class="text-xs text-fg-subtle mr-0.5">Folders:</span>
      <button
        v-for="p in projectPaths"
        :key="p"
        :title="activeFolders.includes(p) && activeFolders.length === 1 ? `${p} (at least one folder is required)` : p"
        :disabled="isStreaming"
        class="flex items-center gap-1 px-2 py-1 rounded-md border text-xs font-medium transition-colors cursor-pointer disabled:cursor-not-allowed disabled:opacity-60"
        :class="activeFolders.includes(p)
          ? 'border-accent/50 bg-accent/10 text-fg'
          : 'border-border text-fg-subtle hover:text-fg hover:border-border-strong'"
        @click="toggleFolder(p)"
      >
        <Check v-if="activeFolders.includes(p)" :size="11" class="text-accent" />
        <FolderOpen v-else :size="11" />
        {{ folderName(p) }}
      </button>
    </div>

    <!-- Input -->
    <MessageInput
      ref="input"
      :skills="skills"
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
