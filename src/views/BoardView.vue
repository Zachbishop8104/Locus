<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import {
  SquareKanban, Ticket, ListTodo, GitPullRequest, GitPullRequestDraft, RefreshCw, AlertCircle, X, Plus,
  CircleCheck, CircleX, CircleDot, ListChecks, ExternalLink, FolderPlus, Folder,
} from '@lucide/vue'
import ChatLinkButton from '@/components/board/ChatLinkButton.vue'
import ChatModal from '@/components/board/ChatModal.vue'
import CardDetailModal from '@/components/board/CardDetailModal.vue'
import { cardKind, priorityTag, SOURCE_STRIPE } from '@/components/board/cardStyle'
import { useBoardStore } from '@/stores/board'
import type { BoardCard, ColumnId } from '@/stores/board'
import { useGithubStore, MY_PR_STATUS } from '@/stores/github'
import type { PrItem } from '@/stores/github'
import { useProjectsStore } from '@/stores/projects'

const router = useRouter()
const board = useBoardStore()
const github = useGithubStore()
const projects = useProjectsStore()

// A saved filter can point at a project that has since been deleted.
if (board.projectFilter !== 'all' && !projects.getById(board.projectFilter)) board.projectFilter = 'all'

onMounted(() => {
  github.startPolling()
  board.refresh()
})
onBeforeUnmount(() => github.stopPolling())

watch(() => [board.projectFilter, board.jiraMineOnly, board.showJira, board.showTaskmaster], () => board.refresh())

const singleProject = computed(() => board.projectFilter === 'all' ? null : projects.getById(board.projectFilter) ?? null)
const showProjectTags = computed(() => !singleProject.value)

function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

function openUrl(url: string | null) {
  if (url) invoke('open_url', { url }).catch(() => {})
}

/** The card chat shown in the modal. */
const modalChatId = ref<string | null>(null)

/** The card opened to see everything in it. */
const detailCard = ref<BoardCard | null>(null)

function openDetail(card: BoardCard, e: MouseEvent) {
  // Buttons and links on the card do their own thing.
  if ((e.target as HTMLElement).closest('button, a')) return
  detailCard.value = card
}

function openChatFromDetail(id: string) {
  detailCard.value = null
  modalChatId.value = id
}

// ── Setup notices ──

const tmErrorList = computed(() => Object.entries(board.tmErrors))
const tmNotInstalled = computed(() => tmErrorList.value.some(([, e]) => e === 'TM_NOT_INSTALLED'))
const tmOtherErrors = computed(() => tmErrorList.value.filter(([, e]) => e !== 'TM_NOT_INSTALLED'))
/** The picked project already has task-master in one of its folders. */
const hasTaskmaster = computed(() => !!singleProject.value && [...board.tmFolders.keys()].some((p) => p in board.tmTasks))
// Setup hints only show while the project has no tasks from either source.
const noJiraKeys = computed(() => board.showJira && !!singleProject.value && board.jiraKeys.length === 0 && !hasTaskmaster.value)
const tmSetupFolders = computed(() =>
  singleProject.value && !hasTaskmaster.value && !board.jiraKeys.length ? board.tmUninitialized : [],
)

// ── task-master folders menu ──

const foldersOpen = ref(false)
const foldersRoot = ref<HTMLElement | null>(null)

function onDocClick(e: MouseEvent) {
  if (foldersOpen.value && foldersRoot.value && !foldersRoot.value.contains(e.target as Node)) foldersOpen.value = false
}
onMounted(() => document.addEventListener('mousedown', onDocClick))
onBeforeUnmount(() => document.removeEventListener('mousedown', onDocClick))

/** Folders with task-master on the board, and which project each belongs to. */
const boardFolders = computed(() =>
  [...board.tmFolders.entries()]
    .filter(([path]) => path in board.tmTasks || board.extraFolders.includes(path))
    .map(([path, project]) => ({ path, project, added: board.extraFolders.includes(path) })),
)

async function pickFolder() {
  const path = await invoke<string | null>('pick_folder')
  if (!path) return
  if (!(await board.addFolder(path))) {
    board.actionError = `${folderName(path)} doesn't use task-master yet. Pick it as a project folder and use "Set up", or run task-master init there.`
    board.removeFolder(path)
  }
}

// ── Drag and drop ──

const dragging = ref<BoardCard | null>(null)
const dropTarget = ref<ColumnId | null>(null)

