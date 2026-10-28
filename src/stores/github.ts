import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Project, PullRequest, PullRequestLists } from '@/types'
import { useChatStore } from './chat'
import { useProjectsStore } from './projects'

const REFRESH_MS = 5 * 60 * 1000

/** Each PR falls in exactly one status bucket. */
export type PrStatus = 'changes' | 'review' | 'approved' | 'draft'
/** How the viewer is involved, in priority order. */
export type PrRole = 'requested' | 'mine' | 'involved'

export const PR_STATUSES: { id: PrStatus; label: string }[] = [
  { id: 'changes', label: 'Changes requested' },
  { id: 'review', label: 'Needs review' },
  { id: 'approved', label: 'Approved' },
  { id: 'draft', label: 'Draft' },
]

export const PR_ROLES: { id: PrRole; label: string }[] = [
  { id: 'requested', label: 'Review requested' },
  { id: 'mine', label: 'Mine' },
  { id: 'involved', label: 'Involved' },
]

const DEFAULT_STATUSES: PrStatus[] = ['changes', 'review', 'approved']
const DEFAULT_ROLES: PrRole[] = ['requested', 'mine', 'involved']

/** Where you stand on someone else's PR. */
export type MyPrStatus = 'requested' | 'rerequested' | 'approved' | 'changes' | 'commented' | 'dismissed'

export const MY_PR_STATUS: Record<MyPrStatus, { label: string; title: string; cls: string }> = {
  requested: { label: 'Review requested', title: 'Your review is requested', cls: 'bg-amber-50 text-amber-700 border-amber-300' },
  rerequested: { label: 'Re-requested', title: 'Your review is requested again', cls: 'bg-amber-50 text-amber-700 border-amber-300' },
  approved: { label: 'You approved', title: 'You approved this PR', cls: 'bg-emerald-50 text-emerald-700 border-emerald-200' },
  changes: { label: 'You requested changes', title: 'You requested changes', cls: 'bg-red-50 text-red-700 border-red-200' },
  commented: { label: 'You commented', title: 'Commented without approving or requesting changes', cls: 'bg-sky-50 text-sky-700 border-sky-200' },
  dismissed: { label: 'Review dismissed', title: 'Your review was dismissed', cls: 'bg-elevated text-fg-subtle border-border' },
}

export interface PrItem extends PullRequest {
  role: PrRole
  status: PrStatus
  /** None on your own PRs, or when you're only mentioned or assigned. */
  myStatus: MyPrStatus | null
}

function myStatusOf(pr: PullRequest, role: PrRole): MyPrStatus | null {
  if (role === 'mine') return null
  if (role === 'requested') return pr.viewerReview || pr.viewerCommented ? 'rerequested' : 'requested'
  if (pr.viewerReview === 'APPROVED') return 'approved'
  if (pr.viewerReview === 'CHANGES_REQUESTED') return 'changes'
  if (pr.viewerCommented) return 'commented'
  if (pr.viewerReview === 'DISMISSED') return 'dismissed'
  return null
}

export interface AuthorGroup {
  author: string
  avatar: string | null
  isMe: boolean
  items: PrItem[]
}

export interface ProjectGroup {
  /** Project id when the repo matches a project, otherwise the repo slug. */
  key: string
  label: string
  color: string | null
  count: number
  authors: AuthorGroup[]
}

function statusOf(pr: PullRequest): PrStatus {
  if (pr.reviewDecision === 'CHANGES_REQUESTED') return 'changes'
  if (pr.isDraft) return 'draft'
  if (pr.reviewDecision === 'APPROVED') return 'approved'
  return 'review'
}

