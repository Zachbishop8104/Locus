<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { useRouter } from 'vue-router'
import { ChevronDown, Check, RefreshCw, Sparkles, Server, ArrowRight } from '@lucide/vue'
import { MODELS, EFFORTS } from '@/types'
import type { ModelId } from '@/types'
import { useSettingsStore } from '@/stores/settings'

defineProps<{ disabled?: boolean }>()

const settings = useSettingsStore()
const router = useRouter()
const open = ref(false)
const root = ref<HTMLElement | null>(null)

const claudeVia = computed(() => (settings.useLocalClaude ? 'Claude Code' : 'API key'))

const showEffort = computed(
  () => settings.activeProvider === 'claude' && settings.claudeModel.supportsEffort,
)

const effortLabel = computed(() => EFFORTS.find((e) => e.id === settings.effort)?.label)

function toggle() {
  open.value = !open.value
  if (open.value && settings.useLocalModel && settings.localModels.length === 0) {
    settings.refreshLocalModels()
  }
}

function pickClaude(id: ModelId) {
  settings.selectClaudeModel(id)
  if (!MODELS.find((m) => m.id === id)?.supportsEffort) open.value = false
}

function pickLocal(name: string) {
  settings.selectLocalModel(name)
  open.value = false
}

function goToSettings() {
  open.value = false
  router.push('/settings')
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
      class="flex items-center gap-1.5 text-xs text-fg-muted border border-border-strong rounded-md px-2 py-1 hover:text-fg hover:bg-elevated/60 transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
      :disabled="disabled"
      @click="toggle"
    >
      <Server v-if="settings.activeProvider === 'local'" :size="12" class="text-fg-subtle" />
      <Sparkles v-else :size="12" class="text-accent-icon" />
      <span class="font-medium">{{ settings.activeModelLabel }}</span>
      <span v-if="showEffort" class="text-fg-subtle">· {{ effortLabel }}</span>
      <ChevronDown :size="12" class="text-fg-subtle transition-transform" :class="open ? 'rotate-180' : ''" />
    </button>

    <div
      v-if="open"
      class="absolute bottom-full left-0 mb-2 w-80 rounded-xl border border-border-strong bg-surface shadow-xl z-30 overflow-hidden"
    >
      <!-- ─── Claude ─── -->
      <div class="px-3 pt-3 pb-1 flex items-center justify-between">
        <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Claude</span>
        <span v-if="settings.claudeReady" class="text-[11px] text-fg-faint">via {{ claudeVia }}</span>
      </div>

      <div v-if="settings.claudeReady" class="px-1.5 pb-1.5">
        <button
          v-for="m in MODELS"
          :key="m.id"
          class="w-full flex items-center gap-3 px-2 py-2 rounded-lg text-left hover:bg-elevated transition-colors cursor-pointer"
          @click="pickClaude(m.id)"
        >
          <div class="flex-1 min-w-0">
            <p class="text-sm text-fg font-medium">{{ m.label }}</p>
            <p class="text-xs text-fg-subtle truncate">{{ m.description }}</p>
          </div>
          <Check
            v-if="settings.activeProvider === 'claude' && settings.claudeModel.id === m.id"
            :size="15"
            class="text-accent-icon shrink-0"
          />
        </button>
      </div>
      <button
        v-else
        class="mx-3 mb-3 w-[calc(100%-1.5rem)] flex items-center justify-between px-3 py-2 rounded-lg border border-dashed border-border-strong text-xs text-fg-muted hover:text-fg hover:border-accent/60 transition-colors cursor-pointer"
        @click="goToSettings"
      >
        Connect Claude with an API key or Claude Code
        <ArrowRight :size="13" />
      </button>

      <!-- Effort -->
      <div v-if="showEffort" class="px-3 pb-3">
        <div class="flex items-center justify-between mb-1.5">
          <span class="text-xs text-fg-muted">Effort</span>
          <span class="text-[11px] text-fg-faint">Higher thinks longer and uses more tokens</span>
        </div>
        <div class="grid grid-cols-5 gap-1 p-0.5 rounded-lg bg-elevated">
          <button
            v-for="e in EFFORTS"
            :key="e.id"
            class="text-xs py-1 rounded-md transition-colors cursor-pointer"
            :class="settings.effort === e.id ? 'bg-surface text-fg font-medium shadow-sm' : 'text-fg-subtle hover:text-fg'"
            @click="settings.effort = e.id"
          >
            {{ e.label }}
          </button>
        </div>
      </div>

      <!-- ─── Local ─── -->
      <div class="border-t border-border">
        <div class="px-3 pt-3 pb-1 flex items-center justify-between">
          <span class="text-[11px] font-semibold uppercase tracking-wide text-fg-subtle">Local models</span>
          <button
            v-if="settings.useLocalModel"
            class="text-fg-subtle hover:text-fg transition-colors cursor-pointer"
            title="Refresh"
            @click="settings.refreshLocalModels()"
          >
            <RefreshCw :size="12" :class="settings.fetchingLocalModels ? 'animate-spin' : ''" />
          </button>
        </div>

        <template v-if="settings.useLocalModel">
          <div class="px-1.5 pb-1.5 max-h-48 overflow-y-auto">
            <button
              v-for="name in settings.localModels"
              :key="name"
              class="w-full flex items-center gap-3 px-2 py-1.5 rounded-lg text-left hover:bg-elevated transition-colors cursor-pointer"
              @click="pickLocal(name)"
            >
              <span class="flex-1 min-w-0 text-sm text-fg font-mono truncate">{{ name }}</span>
              <Check
                v-if="settings.activeProvider === 'local' && settings.localModelName === name"
                :size="15"
                class="text-accent-icon shrink-0"
              />
            </button>
            <p v-if="settings.fetchingLocalModels && !settings.localModels.length" class="px-2 py-2 text-xs text-fg-subtle">
              Loading…
            </p>
            <p
              v-else-if="!settings.localModels.length"
              class="px-2 py-2 text-xs text-fg-subtle"
            >
              {{ settings.localModelsError ? `Couldn't reach ${settings.localModelUrl}` : 'No models found.' }}
            </p>
          </div>
        </template>
        <button
          v-else
          class="mx-3 mb-3 w-[calc(100%-1.5rem)] flex items-center justify-between px-3 py-2 rounded-lg border border-dashed border-border-strong text-xs text-fg-muted hover:text-fg hover:border-accent/60 transition-colors cursor-pointer"
          @click="goToSettings"
        >
          Add Ollama, LM Studio, or another local server
          <ArrowRight :size="13" />
        </button>
      </div>
    </div>
  </div>
</template>