function onDragStart(card: BoardCard, e: DragEvent) {
  dragging.value = card
  e.dataTransfer?.setData('text/plain', card.key)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

function onDragEnd() {
  dragging.value = null
  dropTarget.value = null
}

function onDragOver(col: ColumnId, e: DragEvent) {
  if (!dragging.value) return
  e.preventDefault()
  dropTarget.value = col
}

function onDrop(col: ColumnId) {
  const card = dragging.value
  onDragEnd()
  if (card) board.moveCard(card, col)
}

// ── Adding a task-master task ──

/** Folders in scope where task-master is set up. */
const tmReadyFolders = computed(() => [...board.tmFolders.keys()].filter((p) => p in board.tmTasks))
const adding = ref(false)
const newTitle = ref('')
const newDescription = ref('')
const newFolder = ref('')
const saving = ref(false)

function startAdding() {
  adding.value = true
  newTitle.value = ''
  newDescription.value = ''
  newFolder.value = tmReadyFolders.value[0] ?? ''
}

async function saveTask() {
  if (!newTitle.value.trim() || !newFolder.value || saving.value) return
  saving.value = true
  try {
    await board.addTask(newFolder.value, newTitle.value, newDescription.value)
    adding.value = false
  } catch {
    // shown via board.actionError
  } finally {
    saving.value = false
  }
}

// ── Display helpers ──

const PR_TAG: Record<PrItem['status'], { label: string; cls: string }> = {
  changes: { label: 'Changes requested', cls: 'bg-red-50 text-red-700 border-red-200' },
  review: { label: 'Needs review', cls: 'bg-elevated text-fg-muted border-border' },
  approved: { label: 'Approved', cls: 'bg-emerald-50 text-emerald-700 border-emerald-200' },
  draft: { label: 'Draft', cls: 'bg-elevated text-fg-subtle border-border' },
}

const COLUMN_ACCENT: Record<ColumnId, string> = {
  todo: 'bg-fg-faint',
  in_progress: 'bg-sky-500',
  review: 'bg-violet-500',
  done: 'bg-emerald-500',
}

const sourceChips = computed(() => [
  { label: 'Jira', icon: Ticket, on: board.showJira, toggle: () => (board.showJira = !board.showJira) },
  { label: 'Tasks', icon: ListTodo, on: board.showTaskmaster, toggle: () => (board.showTaskmaster = !board.showTaskmaster) },
  { label: 'PRs', icon: GitPullRequest, on: board.showPrs, toggle: () => (board.showPrs = !board.showPrs) },
])

/** The four status columns with the PR column slotted in before Done. */
const lanes = computed(() => {
  const cols = board.columns.map((c) => ({ kind: 'status' as const, ...c }))
  const out: (typeof cols[number] | { kind: 'prs'; id: 'prs'; label: string })[] = [...cols]
  if (board.showPrs) out.splice(out.length - 1, 0, { kind: 'prs', id: 'prs', label: 'Pull requests' })
  return out
})
</script>

<template>
  <div class="flex flex-col h-full min-h-0">
    <!-- Header -->
    <div class="flex flex-wrap items-center gap-3 px-6 py-4 border-b border-border/60 shrink-0">
      <div class="flex items-center gap-2 mr-2">
        <SquareKanban :size="16" class="text-accent-icon" />
        <h1 class="text-sm font-semibold text-fg">Board</h1>
      </div>

      <select
        v-model="board.projectFilter"
        class="px-2.5 py-1.5 rounded-lg border border-border bg-surface text-sm text-fg outline-none focus:border-accent-light/60 cursor-pointer"
      >
        <option value="all">All projects</option>
        <option v-for="p in projects.projects" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>

      <div class="flex items-center gap-1">
        <button
          v-for="s in sourceChips"
          :key="s.label"
          class="flex items-center gap-1.5 px-2.5 py-1 rounded-full border text-xs font-medium transition-colors cursor-pointer"
          :class="s.on ? 'border-accent/40 bg-accent/10 text-accent-fg' : 'border-border text-fg-subtle hover:text-fg'"
          @click="s.toggle"
        >
          <component :is="s.icon" :size="12" />
          {{ s.label }}
        </button>
      </div>

      <label v-if="board.showJira" class="flex items-center gap-1.5 text-xs text-fg-muted cursor-pointer select-none">
        <input v-model="board.jiraMineOnly" type="checkbox" class="accent-accent" />
        Only my Jira issues
      </label>

      <div v-if="board.showTaskmaster" ref="foldersRoot" class="relative">
        <button
          class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs text-fg-muted hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
          :class="foldersOpen ? 'bg-elevated text-fg' : ''"
          title="Folders whose task-master tasks are on the board"
          @click="foldersOpen = !foldersOpen"
        >
          <Folder :size="12" />
          Task folders
          <span class="text-fg-subtle">{{ boardFolders.length }}</span>
        </button>
        <div
          v-if="foldersOpen"
          class="absolute left-0 top-full mt-2 w-80 rounded-xl border border-border-strong bg-surface shadow-xl z-30 p-1.5"
        >
          <p class="px-2 pt-1 pb-1.5 text-[11px] text-fg-subtle">
            Project folders and any task-master folders next to them are picked up automatically.
          </p>
          <div
            v-for="f in boardFolders"
            :key="f.path"
            class="group flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-elevated"
            :title="f.path"
          >
            <span
              class="w-2 h-2 rounded-full shrink-0"
              :class="f.project ? '' : 'bg-fg-faint'"
              :style="f.project ? { background: f.project.color } : undefined"
            />
            <span class="flex-1 min-w-0">
              <span class="block text-sm text-fg truncate">{{ folderName(f.path) }}</span>
              <span class="block text-[11px] text-fg-subtle truncate">{{ f.project ? f.project.name : 'No project' }}</span>
            </span>
            <button
              v-if="f.added"
              class="shrink-0 p-0.5 rounded text-fg-subtle hover:text-red-600 opacity-0 group-hover:opacity-100 cursor-pointer"
              title="Remove from board"
              @click="board.removeFolder(f.path)"
            >
              <X :size="12" />
            </button>
          </div>
          <p v-if="!boardFolders.length" class="px-2 py-1.5 text-xs text-fg-faint">No task-master folders found.</p>
          <div class="mt-1 pt-1 border-t border-border">
            <button
              class="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-sm text-accent-fg hover:bg-elevated transition-colors cursor-pointer"
              @click="pickFolder"
            >
              <FolderPlus :size="13" class="shrink-0" />
              Add folder…
            </button>
          </div>
        </div>
      </div>

      <span class="flex-1" />
      <span v-if="board.lastUpdated" class="text-[11px] text-fg-faint">
        Updated {{ board.lastUpdated.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) }}
      </span>
      <button
        class="p-1.5 rounded-lg text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
        title="Refresh"
        :disabled="board.loading"
        @click="board.refresh()"
      >
        <RefreshCw :size="15" :class="board.loading ? 'animate-spin' : ''" />
      </button>
    </div>

    <!-- Notices -->
    <div class="flex flex-col gap-2 px-6 empty:hidden pt-3">
      <div
        v-if="board.actionError"
        class="flex items-start gap-2 px-3 py-2 rounded-lg border border-red-200 bg-red-50 text-xs text-red-700"
      >
        <AlertCircle :size="14" class="shrink-0 mt-0.5" />
        <span class="flex-1 break-words">{{ board.actionError }}</span>
        <button class="shrink-0 cursor-pointer hover:text-red-900" title="Dismiss" @click="board.actionError = null">
          <X :size="13" />
        </button>
      </div>

      <div
        v-if="board.showJira && board.jiraError"
        class="flex items-start gap-2 px-3 py-2 rounded-lg border border-border bg-surface text-xs text-fg-muted"
      >
        <AlertCircle :size="14" class="shrink-0 mt-0.5 text-amber-600" />
        <span v-if="board.jiraError === 'JIRA_NOT_INSTALLED'">
          <span class="font-medium text-fg">No Jira CLI found.</span>
          Install Atlassian's <span class="font-mono">twg</span> or <span class="font-mono">acli</span> CLI and sign in, then refresh.
        </span>
        <span v-else-if="board.jiraError === 'JIRA_NOT_AUTHENTICATED'">
          <span class="font-medium text-fg">The Jira CLI isn't signed in.</span>
          Run <code class="font-mono text-fg">twg login</code> (or <code class="font-mono text-fg">acli jira auth login --web</code>), then refresh.
        </span>
        <span v-else class="break-words"><span class="font-medium text-fg">Couldn't load Jira issues:</span> {{ board.jiraError }}</span>
      </div>
      <p v-else-if="noJiraKeys" class="text-xs text-fg-subtle">
        Couldn't find a Jira project key on {{ singleProject?.name }} —
        <button class="underline hover:no-underline cursor-pointer" @click="router.push('/projects')">set one in Projects</button>
        (a key like <span class="font-mono">SJ</span> or a Jira board URL) to see its issues here.
      </p>

      <template v-if="board.showTaskmaster">
        <div
          v-if="tmNotInstalled"
          class="flex items-start gap-2 px-3 py-2 rounded-lg border border-border bg-surface text-xs text-fg-muted"
        >
          <AlertCircle :size="14" class="shrink-0 mt-0.5 text-amber-600" />
          <span>
            <span class="font-medium text-fg">task-master not found.</span>
            Run <code class="font-mono text-fg">npm i -g task-master-ai</code>, then refresh.
          </span>
        </div>
        <div
          v-for="[path, err] in tmOtherErrors"
          :key="path"
          class="flex items-start gap-2 px-3 py-2 rounded-lg border border-border bg-surface text-xs text-fg-muted"
        >
          <AlertCircle :size="14" class="shrink-0 mt-0.5 text-amber-600" />
          <span class="break-words"><span class="font-medium text-fg">task-master in {{ folderName(path) }}:</span> {{ err }}</span>
        </div>
        <!-- Offer setup only when one project is picked, so "All projects" isn't a wall of prompts. -->
        <div
          v-for="path in tmSetupFolders"
          :key="path"
          class="flex items-center gap-2 px-3 py-2 rounded-lg border border-border bg-surface text-xs text-fg-muted"
        >
          <ListTodo :size="14" class="shrink-0 text-fg-subtle" />
          <span class="flex-1 truncate" :title="path">task-master isn't set up in <span class="font-medium text-fg">{{ folderName(path) }}</span>.</span>
          <button
            class="shrink-0 px-2.5 py-1 rounded-md bg-accent hover:bg-accent-light text-white font-medium transition-colors cursor-pointer"
            @click="board.initTaskmaster(path)"
          >
            Set up
          </button>
        </div>
      </template>
    </div>

    <!-- Columns -->
    <div class="flex-1 min-h-0 overflow-x-auto">
      <div class="flex gap-3 h-full px-6 py-4 min-w-max">
        <section
          v-for="lane in lanes"
          :key="lane.id"
          class="w-72 shrink-0 flex flex-col min-h-0 rounded-xl border transition-colors"
          :class="lane.kind === 'status' && dropTarget === lane.id
            ? 'border-accent/60 bg-accent/5'
            : 'border-border/70 bg-surface/60'"
          @dragover="lane.kind === 'status' && onDragOver(lane.id, $event)"
          @dragleave="dropTarget === lane.id && (dropTarget = null)"
          @drop.prevent="lane.kind === 'status' && onDrop(lane.id)"
        >
          <!-- Column header -->
          <div class="flex items-center gap-2 px-3 pt-3 pb-2 shrink-0">
            <GitPullRequest v-if="lane.kind === 'prs'" :size="13" class="text-accent-icon" />
            <span v-else class="w-2 h-2 rounded-full" :class="COLUMN_ACCENT[lane.id]" />
            <h2 class="text-xs font-semibold uppercase tracking-wide text-fg-muted">{{ lane.label }}</h2>
            <span class="text-[10px] font-semibold px-1.5 rounded-full bg-elevated text-fg-subtle">
              {{ lane.kind === 'prs' ? board.prs.length : lane.cards.length }}
            </span>
            <span class="flex-1" />
            <button
              v-if="lane.id === 'todo' && board.showTaskmaster && tmReadyFolders.length"
              class="p-0.5 rounded text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
              title="Add a task-master task"
              @click="startAdding"
            >
              <Plus :size="14" />
            </button>
          </div>

          <div class="flex-1 min-h-0 overflow-y-auto px-2 pb-2 flex flex-col gap-2">
            <!-- New task form -->
            <form
              v-if="lane.id === 'todo' && adding"
              class="flex flex-col gap-1.5 p-2.5 rounded-lg border border-accent/40 bg-surface shadow-sm"
              @submit.prevent="saveTask"
            >
              <input
                v-model="newTitle"
                placeholder="Task title"
                class="px-2 py-1 rounded-md border border-border bg-bg text-sm text-fg outline-none focus:border-accent-light/60"
                autofocus
                @keydown.escape="adding = false"
              />
              <textarea
                v-model="newDescription"
                placeholder="Description (optional)"
                rows="2"
                class="px-2 py-1 rounded-md border border-border bg-bg text-xs text-fg outline-none resize-none focus:border-accent-light/60"
              />
              <select
                v-if="tmReadyFolders.length > 1"
                v-model="newFolder"
                class="px-2 py-1 rounded-md border border-border bg-bg text-xs text-fg outline-none"
              >
                <option v-for="p in tmReadyFolders" :key="p" :value="p">{{ folderName(p) }}</option>
              </select>
              <div class="flex justify-end gap-1.5">
                <button type="button" class="px-2 py-1 rounded-md text-xs text-fg-muted hover:text-fg cursor-pointer" @click="adding = false">
                  Cancel
                </button>
                <button
                  type="submit"
                  class="px-2.5 py-1 rounded-md text-xs font-medium bg-accent hover:bg-accent-light text-white transition-colors cursor-pointer disabled:opacity-50"
                  :disabled="!newTitle.trim() || saving"
                >
                  {{ saving ? 'Adding…' : 'Add task' }}
                </button>
              </div>
            </form>

            <!-- Pull requests -->
            <template v-if="lane.kind === 'prs'">
              <p v-if="github.error && !github.prs" class="px-1 py-2 text-xs text-fg-subtle">
                {{ github.error === 'GH_NOT_INSTALLED' ? 'GitHub CLI not found.' : github.error === 'GH_NOT_AUTHENTICATED' ? 'Run gh auth login to see pull requests.' : github.error }}
              </p>
              <div
                v-for="pr in board.prs"
                :key="pr.id"
                class="group p-2.5 rounded-lg border bg-surface hover:border-border-strong transition-colors"
                :class="pr.role === 'requested' ? 'border-amber-300' : 'border-border'"
              >
                <div class="flex items-start gap-1.5">
                  <GitPullRequestDraft v-if="pr.isDraft" :size="13" class="shrink-0 mt-0.5 text-fg-subtle" />
                  <GitPullRequest v-else :size="13" class="shrink-0 mt-0.5 text-accent-icon" />
                  <button
                    class="flex-1 text-left text-sm text-fg leading-snug line-clamp-3 hover:underline cursor-pointer"
                    :title="`Open ${pr.repo}#${pr.number} on GitHub`"
                    @click="openUrl(pr.url)"
                  >{{ pr.title }}</button>
                </div>
                <div class="mt-2 flex items-center gap-1.5 text-[11px] text-fg-subtle whitespace-nowrap">
                  <span class="min-w-0 truncate" :title="pr.repo">{{ pr.repo.split('/').pop() }}#{{ pr.number }}</span>
                  <span class="shrink-0 px-1.5 rounded border" :class="PR_TAG[pr.status].cls">{{ PR_TAG[pr.status].label }}</span>
                  <span class="flex-1" />
                  <CircleCheck v-if="pr.checks === 'SUCCESS'" :size="13" class="shrink-0 text-emerald-600" />
                  <CircleX v-else-if="pr.checks === 'FAILURE' || pr.checks === 'ERROR'" :size="13" class="shrink-0 text-red-600" />
                  <CircleDot v-else-if="pr.checks" :size="13" class="shrink-0 text-amber-500" />
                </div>
                <div class="mt-2 flex items-center gap-1.5">
                  <img v-if="pr.authorAvatar" :src="pr.authorAvatar" alt="" class="w-4 h-4 rounded-full" />
                  <span class="text-[11px] text-fg-subtle truncate">{{ pr.author }}</span>
                  <span
                    v-if="pr.myStatus"
                    class="min-w-0 truncate px-1.5 rounded border text-[11px]"
                    :class="MY_PR_STATUS[pr.myStatus].cls"
                    :title="MY_PR_STATUS[pr.myStatus].title"
                  >{{ MY_PR_STATUS[pr.myStatus].label }}</span>
                  <span class="flex-1" />
                  <ChatLinkButton
                    :task-key="board.refForPr(pr).key"
                    @start="modalChatId = board.startChatForPr(pr)"
                    @open="modalChatId = $event"
                  />
                </div>
              </div>
              <p v-if="github.prs && !board.prs.length" class="px-1 py-2 text-xs text-fg-faint">No open pull requests.</p>
            </template>

            <!-- Jira and task-master cards -->
            <template v-else>
              <div
                v-for="card in lane.cards"
                :key="card.key"
                draggable="true"
                class="relative shrink-0 pl-3.5 pr-2.5 py-2.5 rounded-lg border bg-surface shadow-sm overflow-hidden hover:border-border-strong hover:shadow-md transition-all cursor-pointer active:cursor-grabbing"
                :class="[
                  card.flag === 'Blocked' ? 'border-red-300' : 'border-border',
                  board.pending.has(card.key) ? 'opacity-50' : '',
                  dragging?.key === card.key ? 'opacity-40' : '',
                ]"
                :title="`Open ${card.label}`"
                @click="openDetail(card, $event)"
                @dragstart="onDragStart(card, $event)"
                @dragend="onDragEnd"
              >
                <span class="absolute left-0 top-0 bottom-0 w-1" :class="SOURCE_STRIPE[card.source]" />

                <div class="flex items-center gap-1.5 text-[11px] text-fg-subtle">
                  <component
                    :is="cardKind(card).icon"
                    :size="13"
                    class="shrink-0"
                    :class="cardKind(card).cls"
                    :title="cardKind(card).label"
                  />
                  <button
                    v-if="card.url"
                    class="font-semibold text-fg-muted hover:underline cursor-pointer flex items-center gap-0.5"
                    :title="`Open ${card.label} in Jira`"
                    @click="openUrl(card.url)"
                  >{{ card.label }}<ExternalLink :size="10" /></button>
                  <span v-else class="font-semibold text-fg-muted">{{ card.label }}</span>
                  <span class="flex-1" />
                  <span
                    v-if="priorityTag(card.priority)"
                    class="shrink-0 px-1.5 rounded border capitalize"
                    :class="priorityTag(card.priority)!"
                    :title="`${card.priority} priority`"
                  >{{ card.priority }}</span>
                </div>

                <p
                  class="mt-1.5 text-sm font-medium leading-snug line-clamp-3"
                  :class="card.column === 'done' ? 'text-fg-subtle' : 'text-fg'"
                >{{ card.title }}</p>

                <div class="mt-2 flex items-center gap-1.5 text-[11px] text-fg-subtle min-w-0">
                  <span
                    v-if="showProjectTags && card.project"
                    class="flex items-center gap-1 min-w-0 truncate"
                    :title="card.project.name"
                  >
                    <span class="w-1.5 h-1.5 rounded-full shrink-0" :style="{ background: card.project.color }" />
                    <span class="truncate">{{ card.project.name }}</span>
                  </span>
                  <span v-else-if="card.group" class="flex items-center gap-1 min-w-0 truncate" :title="card.group">
                    <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-fg-faint" />
                    <span class="truncate">{{ card.group }}</span>
                  </span>
                  <span
                    v-if="card.flag"
                    class="shrink-0 px-1.5 rounded border"
                    :class="card.flag === 'Blocked' ? 'bg-red-50 text-red-700 border-red-200' : 'bg-amber-50 text-amber-700 border-amber-200'"
                  >{{ card.flag }}</span>
                  <span v-if="card.subtasks" class="shrink-0 flex items-center gap-0.5" title="Subtasks done">
                    <ListChecks :size="11" />{{ card.subtasks.done }}/{{ card.subtasks.total }}
                  </span>
                  <span v-if="card.source === 'jira'" class="truncate max-w-24" :title="card.status">{{ card.status }}</span>
                  <span class="flex-1" />
                  <span
                    v-if="card.assignee"
                    class="shrink-0 w-5 h-5 rounded-full bg-elevated border border-border flex items-center justify-center text-[9px] font-semibold text-fg-muted"
                    :title="`Assigned to ${card.assignee}`"
                  >{{ card.assignee.split(/\s+/).map((w) => w[0]).slice(0, 2).join('').toUpperCase() }}</span>
                  <ChatLinkButton
                    :task-key="card.key"
                    @start="modalChatId = board.startChatForCard(card)"
                    @open="modalChatId = $event"
                  />
                </div>
                <div v-if="card.subtasks" class="mt-2 h-0.5 rounded-full bg-elevated overflow-hidden">
                  <div class="h-full bg-emerald-500" :style="{ width: `${(card.subtasks.done / card.subtasks.total) * 100}%` }" />
                </div>
              </div>
              <p
                v-if="!lane.cards.length && !(lane.id === 'todo' && adding)"
                class="px-1 py-2 text-xs text-fg-faint"
              >{{ dragging ? 'Drop here' : 'Nothing here' }}</p>
            </template>
          </div>
        </section>
      </div>
    </div>

    <CardDetailModal
      v-if="detailCard"
      :key="detailCard.key"
      :card="detailCard"
      @close="detailCard = null"
      @chat="openChatFromDetail"
    />
    <ChatModal v-if="modalChatId" :key="modalChatId" :conversation-id="modalChatId" @close="modalChatId = null" />
  </div>
</template>
