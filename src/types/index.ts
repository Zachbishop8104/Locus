export interface Message {
  id: string
  role: 'user' | 'assistant'
  content: string
  timestamp: string
  error?: boolean
  cancelled?: boolean  // rejected write — shown in chat but excluded from API context
  /** What the model receives when it differs from what is shown, e.g. a /skill expanded into its instructions. */
  sent?: string
}

export interface Conversation {
  id: string
  title: string
  messages: Message[]
  createdAt: string
  projectId?: string
  /** Which of the project's folders this chat works in. Falls back to the first one. */
  localPaths?: string[]
  /** The board card (Jira issue, task-master task or PR) this chat is about. */
  task?: TaskRef
  /** Text to put in the message box when the chat is next opened. */
  draft?: string
}

export type TaskSource = 'jira' | 'taskmaster' | 'pr'

/** A snapshot of a board card, saved on the chats linked to it. */
export interface TaskRef {
  /** Stable card key, e.g. `jira:LOC-12`, `tm:C:\repo#3`, `pr:owner/repo#42`. */
  key: string
  source: TaskSource
  /** What the card shows as its id: `LOC-12`, `#3`, `repo#42`. */
  label: string
  title: string
  url?: string
}

export interface Project {
  id: string
  name: string
  description: string
  localPaths: string[]
  gitRepo: string
  jiraProject: string
  color: string
  createdAt: string
}

export interface EditRequest {
  toolUseId: string
  filePath: string
  currentContent: string
  newContent: string
}

export const PROJECT_COLORS = [
  '#2D6A4F',
  '#40916C',
  '#52B788',
  '#1D6A8B',
  '#8B5E3C',
  '#7B5EA7',
  '#C1692C',
  '#1B5E9E',
]

export type ModelId =
  | 'claude-fable-5-1'
  | 'claude-opus-5-5'
  | 'claude-sonnet-5-5'
  | 'claude-haiku-4-5'

export interface ModelOption {
  id: ModelId
  label: string
  description: string
  supportsEffort: boolean
}

export const MODELS: ModelOption[] = [
  { id: 'claude-fable-5-1', label: 'Fable 5.1', description: 'Most capable, for the hardest work', supportsEffort: true },
  { id: 'claude-opus-5-5', label: 'Opus 5.5', description: 'Deep reasoning for complex tasks', supportsEffort: true },
  { id: 'claude-sonnet-5-5', label: 'Sonnet 5.5', description: 'Fast and capable for everyday coding', supportsEffort: true },
  { id: 'claude-haiku-4-5', label: 'Haiku 4.5', description: 'Fastest, for quick answers', supportsEffort: false },
]

export const DEFAULT_MODEL: ModelId = 'claude-sonnet-5-5'

export type Effort = 'low' | 'medium' | 'high' | 'xhigh' | 'max'

export const EFFORTS: { id: Effort; label: string }[] = [
  { id: 'low', label: 'Low' },
  { id: 'medium', label: 'Med' },
  { id: 'high', label: 'High' },
  { id: 'xhigh', label: 'X-High' },
  { id: 'max', label: 'Max' },
]

/** Which family of models the picker has selected. */
export type ProviderKind = 'claude' | 'local'

/** What the backend routes on. */
export type BackendProvider = 'claude_api' | 'claude_cli' | 'local'

/** Permission modes, matching Claude Code. */
export type PermissionMode = 'ask' | 'edits' | 'plan'

export const MODES: { id: PermissionMode; label: string; description: string }[] = [
  { id: 'ask', label: 'Ask permissions', description: 'Approve each file edit before it is written' },
  { id: 'edits', label: 'Auto-accept edits', description: 'Write file edits without asking' },
  { id: 'plan', label: 'Plan mode', description: 'Explore and propose a plan — no edits until you approve' },
]

export interface ClaudeCliStatus {
  installed: boolean
  version: string | null
  loggedIn: boolean
  authMethod: string | null
  error: string | null
}

export interface PullRequest {
  id: string
  number: number
  title: string
  url: string
  repo: string
  author: string
  authorAvatar: string | null
  isDraft: boolean
  updatedAt: string
  reviewDecision: 'APPROVED' | 'CHANGES_REQUESTED' | 'REVIEW_REQUIRED' | null
  checks: 'SUCCESS' | 'FAILURE' | 'ERROR' | 'PENDING' | 'EXPECTED' | null
  comments: number
  /** Your latest verdict on someone else's PR. */
  viewerReview: 'APPROVED' | 'CHANGES_REQUESTED' | 'DISMISSED' | null
  /** You commented, or left a comment-only review. */
  viewerCommented: boolean
}

export interface PullRequestLists {
  login: string
  reviewRequested: PullRequest[]
  authored: PullRequest[]
  involved: PullRequest[]
}

export interface JiraIssue {
  key: string
  project: string
  projectName: string | null
  summary: string
  status: string
  statusCategory: 'new' | 'indeterminate' | 'done' | string
  issueType: string | null
  priority: string | null
  assignee: string | null
  updated: string | null
  url: string | null
}

export interface TmFolder {
  path: string
  remote: string | null
}

export type TmStatus = 'pending' | 'in-progress' | 'review' | 'done' | 'blocked' | 'deferred' | 'cancelled'

export interface TmTask {
  id: string
  title: string
  description: string
  details: string
  status: TmStatus
  priority: string | null
  dependencies: string[]
  testStrategy: string
  subtasks: TmSubtask[]
  subtasksTotal: number
  subtasksDone: number
}

export interface TmSubtask {
  id: string
  title: string
  description: string
  details: string
  status: TmStatus
}

export interface JiraIssueLink {
  key: string
  summary: string
  status: string | null
}

export interface JiraIssueDetail {
  key: string
  /** Markdown. */
  description: string
  issueType: string | null
  reporter: string | null
  created: string | null
  labels: string[]
  parent: JiraIssueLink | null
  subtasks: JiraIssueLink[]
  comments: { author: string | null; created: string | null; body: string }[]
}

export interface Skill {
  name: string
  description: string
  source: 'user' | 'project' | 'plugin'
  /** `skill`: a SKILL.md folder. `command`: a single .md file that takes $ARGUMENTS. */
  kind: 'skill' | 'command'
  path: string
}
