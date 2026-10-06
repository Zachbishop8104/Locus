import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { PullRequest, PullRequestLists } from '@/types'

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

export interface PrItem extends PullRequest {
  role: PrRole
  status: PrStatus
}

export interface RepoGroup {
  repo: string
  items: PrItem[]
}

function statusOf(pr: PullRequest): PrStatus {
  if (pr.reviewDecision === 'CHANGES_REQUESTED') return 'changes'
  if (pr.isDraft) return 'draft'
  if (pr.reviewDecision === 'APPROVED') return 'approved'
  return 'review'
}

export const useGithubStore = defineStore('github', () => {
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
    const tag = (list: PullRequest[], role: PrRole) => list.map((pr) => ({ ...pr, role, status: statusOf(pr) }))
    return [...tag(p.reviewRequested, 'requested'), ...tag(p.authored, 'mine'), ...tag(p.involved, 'involved')]
  })

  const filteredItems = computed(() =>
    allItems.value.filter((pr) => statusFilter.value.includes(pr.status) && roleFilter.value.includes(pr.role)),
  )

  /** Repos ordered by most recent activity; review requests first within each repo. */
  const groups = computed<RepoGroup[]>(() => {
    const byRepo = new Map<string, PrItem[]>()
    for (const pr of filteredItems.value) {
      if (!byRepo.has(pr.repo)) byRepo.set(pr.repo, [])
      byRepo.get(pr.repo)!.push(pr)
    }
    const latest = (items: PrItem[]) => Math.max(...items.map((p) => new Date(p.updatedAt).getTime()))
    return [...byRepo.entries()]
      .map(([repo, items]) => ({
        repo,
        items: items.sort((a, b) =>
          Number(b.role === 'requested') - Number(a.role === 'requested') ||
          new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()),
      }))
      .sort((a, b) => latest(b.items) - latest(a.items))
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
  let timer: ReturnType<typeof setInterval> | null = null
  function onFocus() {
    if (!lastUpdated.value || Date.now() - lastUpdated.value.getTime() > 60_000) refresh()
  }

  function startPolling() {
    if (timer) return
    refresh()
    timer = setInterval(refresh, REFRESH_MS)
    window.addEventListener('focus', onFocus)
  }

  function stopPolling() {
    if (timer) clearInterval(timer)
    timer = null
    window.removeEventListener('focus', onFocus)
  }

  return {
    prs, loading, error, lastUpdated, panelOpen,
    statusFilter, roleFilter, toggleStatus, toggleRole, resetFilters, isDefaultFilter,
    allItems, filteredItems, groups, reviewCount,
    refresh, startPolling, stopPolling,
  }
}, {
  persist: { pick: ['panelOpen', 'statusFilter', 'roleFilter'] },
})
