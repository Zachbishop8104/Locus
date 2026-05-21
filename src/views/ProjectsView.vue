<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { Plus, Folder, FolderOpen, Trash2, MessageSquare, ExternalLink, GitBranch, Ticket, Database } from '@lucide/vue'
import { invoke } from '@tauri-apps/api/core'
import { useProjectsStore } from '@/stores/projects'
import { useChatStore } from '@/stores/chat'
import type { Project } from '@/types'
import { PROJECT_COLORS, DB_TYPES } from '@/types'

const router = useRouter()
const projectsStore = useProjectsStore()
const chatStore = useChatStore()

// ── Selected project + edit form ─────────────────────────────────────────────
const selectedId = ref<string | 'new' | null>(null)

const blankForm = (): Omit<Project, 'id' | 'createdAt'> => ({
  name: '',
  description: '',
  localPath: '',
  gitRepo: '',
  jiraProject: '',
  color: projectsStore.nextColor(),
  dbType: '',
  dbConnectionString: '',
})

const form = ref(blankForm())

function selectProject(p: Project) {
  selectedId.value = p.id
  form.value = {
    name: p.name,
    description: p.description,
    localPath: p.localPath,
    gitRepo: p.gitRepo,
    jiraProject: p.jiraProject,
    color: p.color,
    dbType: p.dbType ?? '',
    dbConnectionString: p.dbConnectionString ?? '',
  }
}

function startNew() {
  selectedId.value = 'new'
  form.value = blankForm()
}

function save() {
  if (!form.value.name.trim()) return
  if (selectedId.value === 'new') {
    const p = projectsStore.createProject(form.value)
    selectedId.value = p.id
  } else if (selectedId.value) {
    projectsStore.updateProject(selectedId.value, form.value)
  }
}

function remove() {
  if (!selectedId.value || selectedId.value === 'new') return
  // Delete all conversations in this project
  const convs = chatStore.conversationsForProject(selectedId.value)
  convs.forEach(c => chatStore.deleteConversation(c.id))
  projectsStore.deleteProject(selectedId.value)
  selectedId.value = null
}

async function browsePath() {
  const path = await invoke<string | null>('pick_folder')
  if (path) form.value.localPath = path
}

function openChat(convId: string) {
  chatStore.selectConversation(convId)
  router.push('/chat')
}

function newChat() {
  if (!selectedId.value || selectedId.value === 'new') return
  const id = chatStore.newConversation(selectedId.value)
  chatStore.selectConversation(id)
  router.push('/chat')
}

const projectConversations = computed(() =>
  selectedId.value && selectedId.value !== 'new'
    ? chatStore.conversationsForProject(selectedId.value)
    : []
)

function formatDate(iso: string) {
  const d = new Date(iso)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' })
}
</script>

