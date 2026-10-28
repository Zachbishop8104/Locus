<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import {
  X, ExternalLink, AlertCircle, CircleCheck, Circle, CircleDot, CircleSlash, CornerLeftUp, Link2, Tag, MessageSquare,
} from '@lucide/vue'
import ChatLinkButton from '@/components/board/ChatLinkButton.vue'
import { cardKind, priorityTag, renderMarkdown, statusTag, SOURCE_STRIPE } from '@/components/board/cardStyle'
import { useBoardStore, COLUMNS } from '@/stores/board'
import type { BoardCard, ColumnId } from '@/stores/board'
import type { JiraIssueDetail } from '@/types'

/** Everything inside a board card: description, details, subtasks, comments. */
const props = defineProps<{ card: BoardCard }>()
const emit = defineEmits<{ close: []; chat: [id: string] }>()

const board = useBoardStore()

// The card is re-read from the store so a move or refresh shows here.
const card = computed(() => board.cards.find((c) => c.key === props.card.key) ?? props.card)
const kind = computed(() => cardKind(card.value))
const tmTask = computed(() => board.tmTaskFor(card.value))

const jira = ref<JiraIssueDetail | null>(null)
const jiraLoading = ref(false)
const jiraError = ref<string | null>(null)

async function loadJira() {
  if (!card.value.jiraKey) return
  jiraLoading.value = true
  jiraError.value = null
  try {
    jira.value = await invoke<JiraIssueDetail>('get_jira_issue_detail', { key: card.value.jiraKey })
  } catch (e) {
    jiraError.value = String(e)
  } finally {
    jiraLoading.value = false
  }
}
watch(() => card.value.jiraKey, loadJira, { immediate: true })

const description = computed(() => card.value.source === 'jira' ? jira.value?.description ?? '' : card.value.description)

/** task-master dependencies, with titles from the same folder. */
const dependencies = computed(() => {
  const t = tmTask.value
  if (!t || !card.value.tmPath) return []
  const siblings = board.tmTasks[card.value.tmPath] ?? []
  return t.dependencies.map((id) => ({ id, task: siblings.find((s) => s.id === id) ?? null }))
})

const subtasksOpen = ref<Set<string>>(new Set())
function toggleSubtask(id: string) {
  const s = new Set(subtasksOpen.value)
  if (s.has(id)) s.delete(id); else s.add(id)
  subtasksOpen.value = s
}

const column = computed({
  get: () => card.value.column,
  set: (to: ColumnId) => board.moveCard(card.value, to),
})

function openUrl(url: string | null | undefined) {
  if (url) invoke('open_url', { url }).catch(() => {})
}

function jiraUrl(key: string) {
  return card.value.url?.replace(/\/browse\/[^/]+$/, `/browse/${key}`) ?? null
}

/** Links in rendered text open in the browser, never inside the app. */
function onProseClick(e: MouseEvent) {
  const a = (e.target as HTMLElement).closest('a')
  if (!a) return
  e.preventDefault()
  const href = a.getAttribute('href') ?? ''
  if (/^(https?:|mailto:)/i.test(href)) openUrl(href)
}

function formatDate(iso: string | null | undefined) {
  if (!iso) return ''
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}
onMounted(() => document.addEventListener('keydown', onKey))
onBeforeUnmount(() => document.removeEventListener('keydown', onKey))

const PROSE = 'prose prose-sm claude-prose max-w-none prose-code:before:content-none prose-code:after:content-none'
</script>

