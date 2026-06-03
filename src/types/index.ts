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
  | 'claude-opus-4-7'
  | 'claude-sonnet-4-6'
  | 'claude-haiku-4-5-20251001'

export interface ModelOption {
  id: ModelId
  label: string
  description: string
}

export const MODELS: ModelOption[] = [
  { id: 'claude-opus-4-7', label: 'Claude Opus 4.7', description: 'Most capable' },
  { id: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6', description: 'Balanced' },
  { id: 'claude-haiku-4-5-20251001', label: 'Claude Haiku 4.5', description: 'Fastest' },
]
