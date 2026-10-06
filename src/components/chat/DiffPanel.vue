<script setup lang="ts">
import { computed } from 'vue'
import { XCircle, CheckCircle } from '@lucide/vue'
import { usePendingEditStore } from '@/stores/pendingEdit'

const pendingEdit = usePendingEditStore()

function lcs(a: string[], b: string[]): number[][] {
  const m = a.length, n = b.length
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0))
  for (let i = 1; i <= m; i++)
    for (let j = 1; j <= n; j++)
      dp[i][j] = a[i-1] === b[j-1] ? dp[i-1][j-1] + 1 : Math.max(dp[i-1][j], dp[i][j-1])
  return dp
}

type SideType = 'equal' | 'remove' | 'add' | 'empty' | 'ellipsis'
interface Side { type: SideType; text: string; lineNo: number | null }
interface Row { left: Side; right: Side }

const CONTEXT = 3

const rows = computed<Row[]>(() => {
  if (!pendingEdit.request) return []

  const normalize = (s: string) => s.replace(/\r\n/g, '\n').replace(/\r/g, '\n').replace(/[ \t]+$/gm, '')
  const oldLines = normalize(pendingEdit.request.currentContent || '').split('\n')
  const newLines = normalize(pendingEdit.request.newContent).split('\n')

  // Unchanged lines at the top and bottom don't need the LCS, so targeted
  // edits to large files stay cheap to diff.
  let prefix = 0
  while (prefix < oldLines.length && prefix < newLines.length && oldLines[prefix] === newLines[prefix]) prefix++
  let suffix = 0
  while (
    suffix < oldLines.length - prefix && suffix < newLines.length - prefix &&
    oldLines[oldLines.length - 1 - suffix] === newLines[newLines.length - 1 - suffix]
  ) suffix++
  const oldMid = oldLines.slice(prefix, oldLines.length - suffix)
  const newMid = newLines.slice(prefix, newLines.length - suffix)

  if (oldMid.length > 1000 || newMid.length > 1000)
    return [{ left: { type: 'equal', text: '(change too large to diff inline)', lineNo: null }, right: { type: 'equal', text: '(change too large to diff inline)', lineNo: null } }]

  const dp = lcs(oldMid, newMid)
  type Flat = { type: 'remove' | 'add' | 'equal'; text: string }
  const mid: Flat[] = []
  let i = oldMid.length, j = newMid.length
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldMid[i-1] === newMid[j-1]) { mid.push({ type: 'equal', text: oldMid[i-1] }); i--; j-- }
    else if (j > 0 && (i === 0 || dp[i][j-1] >= dp[i-1][j])) { mid.push({ type: 'add', text: newMid[j-1] }); j-- }
    else { mid.push({ type: 'remove', text: oldMid[i-1] }); i-- }
  }
  mid.reverse()
  const flat: Flat[] = [
    ...oldLines.slice(0, prefix).map((text) => ({ type: 'equal' as const, text })),
    ...mid,
    ...oldLines.slice(oldLines.length - suffix).map((text) => ({ type: 'equal' as const, text })),
  ]

  let oln = 1, nln = 1
  const numbered = flat.map(l => ({ ...l, oln: l.type !== 'add' ? oln++ : null, nln: l.type !== 'remove' ? nln++ : null }))

  const keep = new Set<number>()
  numbered.forEach((l, idx) => {
    if (l.type !== 'equal')
      for (let k = Math.max(0, idx - CONTEXT); k <= Math.min(numbered.length - 1, idx + CONTEXT); k++) keep.add(k)
  })
  if (keep.size === 0) return []

  const kept = Array.from(keep).sort((a, b) => a - b)
  const out: Row[] = []
  let prev = -2, ptr = 0

  while (ptr < kept.length) {
    const idx = kept[ptr]
    if (idx > prev + 1) out.push({ left: { type: 'ellipsis', text: '…', lineNo: null }, right: { type: 'ellipsis', text: '…', lineNo: null } })
    const line = numbered[idx]
    if (line.type === 'equal') {
      out.push({ left: { type: 'equal', text: line.text, lineNo: line.oln }, right: { type: 'equal', text: line.text, lineNo: line.nln } })
      prev = idx; ptr++
    } else {
      const removes: typeof numbered[0][] = [], adds: typeof numbered[0][] = []
      let p = ptr
      while (p < kept.length && numbered[kept[p]].type === 'remove') { removes.push(numbered[kept[p]]); prev = kept[p]; p++ }
      while (p < kept.length && numbered[kept[p]].type === 'add')    { adds.push(numbered[kept[p]]);    prev = kept[p]; p++ }
      const len = Math.max(removes.length, adds.length)
      for (let r = 0; r < len; r++) {
        const rem = removes[r], add = adds[r]
        out.push({
          left:  rem ? { type: 'remove', text: rem.text, lineNo: rem.oln } : { type: 'empty', text: '', lineNo: null },
          right: add ? { type: 'add',    text: add.text, lineNo: add.nln } : { type: 'empty', text: '', lineNo: null },
        })
      }
      ptr = p
    }
  }
  return out
})

