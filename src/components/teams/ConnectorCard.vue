<script setup lang="ts">
import { computed } from 'vue'
import { marked } from 'marked'
import { ExternalLink, GitPullRequest } from '@lucide/vue'
import { invoke } from '@tauri-apps/api/core'
import type { TeamsAttachment } from '@/stores/teams'

const props = defineProps<{ attachment: TeamsAttachment }>()

// ── AC types ───────────────────────────────────────────────────────────────────
interface ACTextBlock { type: 'TextBlock'; text: string; weight?: string; size?: string; color?: string; isSubtle?: boolean; wrap?: boolean }
interface ACImage     { type: 'Image'; url: string; altText?: string; size?: string }
interface ACFactSet   { type: 'FactSet'; facts: { title: string; value: string }[] }
interface ACColumn    { type: 'Column'; items?: ACElement[]; width?: string }
interface ACColumnSet { type: 'ColumnSet'; columns?: ACColumn[] }
interface ACContainer { type: 'Container'; items?: ACElement[] }
type ACElement = ACTextBlock | ACImage | ACFactSet | ACColumnSet | ACContainer | { type: string }
interface ACActionOpenUrl { type: string; title: string; url?: string }
interface AdaptiveCard { type: 'AdaptiveCard'; body?: ACElement[]; actions?: ACActionOpenUrl[] }

// ── MC types ───────────────────────────────────────────────────────────────────
interface MCFact    { name: string; value: string }
interface MCSection { title?: string; activityTitle?: string; activitySubtitle?: string; activityText?: string; activityImage?: string; facts?: MCFact[]; text?: string }
interface MCAction  { '@type': string; name: string; targets?: { os: string; uri: string }[]; uri?: string }
interface MessageCard { '@type': 'MessageCard'; themeColor?: string; title?: string; summary?: string; text?: string; sections?: MCSection[]; potentialAction?: MCAction[] }

// ── Parsed card ────────────────────────────────────────────────────────────────
const parsed = computed(() => {
  try { return props.attachment.content ? JSON.parse(props.attachment.content) : null }
  catch { return null }
})
const isMessageCard = computed(() => parsed.value?.['@type'] === 'MessageCard')
const isAdaptiveCard = computed(() => parsed.value?.type === 'AdaptiveCard')
const mc = computed<MessageCard | null>(() => isMessageCard.value ? parsed.value : null)
const ac = computed<AdaptiveCard | null>(() => isAdaptiveCard.value ? parsed.value : null)

// ── Flat item types ────────────────────────────────────────────────────────────
type TextFI        = { kind: 'text';    text: string; bold?: boolean; large?: boolean; subtle?: boolean; color?: string }
type RowFI         = { kind: 'row';     items: { text: string; color?: string }[] }
type PersonFI      = { kind: 'person';  url?: string; name: string }
type PersonGroupFI = { kind: 'persons'; people: { url?: string; name: string }[] }
type FactsFI       = { kind: 'facts';   facts: { title: string; value: string }[] }
type ImageFI       = { kind: 'image';   url: string; alt?: string }
type FlatItem = TextFI | RowFI | PersonFI | PersonGroupFI | FactsFI | ImageFI

// ── Helpers ────────────────────────────────────────────────────────────────────
const SEPARATOR_RE = /^[\s|·•\-–—\/\\]+$/
function dedupKey(s: string) { return s.replace(/[\s▼▲↑↓⬆⬇]+$/, '').trim() }