<template>
  <div class="flex h-full">

    <!-- Left: project list -->
    <div class="flex flex-col w-56 shrink-0 border-r border-border overflow-hidden">
      <div class="flex items-center justify-between px-4 py-3.5 border-b border-border/60 shrink-0">
        <span class="text-sm font-semibold text-fg">Projects</span>
        <button
          class="p-1 rounded text-fg-subtle hover:text-fg hover:bg-elevated transition-colors cursor-pointer"
          title="New project"
          @click="startNew"
        >
          <Plus :size="15" />
        </button>
      </div>

      <div class="flex-1 overflow-y-auto py-1">
        <div v-if="projectsStore.projects.length === 0 && selectedId !== 'new'" class="px-4 py-6 text-xs text-fg-faint text-center">
          No projects yet.<br/>Click + to create one.
        </div>

        <button
          v-for="p in projectsStore.projects"
          :key="p.id"
          class="flex items-center gap-2.5 w-full px-3 py-2.5 text-left transition-colors cursor-pointer"
          :class="selectedId === p.id
            ? 'bg-elevated text-fg'
            : 'text-fg-muted hover:bg-elevated/60 hover:text-fg'"
          @click="selectProject(p)"
        >
          <div class="w-2.5 h-2.5 rounded-full shrink-0" :style="{ background: p.color }" />
          <span class="text-sm truncate font-medium">{{ p.name }}</span>
        </button>

        <button
          v-if="selectedId === 'new'"
          class="flex items-center gap-2.5 w-full px-3 py-2.5 text-left bg-elevated text-fg"
        >
          <div class="w-2.5 h-2.5 rounded-full shrink-0" :style="{ background: form.color }" />
          <span class="text-sm truncate italic text-fg-subtle">New project…</span>
        </button>
      </div>
    </div>

    <!-- Right: detail / form -->
    <div class="flex-1 overflow-y-auto">

      <!-- Empty state -->
      <div v-if="!selectedId" class="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
        <Folder :size="32" class="text-fg-faint" />
        <div>
          <p class="text-fg-subtle font-medium text-sm">No project selected</p>
          <p class="text-fg-faint text-xs mt-1">Select a project or create a new one</p>
        </div>
        <button
          class="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-light transition-colors cursor-pointer"
          @click="startNew"
        >
          <Plus :size="14" /> New Project
        </button>
      </div>

      <!-- Form -->
      <div v-else class="max-w-2xl px-8 py-6 flex flex-col gap-6">

        <!-- Header row -->
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <FolderOpen :size="18" class="text-fg-subtle" />
            <h2 class="text-base font-semibold text-fg">
              {{ selectedId === 'new' ? 'New Project' : form.name || 'Project' }}
            </h2>
          </div>
          <div class="flex items-center gap-2">
            <button
              v-if="selectedId !== 'new'"
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-accent text-white hover:bg-accent-light transition-colors cursor-pointer"
              @click="newChat"
            >
              <MessageSquare :size="12" /> New Chat
            </button>
            <button
              v-if="selectedId !== 'new'"
              class="p-1.5 rounded-lg text-fg-subtle hover:text-red-500 hover:bg-red-50 transition-colors cursor-pointer"
              title="Delete project"
              @click="remove"
            >
              <Trash2 :size="15" />
            </button>
          </div>
        </div>

        <!-- Color picker -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide">Color</label>
          <div class="flex gap-2 flex-wrap">
            <button
              v-for="c in PROJECT_COLORS"
              :key="c"
              class="w-6 h-6 rounded-full transition-transform cursor-pointer hover:scale-110"
              :style="{ background: c }"
              :class="form.color === c ? 'ring-2 ring-offset-2 ring-offset-white ring-fg-subtle scale-110' : ''"
              @click="form.color = c"
            />
          </div>
        </div>

        <!-- Name -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide">Name</label>
          <input
            v-model="form.name"
            type="text"
            placeholder="My Project"
            class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm placeholder-fg-faint outline-none focus:border-accent-light transition-colors"
          />
        </div>

        <!-- Description -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide">Description</label>
          <textarea
            v-model="form.description"
            rows="3"
            placeholder="What this project is, the tech stack, goals — anything that helps Claude give relevant answers."
            class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm placeholder-fg-faint outline-none focus:border-accent-light transition-colors resize-none"
          />
        </div>

        <!-- Local path -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide">Local Folder</label>
          <div class="flex gap-2">
            <input
              v-model="form.localPath"
              type="text"
              placeholder="/Users/you/projects/my-app"
              class="flex-1 px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm placeholder-fg-faint outline-none focus:border-accent-light transition-colors font-mono"
            />
            <button
              class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg-muted hover:text-fg hover:bg-border text-xs font-medium transition-colors cursor-pointer shrink-0"
              @click="browsePath"
            >
              Browse
            </button>
          </div>
        </div>

        <!-- Git repo -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide flex items-center gap-1.5">
            <GitBranch :size="12" /> Git Repository
          </label>
          <input
            v-model="form.gitRepo"
            type="text"
            placeholder="https://github.com/org/repo"
            class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm placeholder-fg-faint outline-none focus:border-accent-light transition-colors font-mono"
          />
        </div>

        <!-- Jira -->
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide flex items-center gap-1.5">
            <Ticket :size="12" /> Jira Project
          </label>
          <input
            v-model="form.jiraProject"
            type="text"
            placeholder="https://yourorg.atlassian.net/jira/software/projects/MYAPP"
            class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm placeholder-fg-faint outline-none focus:border-accent-light transition-colors font-mono"
          />
        </div>

        <!-- Database -->
        <div class="flex flex-col gap-3">
          <label class="text-xs font-medium text-fg-subtle uppercase tracking-wide flex items-center gap-1.5">
            <Database :size="12" /> Database
          </label>
          <select
            v-model="form.dbType"
            class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm outline-none focus:border-accent-light transition-colors cursor-pointer"
          >
            <option value="">None</option>
            <option v-for="db in DB_TYPES" :key="db.value" :value="db.value">{{ db.label }}</option>
          </select>
          <div v-if="form.dbType" class="flex flex-col gap-1.5">
            <label class="text-xs text-fg-subtle">Connection String</label>
            <input
              v-model="form.dbConnectionString"
              type="password"
              placeholder="Server=localhost;Database=mydb;User Id=sa;Password=...;TrustServerCertificate=True;"
              class="px-3 py-2 rounded-lg border border-border bg-elevated text-fg text-sm placeholder-fg-faint outline-none focus:border-accent-light transition-colors font-mono"
            />
            <p class="text-xs text-fg-faint">
              Stored locally. Use a read-only SQL account. Add <code class="bg-elevated px-1 rounded">TrustServerCertificate=True</code> for local instances with self-signed certificates.
            </p>
          </div>
        </div>

        <!-- Save -->
        <div>
          <button
            class="px-5 py-2 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-light transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
            :disabled="!form.name.trim()"
            @click="save"
          >
            {{ selectedId === 'new' ? 'Create Project' : 'Save Changes' }}
          </button>
        </div>

        <!-- Conversation history -->
        <template v-if="selectedId !== 'new'">
          <div class="border-t border-border pt-6 flex flex-col gap-3">
            <div class="flex items-center justify-between">
              <span class="text-xs font-semibold text-fg-subtle uppercase tracking-wide">Conversations</span>
              <span class="text-xs text-fg-faint">{{ projectConversations.length }}</span>
            </div>

            <div v-if="projectConversations.length === 0" class="py-6 text-center text-fg-faint text-sm">
              No conversations yet — click "New Chat" to start one.
            </div>

            <div v-else class="flex flex-col gap-1">
              <div
                v-for="conv in projectConversations"
                :key="conv.id"
                class="group flex items-center gap-3 px-3 py-2.5 rounded-lg border border-border bg-elevated/40 hover:bg-elevated transition-colors"
              >
                <MessageSquare :size="13" class="text-fg-subtle shrink-0" />
                <div class="flex-1 min-w-0">
                  <p class="text-sm text-fg truncate">{{ conv.title }}</p>
                  <p class="text-xs text-fg-subtle mt-0.5">{{ formatDate(conv.createdAt) }} · {{ conv.messages.length }} messages</p>
                </div>
                <button
                  class="shrink-0 flex items-center gap-1 text-xs text-fg-subtle hover:text-accent font-medium opacity-0 group-hover:opacity-100 transition-all cursor-pointer"
                  @click="openChat(conv.id)"
                >
                  Open <ExternalLink :size="10" />
                </button>
              </div>
            </div>
          </div>
        </template>

      </div>
    </div>
  </div>
</template>