const stats = computed(() => ({
  added:   rows.value.filter(r => r.right.type === 'add').length,
  removed: rows.value.filter(r => r.left.type === 'remove').length,
}))
</script>

<template>
  <div v-if="pendingEdit.request" class="absolute inset-0 z-30 flex flex-col" style="background:#141414">

    <!-- Header -->
    <div class="flex items-center gap-4 px-5 py-3 shrink-0 border-b border-white/[0.06]" style="background:#1a1a1a">
      <!-- File info -->
      <div class="flex-1 min-w-0 flex items-center gap-3">
        <div class="flex flex-col min-w-0">
          <span class="text-[10px] uppercase tracking-widest text-white/30 font-semibold leading-none mb-1">Proposed edit</span>
          <span class="text-sm font-mono text-white/80 truncate leading-none">{{ pendingEdit.request.filePath }}</span>
        </div>
        <!-- Stats badges -->
        <div class="flex items-center gap-1.5 shrink-0">
          <span v-if="stats.removed" class="px-1.5 py-0.5 rounded text-[11px] font-semibold" style="background:rgba(239,68,68,0.15);color:#f87171">
            −{{ stats.removed }}
          </span>
          <span v-if="stats.added" class="px-1.5 py-0.5 rounded text-[11px] font-semibold" style="background:rgba(34,197,94,0.12);color:#4ade80">
            +{{ stats.added }}
          </span>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex items-center gap-2 shrink-0">
        <button
          class="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold transition-all cursor-pointer border"
          style="background:rgba(239,68,68,0.1);border-color:rgba(239,68,68,0.25);color:#f87171"
          onmouseover="this.style.background='rgba(239,68,68,0.2)'"
          onmouseout="this.style.background='rgba(239,68,68,0.1)'"
          @click="pendingEdit.resolve(false)"
        >
          <XCircle :size="14" />
          Reject
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-semibold transition-all cursor-pointer border"
          style="background:rgba(34,197,94,0.1);border-color:rgba(34,197,94,0.2);color:#4ade80"
          onmouseover="this.style.background='rgba(34,197,94,0.2)'"
          onmouseout="this.style.background='rgba(34,197,94,0.1)'"
          @click="pendingEdit.resolve(true)"
        >
          <CheckCircle :size="14" />
          Accept
        </button>
      </div>
    </div>

    <!-- Column labels -->
    <div class="flex shrink-0 border-b border-white/[0.06] text-[11px] font-medium select-none" style="background:#161616">
      <div class="flex-1 flex items-center gap-2 px-4 py-1.5 border-r border-white/[0.06]">
        <span class="w-2 h-2 rounded-full inline-block" style="background:rgba(239,68,68,0.5)" />
        <span style="color:rgba(255,255,255,0.35)">Original</span>
      </div>
      <div class="flex-1 flex items-center gap-2 px-4 py-1.5">
        <span class="w-2 h-2 rounded-full inline-block" style="background:rgba(34,197,94,0.5)" />
        <span style="color:rgba(255,255,255,0.35)">Modified</span>
      </div>
    </div>

    <!-- No changes -->
    <div v-if="!stats.added && !stats.removed" class="flex-1 flex items-center justify-center" style="color:rgba(255,255,255,0.3)">
      No differences detected
    </div>

    <!-- Side-by-side diff -->
    <div v-else class="flex-1 overflow-y-auto font-mono text-xs leading-5">
      <div
        v-for="(row, idx) in rows"
        :key="idx"
        class="flex"
      >
        <!-- Left side -->
        <div
          class="flex flex-1 min-w-0 border-r"
          :style="{
            borderColor: 'rgba(255,255,255,0.04)',
            background: row.left.type === 'remove' ? 'rgba(239,68,68,0.08)'
                      : row.left.type === 'empty'   ? 'rgba(0,0,0,0.2)'
                      : row.left.type === 'ellipsis' ? 'rgba(255,255,255,0.02)'
                      : 'transparent'
          }"
        >
          <!-- Left edge accent for removes -->
          <div
            class="shrink-0 w-0.5"
            :style="{ background: row.left.type === 'remove' ? 'rgba(239,68,68,0.6)' : 'transparent' }"
          />
          <!-- Sign -->
          <span
            class="select-none shrink-0 w-5 text-center py-px"
            :style="{
              color: row.left.type === 'remove' ? 'rgba(248,113,113,0.7)'
                   : row.left.type === 'ellipsis' ? 'rgba(255,255,255,0.15)'
                   : 'transparent'
            }"
          >{{ row.left.type === 'remove' ? '−' : row.left.type === 'ellipsis' ? '' : '' }}</span>
          <!-- Line number -->
          <span
            class="select-none shrink-0 w-10 text-right pr-3 py-px border-r"
            style="border-color:rgba(255,255,255,0.04);color:rgba(255,255,255,0.2)"
          >{{ row.left.lineNo ?? '' }}</span>
          <!-- Content -->
          <span
            v-if="row.left.type !== 'ellipsis'"
            class="py-px pl-3 pr-2 whitespace-pre min-w-0"
            :style="{
              color: row.left.type === 'remove' ? '#fca5a5'
                   : row.left.type === 'empty'   ? 'transparent'
                   : 'rgba(255,255,255,0.45)'
            }"
          >{{ row.left.text }}</span>
          <span v-else class="py-px pl-3 italic text-[10px] self-center" style="color:rgba(255,255,255,0.15)">⋯ unchanged</span>
        </div>

        <!-- Right side -->
        <div
          class="flex flex-1 min-w-0"
          :style="{
            background: row.right.type === 'add'     ? 'rgba(34,197,94,0.07)'
                      : row.right.type === 'empty'   ? 'rgba(0,0,0,0.2)'
                      : row.right.type === 'ellipsis' ? 'rgba(255,255,255,0.02)'
                      : 'transparent'
          }"
        >
          <!-- Left edge accent for adds -->
          <div
            class="shrink-0 w-0.5"
            :style="{ background: row.right.type === 'add' ? 'rgba(34,197,94,0.5)' : 'transparent' }"
          />
          <!-- Sign -->
          <span
            class="select-none shrink-0 w-5 text-center py-px"
            :style="{
              color: row.right.type === 'add' ? 'rgba(74,222,128,0.7)' : 'transparent'
            }"
          >{{ row.right.type === 'add' ? '+' : '' }}</span>
          <!-- Line number -->
          <span
            class="select-none shrink-0 w-10 text-right pr-3 py-px border-r"
            style="border-color:rgba(255,255,255,0.04);color:rgba(255,255,255,0.2)"
          >{{ row.right.lineNo ?? '' }}</span>
          <!-- Content -->
          <span
            v-if="row.right.type !== 'ellipsis'"
            class="py-px pl-3 pr-2 whitespace-pre min-w-0"
            :style="{
              color: row.right.type === 'add'   ? '#86efac'
                   : row.right.type === 'empty' ? 'transparent'
                   : 'rgba(255,255,255,0.45)'
            }"
          >{{ row.right.text }}</span>
          <span v-else class="py-px pl-3 italic text-[10px] self-center" style="color:rgba(255,255,255,0.15)">⋯ unchanged</span>
        </div>
      </div>
    </div>

  </div>
</template>