function flattenAC(elements: ACElement[] = [], seen = new Set<string>()): FlatItem[] {
  const out: FlatItem[] = []

  for (const el of elements) {
    if (el.type === 'TextBlock') {
      const tb = el as ACTextBlock
      const text = tb.text?.trim()
      if (!text || SEPARATOR_RE.test(text)) continue
      const key = dedupKey(text)
      if (seen.has(key)) continue
      seen.add(key)
      out.push({ kind: 'text', text: tb.text,
        bold: tb.weight === 'Bolder' || tb.weight === 'Bold',
        large: tb.size === 'Large' || tb.size === 'ExtraLarge',
        subtle: tb.isSubtle === true,
        color: tb.color })

    } else if (el.type === 'FactSet') {
      out.push({ kind: 'facts', facts: (el as ACFactSet).facts })

    } else if (el.type === 'Image') {
      const img = el as ACImage
      if (img.url) out.push({ kind: 'image', url: img.url, alt: img.altText })

    } else if (el.type === 'Container') {
      out.push(...flattenAC((el as ACContainer).items, seen))

    } else if (el.type === 'ColumnSet') {
      const cols = (el as ACColumnSet).columns ?? []

      const imageCols = cols.filter(c => c.items?.length === 1 && c.items[0].type === 'Image')
      const textCols  = cols.filter(c => c.items?.length === 1 && c.items[0].type === 'TextBlock')

      if (imageCols.length > 0 && textCols.length > 0 && cols.length <= 4) {
        // Interleaved person chips (avatar + @name)
        for (const col of cols) {
          if (!col.items?.length) continue
          const item = col.items[0]
          if (item.type === 'Image') {
            const img = item as ACImage
            const nextIdx = cols.indexOf(col) + 1
            const next = cols[nextIdx]?.items?.[0]
            const name = next?.type === 'TextBlock' ? (next as ACTextBlock).text?.trim() : undefined
            if (name) {
              const key = dedupKey(name)
              if (!seen.has(key)) {
                seen.add(key)
                out.push({ kind: 'person', url: img.url, name })
              }
            } else {
              out.push({ kind: 'image', url: img.url })
            }
          } else if (item.type === 'TextBlock') {
            const text = (item as ACTextBlock).text?.trim()
            if (!text) continue
            const key = dedupKey(text)
            if (seen.has(key)) continue
          }
        }
      } else {
        // Text-only columns → inline row
        const rowItems: { text: string; color?: string }[] = []
        for (const col of cols) {
          for (const item of col.items ?? []) {
            if (item.type === 'TextBlock') {
              const tb = item as ACTextBlock
              const text = tb.text?.trim()
              if (!text || SEPARATOR_RE.test(text)) continue
              const key = dedupKey(text)
              if (seen.has(key)) continue
              seen.add(key)
              rowItems.push({ text: tb.text, color: tb.color })
            } else {
              out.push(...flattenAC([item], seen))
            }
          }
        }
        if (rowItems.length === 1) {
          const r = rowItems[0]
          out.push({ kind: 'text', text: r.text, color: r.color })
        } else if (rowItems.length > 1) {
          out.push({ kind: 'row', items: rowItems })
        }
      }
    }
  }
  return out
}

// Merge consecutive PersonFI items into a single PersonGroupFI for chip layout
function acItems(card: AdaptiveCard): FlatItem[] {
  const raw = flattenAC(card.body)
  const out: FlatItem[] = []
  let group: PersonFI[] = []

  for (const item of raw) {
    if (item.kind === 'person') {
      group.push(item)
    } else {
      if (group.length === 1) out.push(group[0])
      else if (group.length > 1) out.push({ kind: 'persons', people: group.map(p => ({ url: p.url, name: p.name })) })
      group = []
      out.push(item)
    }
  }
  if (group.length === 1) out.push(group[0])
  else if (group.length > 1) out.push({ kind: 'persons', people: group.map(p => ({ url: p.url, name: p.name })) })
  return out
}

// ── Color helpers ──────────────────────────────────────────────────────────────
function acColorClass(color?: string) {
  switch (color?.toLowerCase()) {
    case 'good':      return 'text-emerald-400'
    case 'warning':   return 'text-amber-400'
    case 'attention': return 'text-red-400'
    case 'accent':    return 'text-accent-fg'
    case 'dark':      return 'text-fg'
    case 'light':     return 'text-fg-subtle'
    default:          return 'text-fg-muted'
  }
}

function renderMd(text?: string) {
  if (!text) return ''
  return marked.parse(text, { async: false }) as string
}

