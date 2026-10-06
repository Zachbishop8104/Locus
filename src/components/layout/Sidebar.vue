<script setup lang="ts">
import { computed, ref, nextTick, watch, onMounted, onBeforeUnmount } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { MessageSquare, FolderOpen, Settings, Plus, Trash2, Users, Pencil, ChevronRight } from '@lucide/vue'
import { useChatStore } from '@/stores/chat'
import { useProjectsStore } from '@/stores/projects'
import type { Conversation } from '@/types'
import logoUrl from '@/assets/locus-logo-dark.svg'

const router = useRouter()
const route = useRoute()
const chat = useChatStore()
const projects = useProjectsStore()

const navItems = [
  { path: '/chat', icon: MessageSquare, label: 'Chat' },
  { path: '/teams', icon: Users, label: 'Teams' },
  { path: '/projects', icon: FolderOpen, label: 'Projects' },
]

const isActive = (path: string) => route.path === path

interface Section {
  key: string
  projectId?: string
  name: string
  color?: string
  conversations: Conversation[]
}

const GENERAL = 'general'

/** One section per project (in project order), then chats with no project. */
const sections = computed<Section[]>(() => {
  const byNewest = (a: Conversation, b: Conversation) =>
    new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
  const projectIds = new Set(projects.projects.map((p) => p.id))

  const out: Section[] = projects.projects.map((p) => ({
    key: p.id,
    projectId: p.id,
    name: p.name,
    color: p.color,
    conversations: chat.conversations.filter((c) => c.projectId === p.id).sort(byNewest),
  }))
  // Includes chats whose project was deleted.
  out.push({
    key: GENERAL,
    name: 'General',
    conversations: chat.conversations
      .filter((c) => !c.projectId || !projectIds.has(c.projectId))
      .sort(byNewest),
  })
  return out
})

// ── Collapsed sections (remembered per device) ──
const COLLAPSE_KEY = 'locus.sidebar.collapsed'
function loadCollapsed(): string[] {
  try {
    return JSON.parse(localStorage.getItem(COLLAPSE_KEY) ?? '[]')
  } catch {
    return []
  }
}
const collapsed = ref(new Set<string>(loadCollapsed()))
watch(collapsed, (set) => {
  try {
    localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...set]))
  } catch {
    // storage unavailable; collapse state just won't persist
  }
}, { deep: true })

function toggleSection(key: string) {
  if (collapsed.value.has(key)) collapsed.value.delete(key)
  else collapsed.value.add(key)
}

/** New chat in a section. Reuses an existing empty chat there instead of piling up blanks. */
function startNewChat(section: Section) {
  const empty = section.conversations.find((c) => c.messages.length === 0)
  if (empty) chat.selectConversation(empty.id)
  else chat.newConversation(section.projectId)
  collapsed.value.delete(section.key)
  router.push('/chat')
}

function openConversation(conv: Conversation) {
  if (renamingId.value === conv.id) return
  chat.selectConversation(conv.id)
  router.push('/chat')
}

const renamingId = ref<string | null>(null)
const renameValue = ref('')

// Delete asks inline first; clicking elsewhere or pressing Escape cancels.
const confirmDeleteId = ref<string | null>(null)

function confirmDelete(id: string) {
  chat.deleteConversation(id)
  confirmDeleteId.value = null
}

function cancelDeleteOnOutside() {
  confirmDeleteId.value = null
}

function cancelDeleteOnEscape(e: KeyboardEvent) {
  if (e.key === 'Escape') confirmDeleteId.value = null
}

onMounted(() => {
  document.addEventListener('click', cancelDeleteOnOutside)
  document.addEventListener('keydown', cancelDeleteOnEscape)
})
onBeforeUnmount(() => {
  document.removeEventListener('click', cancelDeleteOnOutside)
  document.removeEventListener('keydown', cancelDeleteOnEscape)
})

function startRename(conv: Conversation) {
  renamingId.value = conv.id
  renameValue.value = conv.title
  nextTick(() => {
    const input = document.querySelector<HTMLInputElement>(`[data-rename-id="${conv.id}"]`)
    input?.focus()
    input?.select()
  })
}

function commitRename() {
  if (renamingId.value && renameValue.value.trim()) {
    chat.renameConversation(renamingId.value, renameValue.value.trim())
  }
  renamingId.value = null
}

// Dark forest tokens scoped to the sidebar
const sidebarVars = {
  '--color-bg': '#162218',
  '--color-surface': '#1e2f26',
  '--color-elevated': '#263d2e',
  '--color-border': '#304a3c',
  '--color-border-strong': '#406054',
  '--color-fg': '#f0f4f2',
  '--color-fg-muted': '#a8c4b4',
  '--color-fg-subtle': '#6a9a82',
  '--color-fg-faint': '#3d5a4a',
  '--color-accent-fg': '#74c99a',
  '--color-accent-icon': '#52B788',
  background: 'linear-gradient(180deg, #1e2f26 0%, #162218 100%)',
}
</script>