<template>
  <div class="fixed inset-0 z-40 flex items-center justify-center p-6 bg-black/40" @mousedown.self="emit('close')">
    <div class="relative w-full max-w-3xl max-h-full flex flex-col rounded-2xl border border-border-strong bg-bg shadow-2xl overflow-hidden">
      <span class="absolute left-0 top-0 bottom-0 w-1" :class="SOURCE_STRIPE[card.source]" />

      <!-- Header -->
      <div class="flex items-start gap-3 pl-7 pr-4 pt-5 pb-4 border-b border-border/60 shrink-0">
        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-2 text-xs text-fg-subtle">
            <component :is="kind.icon" :size="14" :class="kind.cls" />
            <span class="font-medium" :class="kind.cls">{{ kind.label }}</span>
            <button
              v-if="card.url"
              class="font-semibold text-fg-muted hover:underline cursor-pointer flex items-center gap-0.5"
              :title="`Open ${card.label} in Jira`"
              @click="openUrl(card.url)"
            >{{ card.label }}<ExternalLink :size="11" /></button>
            <span v-else class="font-semibold text-fg-muted">{{ card.label }}</span>
            <template v-if="jira?.parent">
              <span class="text-fg-faint">in</span>
              <button
                class="flex items-center gap-0.5 min-w-0 hover:underline cursor-pointer"
                :title="jira.parent.summary"
                @click="openUrl(jiraUrl(jira.parent.key))"
              >
                <CornerLeftUp :size="11" class="shrink-0" />
                <span class="truncate max-w-56">{{ jira.parent.key }} {{ jira.parent.summary }}</span>
              </button>
            </template>
          </div>
          <h2 class="mt-1.5 text-lg font-semibold text-fg leading-snug">{{ card.title }}</h2>
        </div>
        <button
          class="shrink-0 p-1.5 rounded-lg text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
          title="Close"
          @click="emit('close')"
        >
          <X :size="16" />
        </button>
      </div>

      <div class="flex-1 min-h-0 overflow-y-auto">
        <!-- Fields -->
        <dl class="grid grid-cols-[7rem_1fr] gap-x-4 gap-y-2.5 pl-7 pr-6 py-4 text-xs border-b border-border/60">
          <dt class="text-fg-subtle pt-1">Column</dt>
          <dd class="flex items-center gap-2">
            <select
              v-model="column"
              class="px-2 py-1 rounded-md border border-border bg-surface text-xs text-fg outline-none focus:border-accent-light/60 cursor-pointer disabled:opacity-50"
              :disabled="board.pending.has(card.key)"
            >
              <option v-for="c in COLUMNS" :key="c.id" :value="c.id">{{ c.label }}</option>
            </select>
            <span class="px-1.5 py-0.5 rounded border" :class="statusTag(card.status)">{{ card.status }}</span>
            <span v-if="card.flag" class="px-1.5 py-0.5 rounded border bg-amber-50 text-amber-700 border-amber-200">{{ card.flag }}</span>
          </dd>

          <template v-if="card.priority">
            <dt class="text-fg-subtle">Priority</dt>
            <dd>
              <span class="px-1.5 py-0.5 rounded border capitalize" :class="priorityTag(card.priority) ?? 'bg-elevated text-fg-muted border-border'">{{ card.priority }}</span>
            </dd>
          </template>

          <template v-if="card.project || card.group">
            <dt class="text-fg-subtle">{{ card.project ? 'Project' : card.source === 'jira' ? 'Jira project' : 'Folder' }}</dt>
            <dd class="flex items-center gap-1.5 text-fg">
              <span class="w-2 h-2 rounded-full" :class="card.project ? '' : 'bg-fg-faint'" :style="card.project ? { background: card.project.color } : undefined" />
              {{ card.project?.name ?? card.group }}
            </dd>
          </template>

          <template v-if="card.tmPath">
            <dt class="text-fg-subtle">Folder</dt>
            <dd class="text-fg-muted font-mono truncate" :title="card.tmPath">{{ card.tmPath }}</dd>
          </template>

          <template v-if="card.source === 'jira'">
            <dt class="text-fg-subtle">Assignee</dt>
            <dd class="text-fg">{{ card.assignee ?? 'Unassigned' }}</dd>
            <template v-if="jira?.reporter">
              <dt class="text-fg-subtle">Reporter</dt>
              <dd class="text-fg">{{ jira.reporter }}</dd>
            </template>
            <template v-if="jira?.created">
              <dt class="text-fg-subtle">Created</dt>
              <dd class="text-fg-muted">{{ formatDate(jira.created) }}</dd>
            </template>
            <template v-if="card.updated">
              <dt class="text-fg-subtle">Updated</dt>
              <dd class="text-fg-muted">{{ formatDate(card.updated) }}</dd>
            </template>
            <template v-if="jira?.labels.length">
              <dt class="text-fg-subtle">Labels</dt>
              <dd class="flex flex-wrap gap-1">
                <span v-for="l in jira.labels" :key="l" class="flex items-center gap-1 px-1.5 py-0.5 rounded border border-border bg-elevated text-fg-muted">
                  <Tag :size="10" />{{ l }}
                </span>
              </dd>
            </template>
          </template>

          <template v-if="dependencies.length">
            <dt class="text-fg-subtle pt-0.5">Depends on</dt>
            <dd class="flex flex-col gap-1">
              <span v-for="d in dependencies" :key="d.id" class="flex items-center gap-1.5 min-w-0">
                <Link2 :size="11" class="shrink-0 text-fg-faint" />
                <span class="font-medium text-fg-muted shrink-0">#{{ d.id }}</span>
                <span class="truncate text-fg">{{ d.task?.title ?? '' }}</span>
                <span v-if="d.task" class="shrink-0 px-1.5 rounded border" :class="statusTag(d.task.status)">{{ d.task.status }}</span>
              </span>
            </dd>
          </template>
        </dl>

        <div class="pl-7 pr-6 py-5 flex flex-col gap-6" @click="onProseClick">
          <!-- Description -->
          <section>
            <h3 class="text-[11px] font-semibold uppercase tracking-wide text-fg-subtle mb-2">Description</h3>
            <p v-if="card.source === 'jira' && jiraLoading && !jira" class="text-xs text-fg-faint">Loading…</p>
            <div
              v-else-if="card.source === 'jira' && jiraError"
              class="flex items-start gap-2 px-3 py-2 rounded-lg border border-border bg-surface text-xs text-fg-muted"
            >
              <AlertCircle :size="14" class="shrink-0 mt-0.5 text-amber-600" />
              <span class="flex-1 break-words">Couldn't load the issue: {{ jiraError }}</span>
              <button class="shrink-0 underline hover:no-underline cursor-pointer" @click="loadJira">Retry</button>
            </div>
            <div v-else-if="description && description !== card.title" :class="PROSE" v-html="renderMarkdown(description)" />
            <p v-else class="text-xs text-fg-faint">No description.</p>
          </section>

          <section v-if="tmTask?.details">
            <h3 class="text-[11px] font-semibold uppercase tracking-wide text-fg-subtle mb-2">Details</h3>
            <div :class="PROSE" v-html="renderMarkdown(tmTask.details)" />
          </section>

          <section v-if="tmTask?.testStrategy">
            <h3 class="text-[11px] font-semibold uppercase tracking-wide text-fg-subtle mb-2">Test strategy</h3>
            <div :class="PROSE" v-html="renderMarkdown(tmTask.testStrategy)" />
          </section>

          <!-- task-master subtasks -->
          <section v-if="tmTask?.subtasks.length">
            <h3 class="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle mb-2">
              Subtasks
              <span class="normal-case font-normal tracking-normal">{{ tmTask.subtasksDone }}/{{ tmTask.subtasksTotal }} done</span>
            </h3>
            <div class="h-1 rounded-full bg-elevated overflow-hidden mb-3">
              <div class="h-full bg-emerald-500 transition-all" :style="{ width: `${(tmTask.subtasksDone / tmTask.subtasksTotal) * 100}%` }" />
            </div>
            <ul class="flex flex-col rounded-lg border border-border divide-y divide-border overflow-hidden">
              <li v-for="s in tmTask.subtasks" :key="s.id" class="bg-surface">
                <button
                  class="w-full flex items-start gap-2 px-3 py-2 text-left hover:bg-elevated transition-colors cursor-pointer"
                  :disabled="!s.description && !s.details"
                  @click="toggleSubtask(s.id)"
                >
                  <CircleCheck v-if="s.status === 'done'" :size="14" class="shrink-0 mt-0.5 text-emerald-600" />
                  <CircleDot v-else-if="s.status === 'in-progress' || s.status === 'review'" :size="14" class="shrink-0 mt-0.5 text-sky-600" />
                  <CircleSlash v-else-if="s.status === 'blocked' || s.status === 'cancelled'" :size="14" class="shrink-0 mt-0.5 text-red-500" />
                  <Circle v-else :size="14" class="shrink-0 mt-0.5 text-fg-faint" />
                  <span class="text-[11px] font-medium text-fg-subtle mt-px shrink-0">{{ card.tmId }}.{{ s.id }}</span>
                  <span class="flex-1 text-sm leading-snug" :class="s.status === 'done' ? 'text-fg-subtle line-through' : 'text-fg'">{{ s.title }}</span>
                  <span v-if="s.status !== 'done' && s.status !== 'pending'" class="shrink-0 text-[11px] px-1.5 rounded border" :class="statusTag(s.status)">{{ s.status }}</span>
                </button>
                <div v-if="subtasksOpen.has(s.id)" class="pl-[3.25rem] pr-3 pb-3 flex flex-col gap-2">
                  <div v-if="s.description" :class="PROSE" v-html="renderMarkdown(s.description)" />
                  <div v-if="s.details" :class="PROSE" class="text-fg-subtle" v-html="renderMarkdown(s.details)" />
                </div>
              </li>
            </ul>
          </section>

          <!-- Jira subtasks -->
          <section v-if="jira?.subtasks.length">
            <h3 class="text-[11px] font-semibold uppercase tracking-wide text-fg-subtle mb-2">Subtasks</h3>
            <ul class="flex flex-col rounded-lg border border-border divide-y divide-border overflow-hidden">
              <li v-for="s in jira.subtasks" :key="s.key">
                <button
                  class="w-full flex items-center gap-2 px-3 py-2 bg-surface text-left hover:bg-elevated transition-colors cursor-pointer"
                  @click="openUrl(jiraUrl(s.key))"
                >
                  <span class="text-[11px] font-medium text-fg-subtle shrink-0">{{ s.key }}</span>
                  <span class="flex-1 min-w-0 truncate text-sm text-fg">{{ s.summary }}</span>
                  <span v-if="s.status" class="shrink-0 text-[11px] px-1.5 rounded border" :class="statusTag(s.status)">{{ s.status }}</span>
                </button>
              </li>
            </ul>
          </section>

          <!-- Jira comments -->
          <section v-if="jira">
            <h3 class="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle mb-2">
              <MessageSquare :size="11" />
              Comments
              <span class="normal-case font-normal tracking-normal">{{ jira.comments.length }}</span>
            </h3>
            <p v-if="!jira.comments.length" class="text-xs text-fg-faint">No comments.</p>
            <div class="flex flex-col gap-3">
              <div v-for="(c, i) in jira.comments" :key="i" class="rounded-lg border border-border bg-surface px-3 py-2.5">
                <div class="flex items-baseline gap-2 mb-1.5 text-xs">
                  <span class="font-medium text-fg">{{ c.author ?? 'Someone' }}</span>
                  <span class="text-fg-faint">{{ formatDate(c.created) }}</span>
                </div>
                <div :class="PROSE" v-html="renderMarkdown(c.body)" />
              </div>
            </div>
          </section>
        </div>
      </div>

      <!-- Footer -->
      <div class="flex items-center gap-2 pl-7 pr-4 py-3 border-t border-border/60 shrink-0">
        <button
          v-if="card.url"
          class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs text-fg-muted hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
          @click="openUrl(card.url)"
        >
          <ExternalLink :size="13" />
          Open in Jira
        </button>
        <span class="flex-1" />
        <span class="text-xs text-fg-subtle">Chat about this</span>
        <ChatLinkButton
          :task-key="card.key"
          @start="emit('chat', board.startChatForCard(card))"
          @open="emit('chat', $event)"
        />
      </div>
    </div>
  </div>
</template>
