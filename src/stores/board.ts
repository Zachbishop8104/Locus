import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Conversation, JiraIssue, Project, TaskRef, TmFolder, TmStatus, TmTask } from '@/types'
import { useChatStore } from './chat'
import { useProjectsStore } from './projects'
import { useGithubStore, normalizeRepo } from './github'
import type { PrItem } from './github'

export type ColumnId = 'todo' | 'in_progress' | 'review' | 'done'

export const COLUMNS: { id: ColumnId; label: string }[] = [
  { id: 'todo', label: 'To do' },
  { id: 'in_progress', label: 'In progress' },
  { id: 'review', label: 'In review' },
  { id: 'done', label: 'Done' },
]

/** Status a task-master card moves to when dropped in a column. (Jira picks a transition by column.) */
const TM_STATUS: Record<ColumnId, TmStatus> = {
  todo: 'pending',
  in_progress: 'in-progress',
  review: 'review',
  done: 'done',
}

export interface BoardCard {
  key: string
  source: 'jira' | 'taskmaster'
  label: string
  title: string
  description: string
  column: ColumnId
  /** The source's own status name. */
  status: string
  /** Shown as a warning tag (blocked, deferred). */
  flag: string | null
  priority: string | null
  /** Jira issue type (Bug, Story, Task, Epic…). */
  issueType: string | null
  assignee: string | null
  /** Jira's last update. */
  updated: string | null
  url: string | null
  project: Project | null
  /** Shown instead of a project when the card isn't tied to one: a folder or Jira project name. */
  group: string | null
  subtasks: { done: number; total: number } | null
  /** Jira issue key, or task-master folder + id. */
  jiraKey?: string
  tmPath?: string
  tmId?: string
}

const PRIORITY_RANK: Record<string, number> = { highest: 0, critical: 0, high: 1, medium: 2, low: 3, lowest: 4 }
const rank = (p: string | null) => PRIORITY_RANK[(p ?? '').toLowerCase()] ?? 2

function jiraColumn(issue: JiraIssue): ColumnId {
  if (issue.statusCategory === 'done') return 'done'
  if (/review|qa|testing/i.test(issue.status)) return 'review'
  if (issue.statusCategory === 'indeterminate') return 'in_progress'
  return 'todo'
}

function tmColumn(status: TmStatus): ColumnId {
  switch (status) {
    case 'in-progress': return 'in_progress'
    case 'review': return 'review'
    case 'done': return 'done'
    default: return 'todo'
  }
}

function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

/**
 * Jira project keys from a project's Jira field, which may hold keys (`SJ, SJPT`)
 * or Jira URLs (`…/jira/software/projects/SJ/boards/1`, `…/browse/SJ-12`, `?project=SJ`).
 */
export function parseJiraKeys(value: string | undefined): string[] {
  const keys = new Set<string>()
  for (const part of (value ?? '').split(/[,\s]+/).filter(Boolean)) {
    const m = part.match(/\/projects\/([A-Za-z][A-Za-z0-9_]*)/)
      ?? part.match(/\/browse\/([A-Za-z][A-Za-z0-9_]*)-\d+/)
      ?? part.match(/[?&]project(?:Key)?=([A-Za-z][A-Za-z0-9_]*)/)
      ?? (part.includes('/') ? null : part.match(/^([A-Za-z][A-Za-z0-9_]*)$/))
    if (m) keys.add(m[1].toUpperCase())
  }
  return [...keys]
}

/** For comparing folder paths: case, slash direction and trailing slashes don't matter. */
const normalizePath = (p: string) => p.trim().replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()

function parentFolder(path: string): string | null {
  const m = path.trim().replace(/[\\/]+$/, '').match(/^(.*)[\\/][^\\/]+$/)
  return m && m[1] ? m[1] : null
}

