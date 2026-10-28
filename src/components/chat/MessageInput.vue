<script setup lang="ts">
import { ref, computed, nextTick, watch } from 'vue'
import { Send, Square } from '@lucide/vue'
import { MODES } from '@/types'
import type { Skill } from '@/types'
import { useSettingsStore } from '@/stores/settings'
import ModelPicker from './ModelPicker.vue'
import ModePicker from './ModePicker.vue'

const props = defineProps<{ disabled?: boolean; streaming?: boolean; skills?: Skill[] }>()
const emit = defineEmits<{
  submit: [text: string]
  cancel: []
}>()

const settings = useSettingsStore()
const text = ref('')
const textarea = ref<HTMLTextAreaElement | null>(null)

const placeholder = computed(() =>
  settings.mode === 'plan' ? 'Describe what you want to build — Locus will plan first…' : 'Message Locus…',
)

function submit() {
  const trimmed = text.value.trim()
  if (!trimmed) return
  emit('submit', trimmed)
  text.value = ''
  if (textarea.value) {
    textarea.value.style.height = 'auto'
  }
}

// ── Slash menu: typing `/` lists skills and commands ──

const menuIndex = ref(0)
/** Esc hides the menu until the text changes. */
const dismissed = ref(false)
const menu = ref<HTMLElement | null>(null)

/** The `/word` being typed, while nothing else has been typed after it. */
const slashQuery = computed(() => {
  const m = text.value.match(/^\/(\S*)$/)
  return m ? m[1].toLowerCase() : null
})

/** Names starting with the query first, then names containing it, then description matches. */
const matches = computed<Skill[]>(() => {
  const q = slashQuery.value
  if (q === null || !props.skills?.length) return []
  const score = (s: Skill) => {
    const name = s.name.toLowerCase()
    if (name.startsWith(q) || name.split(':').pop()!.startsWith(q)) return 0
    if (name.includes(q)) return 1
    if (q.length > 1 && s.description.toLowerCase().includes(q)) return 2
    return -1
  }
  return props.skills
    .map((s) => ({ s, rank: score(s) }))
    .filter((x) => x.rank >= 0)
    .sort((a, b) => a.rank - b.rank)
    .map((x) => x.s)
})

const menuOpen = computed(() => !dismissed.value && matches.value.length > 0)

watch(slashQuery, () => {
  menuIndex.value = 0
  dismissed.value = false
})

function moveMenu(delta: number) {
  const n = matches.value.length
  menuIndex.value = (menuIndex.value + delta + n) % n
  nextTick(() => menu.value?.querySelector(`[data-index="${menuIndex.value}"]`)?.scrollIntoView({ block: 'nearest' }))
}

function pickSkill(skill: Skill) {
  setText(`/${skill.name} `)
}

const SOURCE_LABEL: Record<Skill['source'], string> = { user: 'Personal', project: 'Project', plugin: 'Plugin' }

function cycleMode() {
  const idx = MODES.findIndex((m) => m.id === settings.mode)
  settings.mode = MODES[(idx + 1) % MODES.length].id
}

function onKeydown(e: KeyboardEvent) {
  if (menuOpen.value) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      return moveMenu(e.key === 'ArrowDown' ? 1 : -1)
    }
    if ((e.key === 'Enter' && !e.shiftKey) || (e.key === 'Tab' && !e.shiftKey)) {
      e.preventDefault()
      return pickSkill(matches.value[menuIndex.value])
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      dismissed.value = true
      return
    }
  }
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    submit()
  } else if (e.key === 'Tab' && e.shiftKey) {
    e.preventDefault()
    cycleMode()
  }
}

function autoResize(e: Event) {
  const el = e.target as HTMLTextAreaElement
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 200) + 'px'
}

function focus() {
  textarea.value?.focus()
}

/** Replaces the message box text and puts the cursor at the end. */
function setText(value: string) {
  text.value = value
  nextTick(() => {
    const el = textarea.value
    if (!el) return
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 200) + 'px'
    el.focus()
    el.setSelectionRange(value.length, value.length)
  })
}

defineExpose({ focus, setText })
</script>

<template>
  <div class="shrink-0 px-6 pb-5 pt-3 border-t border-border/60">
    <div
      class="relative flex flex-col gap-2 rounded-xl border bg-surface"
      :class="disabled ? 'border-border/50 opacity-60' : 'border-border-strong focus-within:border-accent-light/60'"
    >
      <!-- Slash menu -->
      <div
        v-if="menuOpen"
        ref="menu"
        class="absolute bottom-full left-0 right-0 mb-2 max-h-72 overflow-y-auto rounded-xl border border-border-strong bg-surface shadow-xl z-30 p-1.5"
      >
        <button
          v-for="(s, i) in matches"
          :key="s.name"
          :data-index="i"
          class="w-full flex items-baseline gap-2 px-2.5 py-1.5 rounded-lg text-left transition-colors cursor-pointer"
          :class="i === menuIndex ? 'bg-elevated' : 'hover:bg-elevated/60'"
          @mousedown.prevent="pickSkill(s)"
          @mousemove="menuIndex = i"
        >
          <span class="shrink-0 font-mono text-sm text-fg">/{{ s.name }}</span>
          <span class="flex-1 min-w-0 truncate text-xs text-fg-subtle" :title="s.description">{{ s.description }}</span>
          <span class="shrink-0 text-[10px] uppercase tracking-wide text-fg-faint">{{ SOURCE_LABEL[s.source] }}</span>
        </button>
      </div>

      <textarea
        ref="textarea"
        v-model="text"
        class="w-full bg-transparent px-4 pt-3 pb-1 text-sm text-fg placeholder-fg-subtle resize-none outline-none min-h-11 max-h-50"
        :placeholder="placeholder"
        rows="1"
        :disabled="disabled || streaming"
        @keydown="onKeydown"
        @input="autoResize"
      />

      <div class="flex items-center justify-between px-3 pb-2.5">
        <!-- Left: model + permission mode -->
        <div class="flex items-center gap-2">
          <ModelPicker :disabled="disabled || streaming" />
          <ModePicker :disabled="disabled" />
        </div>

        <!-- Right: hint + send/stop -->
        <div class="flex items-center gap-2">
          <span class="text-xs text-fg-faint">/ skills · ⏎ send · ⇧⏎ newline · ⇧Tab mode</span>
          <button
            v-if="streaming"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-700 hover:bg-slate-600 text-white text-xs font-medium transition-colors cursor-pointer"
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