/** Normalizes a repo URL (https or ssh) or slug to lowercase `owner/name`. */
export function normalizeRepo(repo: string): string {
  return repo.trim().toLowerCase()
    .replace(/^git@[^:]+:/, '')
    .replace(/^[a-z+]+:\/\/[^/]+\//, '')
    .replace(/\/+$/, '')
    .replace(/\.git$/, '')
}

const updatedMs = (pr: PrItem) => new Date(pr.updatedAt).getTime()
const latest = (items: PrItem[]) => Math.max(...items.map(updatedMs))
/** Review requests first, then most recently updated. */
const byPriority = (a: PrItem, b: PrItem) =>
  Number(b.role === 'requested') - Number(a.role === 'requested') || updatedMs(b) - updatedMs(a)

export const useGithubStore = defineStore('github', () => {
  const chat = useChatStore()
  const projects = useProjectsStore()

  const prs = ref<PullRequestLists | null>(null)
  const loading = ref(false)
  /** 'GH_NOT_INSTALLED' | 'GH_NOT_AUTHENTICATED' | any other message */
  const error = ref<string | null>(null)
  const lastUpdated = ref<Date | null>(null)

  // ── Persisted UI state ──
  const panelOpen = ref(true)
  const statusFilter = ref<PrStatus[]>([...DEFAULT_STATUSES])
  const roleFilter = ref<PrRole[]>([...DEFAULT_ROLES])

  const allItems = computed<PrItem[]>(() => {
    const p = prs.value
    if (!p) return []
    const tag = (list: PullRequest[], role: PrRole) =>
      list.map((pr) => ({ ...pr, role, status: statusOf(pr), myStatus: myStatusOf(pr, role) }))
    return [...tag(p.reviewRequested, 'requested'), ...tag(p.authored, 'mine'), ...tag(p.involved, 'involved')]
  })

  const projectByRepo = computed(() => {
    const map = new Map<string, Project>()
    for (const p of projects.projects) {
      const repo = normalizeRepo(p.gitRepo ?? '')
      if (repo) map.set(repo, p)
    }
    return map
  })

  /** The open chat's project, if any. When set, only its PRs are shown. */
  const scopeProject = computed<Project | null>(() => {
    const id = chat.activeConversation?.projectId
    return id ? projects.getById(id) ?? null : null
  })

  /** PRs in scope, before the status/role filters are applied. */
  const scopedItems = computed<PrItem[]>(() => {
    const scope = scopeProject.value
    if (!scope) return allItems.value
    const repo = normalizeRepo(scope.gitRepo ?? '')
    return repo ? allItems.value.filter((pr) => normalizeRepo(pr.repo) === repo) : []
  })

  const filteredItems = computed(() =>
    scopedItems.value.filter((pr) => statusFilter.value.includes(pr.status) && roleFilter.value.includes(pr.role)),
  )

  /** My own PRs that someone requested changes on. These are pinned to the top. */
  const priorityItems = computed(() =>
    filteredItems.value
      .filter((pr) => pr.role === 'mine' && pr.status === 'changes')
      .sort((a, b) => updatedMs(b) - updatedMs(a)),
  )

  /**
   * Everything not pinned, grouped by project and then by author.
   * Projects are ordered by most recent activity. Within a project, my own PRs come
   * first, then authors whose PRs await my review, then authors by most recent activity.
   */
  const groups = computed<ProjectGroup[]>(() => {
    const me = prs.value?.login ?? ''
    const pinned = new Set(priorityItems.value.map((p) => p.id))

    const byProject = new Map<string, { label: string; color: string | null; items: PrItem[] }>()
    for (const pr of filteredItems.value) {
      if (pinned.has(pr.id)) continue
      const project = projectByRepo.value.get(normalizeRepo(pr.repo))
      const key = project?.id ?? pr.repo
      let group = byProject.get(key)
      if (!group) byProject.set(key, (group = { label: project?.name ?? pr.repo, color: project?.color ?? null, items: [] }))
      group.items.push(pr)
    }

    return [...byProject.entries()]
      .map(([key, g]) => {
        const byAuthor = new Map<string, PrItem[]>()
        for (const pr of g.items) {
          if (!byAuthor.has(pr.author)) byAuthor.set(pr.author, [])
          byAuthor.get(pr.author)!.push(pr)
        }
        const hasRequest = (a: AuthorGroup) => a.items.some((p) => p.role === 'requested')
        const authors = [...byAuthor.values()]
          .map((items): AuthorGroup => ({
            author: items[0].author,
            avatar: items[0].authorAvatar,
            isMe: items[0].author === me,
            items: items.sort(byPriority),
          }))
          .sort((a, b) =>
            Number(b.isMe) - Number(a.isMe) ||
            Number(hasRequest(b)) - Number(hasRequest(a)) ||
            latest(b.items) - latest(a.items))
        return { key, label: g.label, color: g.color, count: g.items.length, authors, _latest: latest(g.items) }
      })
      .sort((a, b) => b._latest - a._latest)
      .map(({ _latest, ...g }) => g)
  })

  const isDefaultFilter = computed(() =>
    statusFilter.value.length === DEFAULT_STATUSES.length && DEFAULT_STATUSES.every((s) => statusFilter.value.includes(s)) &&
    roleFilter.value.length === DEFAULT_ROLES.length && DEFAULT_ROLES.every((r) => roleFilter.value.includes(r)),
  )

  /** Badge count: review requests that pass the current filters. */
  const reviewCount = computed(() => filteredItems.value.filter((p) => p.role === 'requested').length)

  function toggleStatus(s: PrStatus) {
    statusFilter.value = statusFilter.value.includes(s)
      ? statusFilter.value.filter((x) => x !== s)
      : [...statusFilter.value, s]
  }

  function toggleRole(r: PrRole) {
    roleFilter.value = roleFilter.value.includes(r)
      ? roleFilter.value.filter((x) => x !== r)
      : [...roleFilter.value, r]
  }

  function resetFilters() {
    statusFilter.value = [...DEFAULT_STATUSES]
    roleFilter.value = [...DEFAULT_ROLES]
  }

  async function refresh() {
    if (loading.value) return
    loading.value = true
    try {
      prs.value = await invoke<PullRequestLists>('get_pull_requests')
      error.value = null
      lastUpdated.value = new Date()
    } catch (e) {
      error.value = String(e)
    } finally {
      loading.value = false
    }
  }

  // Refresh periodically and when the window regains focus (if stale).
  // Counted, so the PR panel and the board can both poll without stopping each other.
  let timer: ReturnType<typeof setInterval> | null = null
  let pollers = 0
  function onFocus() {
    if (!lastUpdated.value || Date.now() - lastUpdated.value.getTime() > 60_000) refresh()
  }

  function startPolling() {
    if (pollers++ > 0) return
    refresh()
    timer = setInterval(refresh, REFRESH_MS)
    window.addEventListener('focus', onFocus)
  }

  function stopPolling() {
    if (pollers === 0 || --pollers > 0) return
    if (timer) clearInterval(timer)
    timer = null
    window.removeEventListener('focus', onFocus)
  }

  return {
    prs, loading, error, lastUpdated, panelOpen,
    statusFilter, roleFilter, toggleStatus, toggleRole, resetFilters, isDefaultFilter,
    allItems, scopedItems, scopeProject, filteredItems, priorityItems, groups, reviewCount,
    refresh, startPolling, stopPolling,
  }
}, {
  persist: { pick: ['panelOpen', 'statusFilter', 'roleFilter'] },
})
