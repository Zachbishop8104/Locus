<script setup lang="ts">
import { computed } from 'vue'
import { X, Check } from '@lucide/vue'
import type { EditRequest } from '@/types'

const props = defineProps<{ request: EditRequest }>()
const emit = defineEmits<{ approve: []; deny: [] }>()

type DiffLine = { type: 'equal' | 'remove' | 'add' | 'ellipsis'; text: string }

function lcs(a: string[], b: string[]): number[][] {
  const m = a.length
  const n = b.length
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0))
  for (let i = 1; i <= m; i++)
    for (let j = 1; j <= n; j++)
      dp[i][j] = a[i - 1] === b[j - 1] ? dp[i - 1][j - 1] + 1 : Math.max(dp[i - 1][j], dp[i][j - 1])
  return dp
}

function computeDiff(oldText: string, newText: string): DiffLine[] {
  const oldLines = oldText.split('\n')
  const newLines = newText.split('\n')

  // Cap to avoid slowness on huge files
  if (oldLines.length > 500 || newLines.length > 500) {
    return [{ type: 'equal', text: '(file too large to diff — showing filename only)' }]
  }

  const dp = lcs(oldLines, newLines)
  const raw: DiffLine[] = []
  let i = oldLines.length
  let j = newLines.length
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
      raw.push({ type: 'equal', text: oldLines[i - 1] })
      i--; j--
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      raw.push({ type: 'add', text: newLines[j - 1] })
      j--
    } else {
      raw.push({ type: 'remove', text: oldLines[i - 1] })
      i--
    }
  }
  raw.reverse()

  // Keep only changed lines + 2 lines of context, collapse the rest into ellipsis
  const CONTEXT = 2
  const keep = new Set<number>()
  raw.forEach((l, idx) => {
    if (l.type !== 'equal') {
      for (let k = Math.max(0, idx - CONTEXT); k <= Math.min(raw.length - 1, idx + CONTEXT); k++)
        keep.add(k)
    }
  })

  if (keep.size === 0) return []

  const out: DiffLine[] = []
  let prev = -2
  Array.from(keep).sort((a, b) => a - b).forEach((idx) => {
    if (prev !== -2 && idx > prev + 1) out.push({ type: 'ellipsis', text: '…' })
    out.push(raw[idx])
    prev = idx
  })
  return out
}

const diffLines = computed(() => computeDiff(props.request.currentContent, props.request.newContent))
</script>

<template>
  <div class="mx-6 mb-3 rounded-xl border border-border-strong bg-elevated shadow-md overflow-hidden">

    <!-- Header -->
    <div class="flex items-center gap-3 px-4 py-2.5 border-b border-border/50">
      <div class="flex-1 min-w-0">
        <p class="text-[11px] text-fg-subtle uppercase tracking-wide font-medium">Proposed edit</p>
        <p class="text-sm font-mono text-fg truncate">{{ request.filePath }}</p>
      </div>

      <!-- No -->
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-semibold border transition-all cursor-pointer
               bg-red-950/50 border-red-700/50 text-red-400
               hover:bg-red-600 hover:border-red-500 hover:text-white active:scale-95"
        @click="emit('deny')"
      >
        <X :size="13" stroke-width="2.5" />
        No
      </button>

      <!-- Yes -->
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-semibold border transition-all cursor-pointer
               bg-emerald-950/50 border-emerald-700/50 text-emerald-400
               hover:bg-emerald-500 hover:border-emerald-400 hover:text-white active:scale-95"
        @click="emit('approve')"
      >
        <Check :size="13" stroke-width="2.5" />
        Yes
      </button>
    </div>

    <!-- Diff -->
    <div v-if="diffLines.length" class="max-h-52 overflow-y-auto">
      <div
        v-for="(line, idx) in diffLines"
        :key="idx"
        class="flex items-start gap-2 px-3 py-px font-mono text-xs leading-5 whitespace-pre"
        :class="{
          'bg-emerald-950/50 text-emerald-300': line.type === 'add',
          'bg-red-950/50 text-red-300': line.type === 'remove',
          'text-fg-faint italic': line.type === 'ellipsis',
          'text-fg-subtle': line.type === 'equal',
        }"
      >
        <span class="select-none shrink-0 w-3 opacity-60">
          {{ line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' ' }}
        </span>
        <span>{{ line.text }}</span>
      </div>
    </div>

  </div>
</template>
