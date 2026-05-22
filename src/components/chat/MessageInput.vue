<script setup lang="ts">
import { ref, computed } from 'vue'
import { Send, Square } from '@lucide/vue'
import { MODELS } from '@/types'
import { useSettingsStore } from '@/stores/settings'

defineProps<{ disabled?: boolean; streaming?: boolean }>()
const emit = defineEmits<{
  submit: [text: string]
  cancel: []
}>()

const settings = useSettingsStore()
const text = ref('')
const textarea = ref<HTMLTextAreaElement | null>(null)

const permissionMode = computed({
  get: () => (settings.autoApproveEdits ? 'auto' : 'ask'),
  set: (val: string) => {
    settings.autoApproveEdits = val === 'auto'
  },
})

function submit() {
  const trimmed = text.value.trim()
  if (!trimmed) return
  emit('submit', trimmed)
  text.value = ''
  if (textarea.value) {
    textarea.value.style.height = 'auto'
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    submit()
  }
}

function autoResize(e: Event) {
  const el = e.target as HTMLTextAreaElement
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 200) + 'px'
}
</script>

<template>
  <div class="shrink-0 px-6 pb-5 pt-3 border-t border-border/60">
    <div
      class="flex flex-col gap-2 rounded-xl border bg-surface"
      :class="disabled ? 'border-border/50 opacity-60' : 'border-border-strong focus-within:border-accent-light/60'"
    >
      <textarea
        ref="textarea"
        v-model="text"
        class="w-full bg-transparent px-4 pt-3 pb-1 text-sm text-fg placeholder-fg-subtle resize-none outline-none min-h-11 max-h-50"
        placeholder="Message Locus…"
        rows="1"
        :disabled="disabled || streaming"
        @keydown="onKeydown"
        @input="autoResize"
      />

      <div class="flex items-center justify-between px-3 pb-2.5">
        <!-- Left: model + permission mode -->
        <div class="flex items-center gap-2">
          <select
            v-model="settings.model"
            class="text-xs text-fg-muted bg-transparent border border-border-strong rounded-md px-2 py-1 outline-none hover:border-border-strong cursor-pointer transition-colors"
            :disabled="disabled || streaming"
          >
            <option v-for="m in MODELS" :key="m.id" :value="m.id">
              {{ m.label }}
            </option>
          </select>

          <select
            v-model="permissionMode"
            class="text-xs bg-transparent border rounded-md px-2 py-1 outline-none cursor-pointer transition-colors"
            :class="
              permissionMode === 'auto'
                ? 'border-emerald-700/60 text-emerald-400 hover:border-emerald-600'
                : 'border-border-strong text-fg-muted hover:border-border-strong'
            "
            :disabled="disabled || streaming"
          >
            <option value="ask">Ask permission</option>
            <option value="auto">Auto edit</option>
          </select>
        </div>

        <!-- Right: hint + send/stop -->
        <div class="flex items-center gap-2">
          <span class="text-xs text-fg-faint">⏎ send · ⇧⏎ newline</span>
          <button
            v-if="streaming"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-700 hover:bg-slate-600 text-fg-muted text-xs font-medium transition-colors cursor-pointer"
            @click="emit('cancel')"
          >
            <Square :size="12" />
            Stop
          </button>
          <button
            v-else
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors cursor-pointer"
            :class="
              text.trim() && !disabled
                ? 'bg-accent hover:bg-accent-light text-white'
                : 'bg-elevated text-fg-subtle cursor-not-allowed'
            "
            :disabled="!text.trim() || disabled"
            @click="submit"
          >
            <Send :size="12" />
            Send
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