function mcThemeColor(card: MessageCard) {
  const c = card.themeColor?.replace('#', '')
  return c ? `#${c}` : 'var(--color-accent)'
}
function mcActionUrl(a: MCAction) {
  return a.targets?.find(t => t.os === 'default' || t.os === 'windows')?.uri ?? a.uri ?? ''
}

function openUrl(url?: string) { if (url) invoke('open_url', { url }) }
function hideImg(e: Event) { (e.target as HTMLImageElement).style.display = 'none' }
</script>

<template>
  <!-- ── O365 MessageCard ──────────────────────────────────────────────────── -->
  <div
    v-if="mc"
    class="rounded-xl border border-border bg-surface overflow-hidden text-sm max-w-lg w-full"
  >
    <div class="h-1 w-full shrink-0" :style="{ background: mcThemeColor(mc) }" />
    <div class="px-4 py-3 flex flex-col gap-2.5">

      <div class="flex items-start gap-2">
        <GitPullRequest :size="15" class="text-fg-subtle shrink-0 mt-0.5" />
        <p v-if="mc.title" class="font-semibold text-fg leading-snug">{{ mc.title }}</p>
      </div>

      <div v-if="mc.text" class="text-fg-muted leading-relaxed teams-card-prose" v-html="renderMd(mc.text)" />

      <div v-for="(section, i) in mc.sections" :key="i"
        class="flex flex-col gap-1.5"
        :class="i > 0 ? 'pt-2.5 border-t border-border' : ''"
      >
        <div v-if="section.activityTitle" class="flex items-start gap-2.5">
          <img v-if="section.activityImage" :src="section.activityImage"
            class="w-7 h-7 rounded-full shrink-0 object-cover mt-0.5" @error="hideImg" />
          <div class="min-w-0">
            <div class="text-fg text-sm font-medium leading-snug teams-card-prose" v-html="renderMd(section.activityTitle)" />
            <div v-if="section.activitySubtitle" class="text-fg-subtle text-xs mt-0.5 teams-card-prose" v-html="renderMd(section.activitySubtitle)" />
          </div>
        </div>
        <div v-if="section.activityText" class="text-fg-muted text-xs leading-relaxed teams-card-prose" v-html="renderMd(section.activityText)" />
        <div v-if="section.text && !section.activityTitle" class="text-fg-muted leading-relaxed teams-card-prose" v-html="renderMd(section.text)" />
        <p v-if="section.title" class="text-fg-subtle text-xs font-semibold uppercase tracking-wide">{{ section.title }}</p>
        <div v-if="section.facts?.length" class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5 mt-0.5">
          <template v-for="fact in section.facts" :key="fact.name">
            <span class="text-fg-subtle text-xs">{{ fact.name }}</span>
            <span class="text-fg-muted text-xs truncate">{{ fact.value }}</span>
          </template>
        </div>
      </div>

      <div v-if="mc.potentialAction?.some(a => a['@type'] === 'OpenUri')"
        class="flex flex-wrap gap-1.5 pt-0.5 border-t border-border mt-0.5"
      >
        <button
          v-for="action in mc.potentialAction?.filter(a => a['@type'] === 'OpenUri')"
          :key="action.name"
          class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-elevated hover:bg-border-strong text-fg-muted hover:text-fg transition-colors cursor-pointer border border-border"
          @click="openUrl(mcActionUrl(action))"
        >
          <ExternalLink :size="11" /> {{ action.name }}
        </button>
      </div>
    </div>
  </div>

  <!-- ── Adaptive Card ─────────────────────────────────────────────────────── -->
  <div
    v-else-if="ac"
    class="rounded-xl border border-border bg-surface overflow-hidden text-sm max-w-lg w-full"
  >
    <div class="h-1 w-full shrink-0 bg-accent" />
    <div class="px-4 py-3 flex flex-col gap-2">

      <template v-for="(item, i) in acItems(ac)" :key="i">

        <!-- Standalone image -->
        <img v-if="item.kind === 'image'"
          :src="item.url" :alt="item.alt"
          class="w-8 h-8 rounded-full object-cover"
          @error="hideImg"
        />

        <!-- Single person chip (avatar + name) -->
        <div v-else-if="item.kind === 'person'" class="flex items-center gap-1.5">
          <img v-if="item.url" :src="item.url"
            class="w-5 h-5 rounded-full object-cover shrink-0"
            @error="hideImg"
          />
          <span class="text-xs text-accent-icon">{{ item.name }}</span>
        </div>

        <!-- Multiple person chips (reviewers) — wrapping row -->
        <div v-else-if="item.kind === 'persons'" class="flex flex-wrap gap-1.5">
          <div
            v-for="p in item.people"
            :key="p.name"
            class="flex items-center gap-1.5 bg-elevated/70 border border-border rounded-full pl-0.5 pr-2.5 py-0.5"
          >
            <img v-if="p.url" :src="p.url"
              class="w-4 h-4 rounded-full object-cover shrink-0"
              @error="hideImg"
            />
            <div v-else class="w-4 h-4 rounded-full bg-accent/30 shrink-0 flex items-center justify-center text-[9px] text-accent-fg font-semibold">
              {{ p.name.charAt(0).toUpperCase() }}
            </div>
            <span class="text-xs text-accent-icon leading-none">{{ p.name }}</span>
          </div>
        </div>

        <!-- Inline row (e.g. "+384  ·  -21" or "3 Reviewers · 0 Comments · 11 Files") -->
        <div v-else-if="item.kind === 'row'" class="flex items-center gap-2 flex-wrap">
          <template v-for="(r, ri) in item.items" :key="ri">
            <span v-if="ri > 0" class="text-fg-faint text-xs select-none">·</span>
            <span class="text-xs font-medium" :class="acColorClass(r.color)">{{ r.text }}</span>
          </template>
        </div>

        <!-- Normal text block -->
        <div v-else-if="item.kind === 'text'"
          class="leading-snug teams-card-prose"
          :class="[
            item.bold && item.large ? 'text-base font-semibold text-fg' :
            item.bold ? 'font-semibold text-fg-muted' :
            item.large ? 'text-base text-fg' : 'text-sm',
            !item.bold && !item.large ? (item.subtle ? 'text-fg-subtle' : acColorClass(item.color)) : '',
          ]"
          v-html="renderMd(item.text)"
        />

        <!-- Facts grid -->
        <div v-else-if="item.kind === 'facts'" class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5">
          <template v-for="fact in item.facts" :key="fact.title">
            <span class="text-fg-subtle text-xs">{{ fact.title }}</span>
            <span class="text-fg-muted text-xs truncate">{{ fact.value }}</span>
          </template>
        </div>

      </template>

      <div v-if="ac.actions?.some(a => a.url)"
        class="flex flex-wrap gap-1.5 pt-1.5 border-t border-border mt-0.5"
      >
        <button
          v-for="action in ac.actions?.filter(a => a.url)"
          :key="action.title"
          class="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-elevated hover:bg-border-strong text-fg-muted hover:text-fg transition-colors cursor-pointer border border-border"
          @click="openUrl(action.url)"
        >
          <ExternalLink :size="11" /> {{ action.title }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.teams-card-prose :deep(p) { margin: 0 0 2px; }
.teams-card-prose :deep(p:last-child) { margin: 0; }
.teams-card-prose :deep(a) { color: var(--color-accent-icon); text-decoration: underline; }
.teams-card-prose :deep(strong) { color: var(--color-fg); font-weight: 600; }
.teams-card-prose :deep(code) { background: var(--color-elevated); padding: 1px 4px; border-radius: 3px; font-size: 0.8em; color: #1b5e3a; font-family: ui-monospace, monospace; }
.teams-card-prose :deep(ul), .teams-card-prose :deep(ol) { padding-left: 1.2rem; margin: 2px 0; }
.teams-card-prose :deep(li) { margin: 1px 0; }
</style>
