<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { Hand, FastForward, ClipboardList, Check } from '@lucide/vue'
import { MODES } from '@/types'
import type { PermissionMode } from '@/types'
import { useSettingsStore } from '@/stores/settings'

defineProps<{ disabled?: boolean }>()

const settings = useSettingsStore()
const open = ref(false)
const root = ref<HTMLElement | null>(null)

const ICONS = { ask: Hand, edits: FastForward, plan: ClipboardList } as const

const STYLES: Record<PermissionMode, string> = {
  ask: 'border-border-strong text-fg-muted hover:text-fg',
  edits: 'border-amber-500/50 bg-amber-50 text-amber-700 hover:border-amber-500',
  plan: 'border-sky-500/50 bg-sky-50 text-sky-700 hover:border-sky-500',
}

const current = computed(() => MODES.find((m) => m.id === settings.mode) ?? MODES[0])

function pick(id: PermissionMode) {
  settings.mode = id
  open.value = false
}

function onDocClick(e: MouseEvent) {
  if (open.value && root.value && !root.value.contains(e.target as Node)) open.value = false
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}

onMounted(() => {
  document.addEventListener('mousedown', onDocClick)
  document.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onDocClick)
  document.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div ref="root" class="relative">
    <button
      class="flex items-center gap-1.5 text-xs border rounded-md px-2 py-1 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
      :class="STYLES[current.id]"
      :disabled="disabled"
      :title="`${current.description} (Shift+Tab to switch)`"
      @click="open = !open"
    >
      <component :is="ICONS[current.id]" :size="12" />
      <span class="font-medium">{{ current.label }}</span>
    </button>

    <div
      v-if="open"
      class="absolute bottom-full left-0 mb-2 w-72 rounded-xl border border-border-strong bg-surface shadow-xl z-30 overflow-hidden"
    >
      <div class="p-1.5">
        <button
          v-for="m in MODES"
          :key="m.id"
          class="w-full flex items-start gap-3 px-2 py-2 rounded-lg text-left hover:bg-elevated transition-colors cursor-pointer"
          @click="pick(m.id)"
        >
          <component :is="ICONS[m.id]" :size="14" class="mt-0.5 shrink-0 text-fg-subtle" />
          <div class="flex-1 min-w-0">
            <p class="text-sm text-fg font-medium">{{ m.label }}</p>
            <p class="text-xs text-fg-subtle">{{ m.description }}</p>
          </div>
          <Check v-if="m.id === settings.mode" :size="15" class="mt-0.5 text-accent-icon shrink-0" />
        </button>
      </div>
      <div class="px-3 py-2 border-t border-border text-[11px] text-fg-faint">
        <kbd class="font-sans">⇧ Tab</kbd> to switch modes
      </div>
    </div>
  </div>
</template>
