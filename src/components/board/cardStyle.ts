import type { Component } from 'vue'
import { Bug, Bookmark, SquareCheckBig, Zap, Network, ListTodo, Ticket } from '@lucide/vue'
import { Marked } from 'marked'
import type { BoardCard } from '@/stores/board'

/** Look shared by board cards and the card detail view. */

/** Stripe down the card's left edge, by source. */
export const SOURCE_STRIPE: Record<BoardCard['source'], string> = {
  jira: 'bg-sky-500',
  taskmaster: 'bg-accent',
}

const PRIORITY_TAG: Record<string, string> = {
  highest: 'bg-red-50 text-red-700 border-red-200',
  critical: 'bg-red-50 text-red-700 border-red-200',
  high: 'bg-orange-50 text-orange-700 border-orange-200',
  medium: 'bg-amber-50 text-amber-700 border-amber-200',
  low: 'bg-sky-50 text-sky-700 border-sky-200',
  lowest: 'bg-sky-50 text-sky-600 border-sky-200',
}
export const priorityTag = (p: string | null) => PRIORITY_TAG[(p ?? '').toLowerCase()] ?? null

/** Icon and colour for a card's kind: the Jira issue type, or a task-master task. */
export function cardKind(card: Pick<BoardCard, 'source' | 'issueType'>): { icon: Component; cls: string; label: string } {
  if (card.source === 'taskmaster') return { icon: ListTodo, cls: 'text-accent-icon', label: 'Task' }
  const type = card.issueType ?? 'Issue'
  const t = type.toLowerCase()
  if (t.includes('bug')) return { icon: Bug, cls: 'text-red-600', label: type }
  if (t.includes('story')) return { icon: Bookmark, cls: 'text-emerald-600', label: type }
  if (t.includes('epic')) return { icon: Zap, cls: 'text-violet-600', label: type }
  if (t.includes('sub')) return { icon: Network, cls: 'text-sky-600', label: type }
  if (t.includes('task')) return { icon: SquareCheckBig, cls: 'text-sky-600', label: type }
  return { icon: Ticket, cls: 'text-sky-600', label: type }
}

/** task-master and Jira status names → a tag colour. */
export function statusTag(status: string): string {
  const s = status.toLowerCase()
  if (/done|closed|resolved/.test(s)) return 'bg-emerald-50 text-emerald-700 border-emerald-200'
  if (/block/.test(s)) return 'bg-red-50 text-red-700 border-red-200'
  if (/review|qa|test/.test(s)) return 'bg-violet-50 text-violet-700 border-violet-200'
  if (/progress/.test(s)) return 'bg-sky-50 text-sky-700 border-sky-200'
  if (/defer|cancel/.test(s)) return 'bg-elevated text-fg-subtle border-border'
  return 'bg-elevated text-fg-muted border-border'
}

const escapeHtml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

// Ticket text comes from Jira and task files, so raw HTML in it is shown as text.
const md = new Marked({ renderer: { html: ({ text }) => escapeHtml(text) } })

export const renderMarkdown = (text: string) => md.parse(text, { async: false }) as string