<template>
  <aside
    class="flex flex-col w-60 shrink-0 border-r border-border overflow-hidden"
    :style="sidebarVars"
  >
    <!-- Logo -->
    <div class="flex items-center px-4 py-3.5 border-b border-border/60">
      <img :src="logoUrl" alt="Locus" class="h-8 w-auto" />
    </div>

    <!-- Nav -->
    <nav class="flex flex-col gap-0.5 px-2 pt-3">
      <button
        v-for="item in navItems"
        :key="item.path"
        class="flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer border"
        :class="
          isActive(item.path)
            ? 'bg-accent/15 text-accent-fg border-accent-light/20'
            : 'text-fg-muted hover:text-fg hover:bg-elevated/60 border-transparent'
        "
        @click="router.push(item.path)"
      >
        <component :is="item.icon" :size="16" />
        {{ item.label }}
      </button>
    </nav>

    <!-- Conversations, grouped by project -->
    <div class="flex-1 min-h-0 overflow-y-auto mt-4 px-2 pb-2">
      <div v-for="section in sections" :key="section.key" class="mb-2">
        <!-- Section header -->
        <div
          class="group/header flex items-center gap-1.5 pl-1.5 pr-1 py-1 rounded-md cursor-pointer hover:bg-elevated/40 transition-colors"
          @click="toggleSection(section.key)"
        >
          <ChevronRight
            :size="12"
            class="shrink-0 text-fg-subtle transition-transform"
            :class="collapsed.has(section.key) ? '' : 'rotate-90'"
          />
          <div
            v-if="section.color"
            class="w-2 h-2 rounded-full shrink-0"
            :style="{ background: section.color }"
          />
          <span class="flex-1 truncate text-xs font-semibold text-fg-subtle uppercase tracking-wider">
            {{ section.name }}
          </span>
          <span
            v-if="collapsed.has(section.key) && section.conversations.length"
            class="text-[11px] text-fg-faint"
          >{{ section.conversations.length }}</span>
          <button
            class="p-1 rounded text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
            :title="section.projectId ? `New chat in ${section.name}` : 'New chat'"
            @click.stop="startNewChat(section)"
          >
            <Plus :size="14" />
          </button>
        </div>

        <!-- Section conversations -->
        <template v-if="!collapsed.has(section.key)">
          <div
            v-if="section.conversations.length === 0"
            class="pl-7 pr-3 py-1.5 text-xs text-fg-faint"
          >
            No conversations yet
          </div>
          <div
            v-for="conv in section.conversations"
            :key="conv.id"
            class="group w-full flex items-center gap-2 pl-7 pr-2 py-1.5 rounded-lg text-sm transition-colors cursor-pointer"
            :class="
              chat.activeId === conv.id
                ? 'bg-elevated text-fg'
                : 'text-fg-muted hover:text-fg hover:bg-elevated/60'
            "
            @click="openConversation(conv)"
          >
            <!-- Rename input -->
            <input
              v-if="renamingId === conv.id"
              v-model="renameValue"
              :data-rename-id="conv.id"
              class="flex-1 min-w-0 bg-transparent text-fg text-sm outline-none border-b border-accent-fg"
              @click.stop
              @keydown.enter.stop="commitRename"
              @keydown.escape.stop="renamingId = null"
              @blur="commitRename"
            />

            <!-- Normal title -->
            <span
              v-else
              class="flex-1 truncate"
              @dblclick.stop="startRename(conv)"
            >{{ conv.title }}</span>

            <!-- Delete confirmation -->
            <div
              v-if="confirmDeleteId === conv.id"
              class="flex items-center gap-1 shrink-0"
              @click.stop
            >
              <span class="text-xs text-red-300 mr-0.5">Delete?</span>
              <button
                class="px-1.5 py-0.5 rounded text-xs font-medium bg-red-600 hover:bg-red-500 text-white transition-colors cursor-pointer"
                @click.stop="confirmDelete(conv.id)"
              >
                Yes
              </button>
              <button
                class="px-1.5 py-0.5 rounded text-xs text-fg-muted hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
                @click.stop="confirmDeleteId = null"
              >
                Cancel
              </button>
            </div>

            <!-- Action buttons -->
            <template v-else-if="renamingId !== conv.id">
              <button
                class="shrink-0 p-0.5 rounded opacity-0 group-hover:opacity-100 text-fg-subtle hover:text-fg transition-all cursor-pointer"
                title="Rename"
                @click.stop="startRename(conv)"
              >
                <Pencil :size="12" />
              </button>
              <button
                class="shrink-0 p-0.5 rounded opacity-0 group-hover:opacity-100 text-fg-subtle hover:text-red-400 transition-all cursor-pointer"
                title="Delete"
                @click.stop="confirmDeleteId = conv.id"
              >
                <Trash2 :size="12" />
              </button>
            </template>
          </div>
        </template>
      </div>
    </div>

    <!-- Settings link -->
    <div class="px-2 pb-3 border-t border-border/60 pt-2">
      <button
        class="flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer border"
        :class="
          isActive('/settings')
            ? 'bg-accent/15 text-accent-fg border-accent-light/20'
            : 'text-fg-muted hover:text-fg hover:bg-elevated/60 border-transparent'
        "
        @click="router.push('/settings')"
      >
        <Settings :size="16" />
        Settings
      </button>
    </div>
  </aside>
</template>
