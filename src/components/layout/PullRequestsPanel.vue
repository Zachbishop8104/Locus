<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import {
  GitPullRequest, GitPullRequestDraft, RefreshCw, PanelRightClose, CircleCheck, CircleX, CircleDot,
  MessageSquare, AlertCircle, ListFilter, Check,
} from '@lucide/vue'
import { useGithubStore, PR_STATUSES, PR_ROLES, MY_PR_STATUS } from '@/stores/github'
import type { PrItem, AuthorGroup } from '@/stores/github'

const github = useGithubStore()
const filterOpen = ref(false)
const filterRoot = ref<HTMLElement | null>(null)

// Re-render relative times every minute.
const now = ref(Date.now())
let clock: ReturnType<typeof setInterval> | null = null

function onDocClick(e: MouseEvent) {
  if (filterOpen.value && filterRoot.value && !filterRoot.value.contains(e.target as Node)) filterOpen.value = false
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') filterOpen.value = false
}

onMounted(() => {
  github.startPolling()
  clock = setInterval(() => (now.value = Date.now()), 60_000)
  document.addEventListener('mousedown', onDocClick)
  document.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => {
  github.stopPolling()
  if (clock) clearInterval(clock)
  document.removeEventListener('mousedown', onDocClick)
  document.removeEventListener('keydown', onKey)
})

interface Section {
  key: string
  label: string
  color: string | null
  count: number
  priority: boolean
  authors: AuthorGroup[]
}

/** Pinned "changes requested on my PRs" section first, then project groups. */
const sections = computed<Section[]>(() => {
  const out: Section[] = []
  if (github.priorityItems.length) {
    out.push({
      key: '__priority',
      label: 'Changes requested on your PRs',
      color: null,
      count: github.priorityItems.length,
      priority: true,
      authors: [{ author: github.prs?.login ?? '', avatar: null, isMe: true, items: github.priorityItems }],
    })
  }
  for (const g of github.groups) out.push({ ...g, priority: false })
  return out
})

/** Project headers are redundant when the chat is scoped to one project. */
const showProjectHeaders = computed(() => !github.scopeProject)
const hiddenCount = computed(() => github.scopedItems.length - github.filteredItems.length)

function timeAgo(iso: string): string {
  const mins = Math.max(0, Math.round((now.value - new Date(iso).getTime()) / 60_000))
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m`
  const hrs = Math.round(mins / 60)
  if (hrs < 24) return `${hrs}h`
  const days = Math.round(hrs / 24)
  if (days < 30) return `${days}d`
  return new Date(iso).toLocaleDateString([], { month: 'short', day: 'numeric' })
}

const STATUS_TAG: Record<PrItem['status'], { label: string; cls: string }> = {
  changes: { label: 'Changes requested', cls: 'bg-red-50 text-red-700 border-red-200' },
  review: { label: 'Needs review', cls: 'bg-elevated text-fg-muted border-border' },
  approved: { label: 'Approved', cls: 'bg-emerald-50 text-emerald-700 border-emerald-200' },
  draft: { label: 'Draft', cls: 'bg-elevated text-fg-subtle border-border' },
}

function checksLabel(pr: PrItem): string {
  switch (pr.checks) {
    case 'SUCCESS': return 'Checks passed'
    case 'FAILURE':
    case 'ERROR': return 'Checks failing'
    default: return 'Checks running'
  }
}

function open(pr: PrItem) {
  invoke('open_url', { url: pr.url }).catch(() => {})
}
</script>

<template>
  <!-- Collapsed rail -->
  <aside
    v-if="!github.panelOpen"
    class="w-11 shrink-0 flex flex-col items-center pt-3 border-l border-border/60 bg-surface/50"
  >
    <button
      class="relative p-2 rounded-lg text-fg-muted hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
      title="Show pull requests"
      @click="github.panelOpen = true"
    >
      <GitPullRequest :size="17" />
      <span
        v-if="github.reviewCount > 0"
        class="absolute -top-0.5 -right-0.5 min-w-4 h-4 px-1 rounded-full bg-amber-500 text-white text-[10px] font-semibold leading-4 text-center"
      >{{ github.reviewCount }}</span>
    </button>
  </aside>

  <!-- Panel -->
  <aside v-else class="w-80 shrink-0 flex flex-col border-l border-border/60 bg-surface/50 min-h-0">
    <div class="flex items-center gap-1 pl-4 pr-3 py-4 border-b border-border/60 shrink-0">
      <GitPullRequest :size="15" class="text-accent-icon mr-1" />
      <div class="flex-1 min-w-0">
        <h2 class="text-sm font-semibold text-fg">Pull requests</h2>
        <p v-if="github.scopeProject" class="text-[11px] text-fg-subtle truncate" :title="github.scopeProject.name">
          {{ github.scopeProject.name }}
        </p>
      </div>

      <!-- Filters -->
      <div ref="filterRoot" class="relative">
        <button
          class="relative p-1 rounded transition-colors cursor-pointer"
          :class="filterOpen || !github.isDefaultFilter ? 'text-accent-icon bg-elevated' : 'text-fg-subtle hover:text-fg hover:bg-elevated'"
          title="Filter"
          @click="filterOpen = !filterOpen"
        >
          <ListFilter :size="14" />
          <span v-if="!github.isDefaultFilter" class="absolute top-0 right-0 w-1.5 h-1.5 rounded-full bg-accent" />
        </button>

        <div
          v-if="filterOpen"
          class="absolute right-0 top-full mt-2 w-60 rounded-xl border border-border-strong bg-surface shadow-xl z-30 p-1.5"
        >
          <p class="px-2 pt-1.5 pb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Status</p>
          <button
            v-for="s in PR_STATUSES"
            :key="s.id"
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-sm text-fg hover:bg-elevated transition-colors cursor-pointer"
            @click="github.toggleStatus(s.id)"
          >
            <span
              class="w-4 h-4 rounded border flex items-center justify-center shrink-0"
              :class="github.statusFilter.includes(s.id) ? 'bg-accent border-accent text-white' : 'border-border-strong'"
            >
              <Check v-if="github.statusFilter.includes(s.id)" :size="11" stroke-width="3" />
            </span>
            {{ s.label }}
          </button>

          <p class="px-2 pt-2.5 pb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Involvement</p>
          <button
            v-for="r in PR_ROLES"
            :key="r.id"
            class="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg text-sm text-fg hover:bg-elevated transition-colors cursor-pointer"
            @click="github.toggleRole(r.id)"
          >
            <span
              class="w-4 h-4 rounded border flex items-center justify-center shrink-0"
              :class="github.roleFilter.includes(r.id) ? 'bg-accent border-accent text-white' : 'border-border-strong'"
            >
              <Check v-if="github.roleFilter.includes(r.id)" :size="11" stroke-width="3" />
            </span>
            {{ r.label }}
          </button>

          <div class="mt-1.5 pt-1.5 border-t border-border px-2 pb-1">
            <button
              class="text-xs text-fg-muted hover:text-fg transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-default"
              :disabled="github.isDefaultFilter"
              @click="github.resetFilters()"
            >
              Reset to default
            </button>
          </div>
        </div>
      </div>

      <button
        class="p-1 rounded text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
        title="Refresh"
        :disabled="github.loading"
        @click="github.refresh()"
      >
        <RefreshCw :size="14" :class="github.loading ? 'animate-spin' : ''" />
      </button>
      <button
        class="p-1 rounded text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
        title="Hide"
        @click="github.panelOpen = false"
      >
        <PanelRightClose :size="14" />
      </button>
    </div>

    <div class="flex-1 overflow-y-auto min-h-0">
      <!-- Setup / error states -->
      <div v-if="github.error && !github.prs" class="m-4 p-3 rounded-lg border border-border bg-surface text-xs text-fg-muted">
        <div class="flex items-start gap-2">
          <AlertCircle :size="14" class="shrink-0 mt-0.5 text-amber-600" />
          <div v-if="github.error === 'GH_NOT_INSTALLED'">
            <p class="font-medium text-fg">GitHub CLI not found</p>
            <p class="mt-1">
              Install it from <span class="font-mono">cli.github.com</span>, then run
              <code class="font-mono text-fg">gh auth login</code> in a terminal and refresh.
            </p>
          </div>
          <div v-else-if="github.error === 'GH_NOT_AUTHENTICATED'">
            <p class="font-medium text-fg">GitHub CLI isn't signed in</p>
            <p class="mt-1">Run <code class="font-mono text-fg">gh auth login</code> in a terminal, then refresh.</p>
          </div>
          <div v-else>
            <p class="font-medium text-fg">Couldn't load pull requests</p>
            <p class="mt-1 break-words">{{ github.error }}</p>
          </div>
        </div>
      </div>

      <!-- First load -->
      <div v-else-if="!github.prs" class="flex items-center gap-2 px-4 py-6 text-xs text-fg-subtle">
        <RefreshCw :size="13" class="animate-spin" />
        Loading pull requests…
      </div>

      <template v-else>
        <p v-if="github.scopeProject && !github.scopeProject.gitRepo?.trim()" class="px-4 py-6 text-sm text-fg-subtle">
          This project has no GitHub repo set.
        </p>
        <p v-else-if="github.scopeProject && github.scopedItems.length === 0" class="px-4 py-6 text-sm text-fg-subtle">
          No open pull requests involving you in {{ github.scopeProject.name }}.
        </p>
        <p v-else-if="github.allItems.length === 0" class="px-4 py-6 text-sm text-fg-subtle">
          No open pull requests involving you.
        </p>
        <p v-else-if="github.filteredItems.length === 0" class="px-4 py-6 text-sm text-fg-subtle">
          No pull requests match your filters.
        </p>

        <!-- Pinned section, then project → author groups -->
        <section
          v-for="section in sections"
          :key="section.key"
          class="pt-3"
          :class="section.priority ? 'pb-2 bg-red-50/40 border-b border-red-200/60' : ''"
        >
          <div v-if="section.priority" class="flex items-center gap-1.5 px-4 pb-1">
            <AlertCircle :size="13" class="shrink-0 text-red-600" />
            <span class="text-xs font-semibold text-red-700 flex-1 truncate">{{ section.label }}</span>
            <span class="text-[10px] font-semibold px-1.5 rounded-full bg-red-100 text-red-700">{{ section.count }}</span>
          </div>
          <div v-else-if="showProjectHeaders" class="flex items-center gap-2 px-4 pb-1">
            <span
              class="w-2 h-2 rounded-full shrink-0"
              :class="section.color ? '' : 'bg-fg-faint'"
              :style="section.color ? { backgroundColor: section.color } : undefined"
            />
            <span class="text-xs font-semibold text-fg truncate" :title="section.label">{{ section.label }}</span>
            <span class="text-[10px] font-semibold px-1.5 rounded-full bg-elevated text-fg-subtle">{{ section.count }}</span>
          </div>

          <div v-for="a in section.authors" :key="a.author">
            <div v-if="!section.priority" class="flex items-center gap-1.5 px-4 pt-1.5 pb-0.5 text-[11px] text-fg-subtle">
              <img v-if="a.avatar" :src="a.avatar" alt="" class="w-3.5 h-3.5 rounded-full shrink-0" />
              <span class="font-medium truncate" :class="a.isMe ? 'text-fg' : ''" :title="a.author">
                {{ a.isMe ? 'You' : a.author }}
              </span>
              <span class="shrink-0">· {{ a.items.length }}</span>
            </div>

          <button
            v-for="pr in a.items"
            :key="pr.id"
            class="w-full text-left px-4 py-2.5 hover:bg-elevated/70 transition-colors cursor-pointer border-l-2"
            :class="pr.role === 'requested' ? 'border-amber-400' : 'border-transparent'"
            :title="`Open ${pr.repo}#${pr.number} on GitHub`"
            @click="open(pr)"
          >
            <div class="flex items-start gap-1.5">
              <GitPullRequestDraft v-if="pr.isDraft" :size="13" class="shrink-0 mt-0.5 text-fg-subtle" />
              <GitPullRequest v-else :size="13" class="shrink-0 mt-0.5 text-accent-icon" />
              <p class="flex-1 text-sm text-fg leading-snug line-clamp-2">{{ pr.title }}</p>
              <span class="shrink-0 text-[11px] text-fg-subtle mt-0.5">{{ timeAgo(pr.updatedAt) }}</span>
            </div>

            <!-- One line: the author name shortens first; everything else stays fixed. -->
            <div class="mt-1.5 pl-5 flex items-center gap-1.5 text-[11px] text-fg-subtle whitespace-nowrap">
              <span class="shrink-0">#{{ pr.number }}</span>
              <span v-if="section.priority && showProjectHeaders" class="min-w-0 truncate" :title="pr.repo">· {{ pr.repo.split('/').pop() }}</span>
              <span class="shrink-0 px-1.5 rounded border" :class="STATUS_TAG[pr.status].cls">{{ STATUS_TAG[pr.status].label }}</span>
              <span
                v-if="pr.myStatus"
                class="min-w-0 truncate px-1.5 rounded border"
                :class="MY_PR_STATUS[pr.myStatus].cls"
                :title="MY_PR_STATUS[pr.myStatus].title"
              >{{ MY_PR_STATUS[pr.myStatus].label }}</span>
              <span class="flex-1" />
              <span v-if="pr.comments > 0" class="shrink-0 flex items-center gap-0.5" :title="`${pr.comments} comments`">
                <MessageSquare :size="11" />{{ pr.comments }}
              </span>
              <span v-if="pr.checks" class="shrink-0 flex" :title="checksLabel(pr)">
                <CircleCheck v-if="pr.checks === 'SUCCESS'" :size="13" class="text-emerald-600" />
                <CircleX v-else-if="pr.checks === 'FAILURE' || pr.checks === 'ERROR'" :size="13" class="text-red-600" />
                <CircleDot v-else :size="13" class="text-amber-500" />
              </span>
            </div>
          </button>
          </div>
        </section>
      </template>
    </div>

    <div
      v-if="github.lastUpdated"
      class="shrink-0 flex items-center gap-1 px-4 py-2 border-t border-border/60 text-[11px] text-fg-faint"
    >
      <span>Updated {{ timeAgo(github.lastUpdated.toISOString()) }}{{ timeAgo(github.lastUpdated.toISOString()) === 'just now' ? '' : ' ago' }}</span>
      <span v-if="github.error" class="text-amber-600">· last refresh failed</span>
      <span class="flex-1" />
      <button
        v-if="hiddenCount > 0"
        class="text-fg-subtle hover:text-fg transition-colors cursor-pointer"
        title="Change filters"
        @click="filterOpen = true"
      >{{ hiddenCount }} hidden by filters</button>
    </div>
  </aside>
</template>
