export interface Message {
  id: string
  role: 'user' | 'assistant'
  content: string
  timestamp: string
  error?: boolean
  cancelled?: boolean  // rejected write — shown in chat but excluded from API context
}

export interface Conversation {
  id: string
  title: string
  messages: Message[]
  createdAt: string
  projectId?: string
}

export interface Project {
  id: string
  name: string
  description: string
  localPath: string
  gitRepo: string
  jiraProject: string
  color: string
  createdAt: string
  dbType?: string
  dbConnectionString?: string
}

export interface EditRequest {
  toolUseId: string
  filePath: string
  currentContent: string
  newContent: string
}

export const DB_TYPES = [
  { value: 'sqlserver', label: 'SQL Server' },
] as const

export type DbType = typeof DB_TYPES[number]['value']

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