export const useBoardStore = defineStore('board', () => {
  const chat = useChatStore()
  const projects = useProjectsStore()
  const github = useGithubStore()

  // ── Persisted UI state ──
  /** A project id, or 'all'. */
  const projectFilter = ref<string>('all')
  const showJira = ref(true)
  const showTaskmaster = ref(true)
  const showPrs = ref(true)
  const jiraMineOnly = ref(false)
  /** task-master folders added by hand, on top of project folders and their neighbours. */
  const extraFolders = ref<string[]>([])

  // ── Loaded data ──
  const jiraIssues = ref<JiraIssue[]>([])
  /** 'JIRA_NOT_INSTALLED' | 'JIRA_NOT_AUTHENTICATED' | any other message */
  const jiraError = ref<string | null>(null)
  const tmTasks = ref<Record<string, TmTask[]>>({})
  /** Per folder. 'TM_NOT_INSTALLED' | any other message */
  const tmErrors = ref<Record<string, string>>({})
  /** Folders in scope without a `.taskmaster/` directory. */
  const tmUninitialized = ref<string[]>([])
  /** task-master folders found next to project folders and added folders. */
  const discovered = ref<TmFolder[]>([])
  const loading = ref(false)
  const lastUpdated = ref<Date | null>(null)
  /** Last failed move or add, shown until dismissed. */
  const actionError = ref<string | null>(null)
  /** Cards with a status change in flight. */
  const pending = ref<Set<string>>(new Set())

  const scopeProjects = computed<Project[]>(() =>
    projectFilter.value === 'all'
      ? projects.projects
      : projects.projects.filter((p) => p.id === projectFilter.value),
  )

  const jiraKeys = computed(() => [
    ...new Set(scopeProjects.value.flatMap((p) => parseJiraKeys(p.jiraProject))),
  ])

  /**
   * Folder → project (or null), for every folder in scope: the projects' own folders,
   * then task-master folders found nearby or added by hand. A found folder belongs to a
   * project when it is one of the project's folders or its git remote is the project's
   * repo; folders that match no project only show under "All projects".
   */
  const tmFolders = computed(() => {
    const map = new Map<string, Project | null>()
    const seen = new Set<string>()
    const add = (path: string, project: Project | null) => {
      const key = normalizePath(path)
      if (!key || seen.has(key)) return
      seen.add(key)
      map.set(path, project)
    }
    for (const p of scopeProjects.value) for (const path of p.localPaths ?? []) add(path, p)

    const found: TmFolder[] = [
      ...discovered.value,
      ...extraFolders.value.map((path) => ({ path, remote: null })),
    ]
    for (const f of found) {
      const key = normalizePath(f.path)
      const remote = f.remote ? normalizeRepo(f.remote) : ''
      const project = projects.projects.find((p) =>
        (p.localPaths ?? []).some((lp) => normalizePath(lp) === key) ||
        (!!remote && normalizeRepo(p.gitRepo ?? '') === remote),
      ) ?? null
      if (project ? scopeProjects.value.includes(project) : projectFilter.value === 'all') add(f.path, project)
    }
    return map
  })

  /** Where to look for task-master folders: project folders, added folders, and their parents. */
  const searchRoots = computed(() => {
    const paths = [...projects.projects.flatMap((p) => p.localPaths ?? []), ...extraFolders.value].filter((p) => p.trim())
    const roots = new Map<string, string>()
    for (const p of [...paths, ...paths.map(parentFolder)]) if (p) roots.set(normalizePath(p), p)
    return [...roots.values()]
  })

  const cards = computed<BoardCard[]>(() => {
    const out: BoardCard[] = []
    if (showJira.value) {
      const byKey = new Map(projects.projects.flatMap((p) => parseJiraKeys(p.jiraProject).map((k) => [k, p] as const)))
      for (const issue of jiraIssues.value) {
        const project = byKey.get(issue.project) ?? null
        out.push({
          key: `jira:${issue.key}`,
          source: 'jira',
          label: issue.key,
          title: issue.summary,
          description: '',
          column: jiraColumn(issue),
          status: issue.status,
          flag: /blocked/i.test(issue.status) ? 'Blocked' : null,
          priority: issue.priority,
          issueType: issue.issueType,
          assignee: issue.assignee,
          updated: issue.updated,
          url: issue.url,
          project,
          group: project ? null : issue.projectName ?? issue.project,
          subtasks: null,
          jiraKey: issue.key,
        })
      }
    }
    if (showTaskmaster.value) {
      const multiFolder = (p: Project | null) => !!p && (p.localPaths ?? []).filter((x) => x.trim()).length > 1
      for (const [path, project] of tmFolders.value) {
        for (const t of tmTasks.value[path] ?? []) {
          if (t.status === 'cancelled') continue
          out.push({
            key: `tm:${path}#${t.id}`,
            source: 'taskmaster',
            label: multiFolder(project) ? `${folderName(path)}#${t.id}` : `#${t.id}`,
            title: t.title,
            description: t.description,
            column: tmColumn(t.status),
            status: t.status,
            flag: t.status === 'blocked' ? 'Blocked' : t.status === 'deferred' ? 'Deferred' : null,
            priority: t.priority,
            issueType: null,
            assignee: null,
            updated: null,
            url: null,
            project,
            group: project ? null : folderName(path),
            subtasks: t.subtasksTotal ? { done: t.subtasksDone, total: t.subtasksTotal } : null,
            tmPath: path,
            tmId: t.id,
          })
        }
      }
    }
    return out
  })

  const columns = computed(() =>
    COLUMNS.map((c) => ({
      ...c,
      cards: cards.value.filter((card) => card.column === c.id).sort((a, b) => rank(a.priority) - rank(b.priority)),
    })),
  )

  /** Open PRs for the projects in scope (all PRs involving you when no project is picked). */
  const prs = computed<PrItem[]>(() => {
    if (!showPrs.value) return []
    if (projectFilter.value === 'all') return github.allItems
    const repos = new Set(scopeProjects.value.map((p) => normalizeRepo(p.gitRepo ?? '')).filter(Boolean))
    return github.allItems.filter((pr) => repos.has(normalizeRepo(pr.repo)))
  })

  function projectForPr(pr: PrItem): Project | null {
    const repo = normalizeRepo(pr.repo)
    return projects.projects.find((p) => normalizeRepo(p.gitRepo ?? '') === repo) ?? null
  }

  // ── Loading ──

  async function loadJira() {
    // Only the Jira projects linked to Locus projects. With none linked, "All projects"
    // falls back to whatever is assigned to you in Jira.
    const includeMine = projectFilter.value === 'all' && !jiraKeys.value.length
    if (!jiraKeys.value.length && !includeMine) {
      jiraIssues.value = []
      jiraError.value = null
      return
    }
    try {
      jiraIssues.value = await invoke<JiraIssue[]>('get_jira_issues', {
        projects: jiraKeys.value,
        includeMine,
        mineOnly: jiraMineOnly.value,
      })
      jiraError.value = null
    } catch (e) {
      jiraError.value = String(e)
    }
  }

  async function loadTaskmasterFolder(path: string) {
    try {
      tmTasks.value[path] = await invoke<TmTask[]>('get_taskmaster_tasks', { path })
      delete tmErrors.value[path]
    } catch (e) {
      const msg = String(e)
      delete tmTasks.value[path]
      if (msg === 'TM_NOT_INITIALIZED') {
        if (!tmUninitialized.value.includes(path)) tmUninitialized.value.push(path)
      } else {
        tmErrors.value[path] = msg
      }
    }
  }

  async function discoverFolders() {
    try {
      discovered.value = await invoke<TmFolder[]>('find_taskmaster_folders', { roots: searchRoots.value })
    } catch {
      // keep the last result; project folders still load
    }
  }

  async function loadTaskmaster() {
    await discoverFolders()
    const paths = [...tmFolders.value.keys()]
    tmUninitialized.value = []
    tmErrors.value = {}
    await Promise.all(paths.map(loadTaskmasterFolder))
  }

  async function refresh() {
    if (loading.value) return
    loading.value = true
    try {
      await Promise.all([
        showJira.value ? loadJira() : null,
        showTaskmaster.value ? loadTaskmaster() : null,
        showPrs.value ? github.refresh() : null,
      ])
      lastUpdated.value = new Date()
    } finally {
      loading.value = false
    }
  }

  // ── Changes ──

  /** Moves a card to another column, updating Jira or task-master. */
  async function moveCard(card: BoardCard, to: ColumnId) {
    if (card.column === to || pending.value.has(card.key)) return
    pending.value.add(card.key)
    actionError.value = null
    try {
      if (card.source === 'jira' && card.jiraKey) {
        await invoke('set_jira_column', { key: card.jiraKey, column: to })
        await loadJira()
      } else if (card.source === 'taskmaster' && card.tmPath && card.tmId) {
        await invoke('set_taskmaster_status', { path: card.tmPath, id: card.tmId, status: TM_STATUS[to] })
        await loadTaskmasterFolder(card.tmPath)
      }
    } catch (e) {
      const column = COLUMNS.find((c) => c.id === to)!.label
      actionError.value = card.source === 'jira'
        ? `Couldn't move ${card.label} to ${column}: ${e}`
        : `Couldn't update task ${card.label}: ${e}`
    } finally {
      pending.value.delete(card.key)
    }
  }

  async function addTask(path: string, title: string, description: string) {
    actionError.value = null
    try {
      await invoke('add_taskmaster_task', { path, title, description })
      await loadTaskmasterFolder(path)
    } catch (e) {
      actionError.value = `Couldn't add the task: ${e}`
      throw e
    }
  }

  async function initTaskmaster(path: string) {
    actionError.value = null
    try {
      await invoke('init_taskmaster', { path })
      tmUninitialized.value = tmUninitialized.value.filter((p) => p !== path)
      await loadTaskmasterFolder(path)
    } catch (e) {
      actionError.value = `Couldn't set up task-master in ${folderName(path)}: ${e}`
    }
  }

  /** Adds a folder by hand. Returns false when it has no `.taskmaster/` yet. */
  async function addFolder(path: string): Promise<boolean> {
    if (!extraFolders.value.some((p) => normalizePath(p) === normalizePath(path))) extraFolders.value.push(path)
    await loadTaskmaster()
    return !tmUninitialized.value.includes(path)
  }

  function removeFolder(path: string) {
    extraFolders.value = extraFolders.value.filter((p) => normalizePath(p) !== normalizePath(path))
    delete tmTasks.value[path]
  }

  /** The full task-master task behind a card. */
  function tmTaskFor(card: BoardCard): TmTask | null {
    if (!card.tmPath || !card.tmId) return null
    return tmTasks.value[card.tmPath]?.find((t) => t.id === card.tmId) ?? null
  }

  // ── Chats ──

  function chatsFor(key: string): Conversation[] {
    return chat.conversations
      .filter((c) => c.task?.key === key)
      .sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
  }

  function refFor(card: BoardCard): TaskRef {
    return { key: card.key, source: card.source, label: card.label, title: card.title, url: card.url ?? undefined }
  }

  function refForPr(pr: PrItem): TaskRef {
    return { key: `pr:${pr.repo}#${pr.number}`, source: 'pr', label: `${pr.repo.split('/').pop()}#${pr.number}`, title: pr.title, url: pr.url }
  }

  function draftFor(card: BoardCard): string {
    if (card.source === 'jira') {
      return `Let's work on Jira issue ${card.label}: "${card.title}" (currently ${card.status}).${card.url ? `\n${card.url}` : ''}\n\n`
    }
    const lines = [`Let's work on task-master task ${card.label}: "${card.title}".`]
    if (card.description && card.description !== card.title) lines.push('', card.description)
    const details = tmTaskFor(card)?.details
    if (details) lines.push('', details)
    return lines.join('\n') + '\n\n'
  }

  function draftForPr(pr: PrItem): string {
    return `Let's look at pull request ${pr.repo}#${pr.number}: "${pr.title}".\n${pr.url}\n\n`
  }

  /** Starts a chat linked to the card, with a first message ready to edit. Returns its id. */
  function startChat(ref: TaskRef, project: Project | null, draft: string, localPath?: string): string {
    const id = chat.newConversation(project?.id)
    const conv = chat.conversations.find((c) => c.id === id)!
    conv.task = ref
    conv.title = `${ref.label} ${ref.title}`.slice(0, 80)
    conv.draft = draft
    if (localPath) conv.localPaths = [localPath]
    return id
  }

  function startChatForCard(card: BoardCard): string {
    return startChat(refFor(card), card.project, draftFor(card), card.tmPath)
  }

  function startChatForPr(pr: PrItem): string {
    return startChat(refForPr(pr), projectForPr(pr), draftForPr(pr))
  }

  return {
    projectFilter, showJira, showTaskmaster, showPrs, jiraMineOnly, extraFolders,
    jiraIssues, jiraError, tmTasks, tmErrors, tmUninitialized, discovered, loading, lastUpdated, actionError, pending,
    scopeProjects, jiraKeys, tmFolders, cards, columns, prs,
    refresh, loadJira, moveCard, addTask, initTaskmaster, addFolder, removeFolder,
    tmTaskFor, chatsFor, refForPr, projectForPr, startChatForCard, startChatForPr,
  }
}, {
  persist: { pick: ['projectFilter', 'showJira', 'showTaskmaster', 'showPrs', 'jiraMineOnly', 'extraFolders'] },
})
