<script setup lang="ts">
import { computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { MessageSquare, FolderOpen, Settings, Plus, Trash2, Users } from '@lucide/vue'
import { useChatStore } from '@/stores/chat'
import { useProjectsStore } from '@/stores/projects'
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

function startNewChat() {
  chat.newConversation()
  router.push('/chat')
}

const isActive = (path: string) => route.path === path

const sortedConversations = computed(() =>
  [...chat.conversations].sort(
    (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
  ),
)

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

    <!-- Conversations -->
    <div class="flex flex-col flex-1 min-h-0 mt-4">
      <div class="flex items-center justify-between px-3 mb-1.5">
        <span class="text-xs font-medium text-fg-subtle uppercase tracking-wider">
          Conversations
        </span>
        <button
          class="p-1 rounded text-fg-subtle hover:text-fg-muted hover:bg-elevated transition-colors cursor-pointer"
          title="New chat"
          @click="startNewChat"
        >
          <Plus :size="14" />
        </button>
      </div>

      <div class="flex-1 overflow-y-auto px-2 pb-2">
        <div v-if="sortedConversations.length === 0" class="px-3 py-2 text-xs text-fg-faint">
          No conversations yet
        </div>
        <button
          v-for="conv in sortedConversations"
          :key="conv.id"
          class="group w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm transition-colors cursor-pointer text-left"
          :class="
            chat.activeId === conv.id
              ? 'bg-elevated text-fg'
              : 'text-fg-muted hover:text-fg hover:bg-elevated/60'
          "
          @click="
            () => {
              chat.selectConversation(conv.id)
              router.push('/chat')
            }
          "
        >
          <div
            v-if="conv.projectId && projects.getById(conv.projectId)"
            class="w-1.5 h-1.5 rounded-full shrink-0"
            :style="{ background: projects.getById(conv.projectId)!.color }"
          />
          <span class="flex-1 truncate">{{ conv.title }}</span>
          <button
            class="shrink-0 p-0.5 rounded opacity-0 group-hover:opacity-100 text-fg-subtle hover:text-red-400 transition-all cursor-pointer"
            @click.stop="chat.deleteConversation(conv.id)"
          >
            <Trash2 :size="12" />
          </button>
        </button>
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
