<script setup lang="ts">
import { onMounted, computed, ref, watch } from 'vue'
import {
  Users, Hash, AlertCircle, Loader2,
  ChevronRight, RefreshCw, MessageCircle, Send, Paperclip,
} from '@lucide/vue'
import { useTeamsStore } from '@/stores/teams'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import TeamsAvatar from '@/components/teams/TeamsAvatar.vue'
import ConnectorCard from '@/components/teams/ConnectorCard.vue'
import type { TeamsMessage, ChatInfo, TeamsAttachment } from '@/stores/teams'

const router = useRouter()
const teams = useTeamsStore()
const activeTab = ref<'channels' | 'chats'>('chats')
const chatInput = ref('')
const channelInput = ref('')
const sending = ref(false)
const sendError = ref('')

async function submitChatMessage() {
  const text = chatInput.value.trim()
  if (!text || !teams.selectedChatId || sending.value) return
  sending.value = true
  sendError.value = ''
  try {
    await teams.sendChatMessage(teams.selectedChatId, text)
    chatInput.value = ''
  } catch (e) {
    sendError.value = String(e)
  } finally {
    sending.value = false
  }
}

async function submitChannelMessage() {
  const text = channelInput.value.trim()
  if (!text || !teams.selectedTeamId || !teams.selectedChannelId || sending.value) return
  sending.value = true
  sendError.value = ''
  try {
    await teams.sendChannelMessage(teams.selectedTeamId, teams.selectedChannelId, text)
    channelInput.value = ''
  } catch (e) {
    sendError.value = String(e)
  } finally {
    sending.value = false
  }
}

function onChatKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submitChatMessage() }
}
function onChannelKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submitChannelMessage() }
}

onMounted(async () => {
  await teams.loadStatus()
  if (teams.status.connected) {
    teams.loadChats()
    teams.loadTeams()
  }
})

const selectedTeam = computed(() => teams.teams.find((t) => t.id === teams.selectedTeamId))
const selectedChannel = computed(() => teams.channels.find((c) => c.id === teams.selectedChannelId))
const selectedChat = computed(() => teams.chats.find((c) => c.id === teams.selectedChatId))

function chatLabel(chat: ChatInfo | undefined) {
  if (!chat) return ''
  if (chat.topic) return chat.topic
  return chat.members.filter((m: string) => m !== teams.status.userName).join(', ') || 'Chat'
}

function chatAvatarUserId(chat: ChatInfo): string | undefined {
  if (!teams.status.userId) return undefined
  return chat.memberIds?.find(id => id !== teams.status.userId)
}

function chatAvatarName(chat: ChatInfo): string {
  const otherMembers = chat.members.filter(m => m !== teams.status.userName)
  return otherMembers[0] ?? chatLabel(chat)
}

function isMine(msg: TeamsMessage) {
  if (teams.status.userId && msg.fromUserId)
    return msg.fromUserId === teams.status.userId
  if (teams.status.userName && msg.fromName)
    return msg.fromName.trim().toLowerCase() === teams.status.userName.trim().toLowerCase()
  return false
}

const CARD_TYPES = new Set([
  'application/vnd.microsoft.card.o365connector',
  'application/vnd.microsoft.card.adaptive',
])

function cardAttachments(msg: TeamsMessage): TeamsAttachment[] {
  return msg.attachments.filter(a => CARD_TYPES.has(a.contentType) && a.content)
}

// Strip Teams-specific HTML tags that browsers won't render meaningfully
function sanitizeTeamsHtml(html: string): string {
  return html
    .replace(/<at[^>]*>([\s\S]*?)<\/at>/gi, '<span class="text-accent-fg">$1</span>')
    .replace(/<attachment[^>]*\/?>([\s\S]*?<\/attachment>)?/gi, '')
    .trim()
}

function formatDate(iso: string) {
  if (!iso) return ''
  const d = new Date(iso)
  const now = new Date()
  if (d.toDateString() === now.toDateString())
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  return d.toLocaleDateString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

// ── Inline image resolution ──────────────────────────────────────────────────
// Cache: url → data URL ('' means failed)
const imgCache = new Map<string, string>()
// Cache: msgId → processed HTML with data URLs substituted
const processedBodies = ref<Record<string, string>>({})

const HOSTED_IMG_RE = /src="(https:\/\/graph\.microsoft\.com\/[^"]*\/hostedContents\/[^"]*\$value)"/g

async function processBody(msg: TeamsMessage) {
  if (msg.contentType !== 'html' || msg.id in processedBodies.value) return
  const html = msg.body
  if (!html.includes('hostedContents')) {
    processedBodies.value[msg.id] = html
    return
  }

  const urls = new Set<string>()
  let m: RegExpExecArray | null
  HOSTED_IMG_RE.lastIndex = 0
  while ((m = HOSTED_IMG_RE.exec(html)) !== null) urls.add(m[1])

  await Promise.all([...urls].map(url => {
    if (imgCache.has(url)) return Promise.resolve()
    return invoke<string>('fetch_teams_image', { url })
      .then(d => { imgCache.set(url, d) })
      .catch(() => { imgCache.set(url, '') })
  }))

  let result = html
  for (const url of urls) {
    const d = imgCache.get(url)
    if (d) result = result.split(`src="${url}"`).join(`src="${d}"`)
  }
  processedBodies.value[msg.id] = result
}

watch(() => teams.chatMessages, msgs => msgs.forEach(processBody), { immediate: true })
watch(() => teams.messages,     msgs => msgs.forEach(processBody), { immediate: true })

function bodyHtml(msg: TeamsMessage) {
  return sanitizeTeamsHtml(processedBodies.value[msg.id] ?? msg.body)
}

// File/image attachments (excludes cards)
function fileAttachments(msg: TeamsMessage): TeamsAttachment[] {
  return msg.attachments.filter(a => !CARD_TYPES.has(a.contentType) && a.name)
}

function openAttachment(att: TeamsAttachment) {
  if (att.contentUrl) invoke('open_url', { url: att.contentUrl })
}
</script>

<template>
  <div class="flex h-full">

    <!-- Not connected -->
    <div v-if="!teams.status.connected" class="flex flex-col items-center justify-center w-full gap-4 px-6">
      <div class="w-14 h-14 rounded-2xl bg-accent/20 flex items-center justify-center">
        <Users :size="28" class="text-accent-icon" />
      </div>
      <div class="text-center">
        <p class="text-fg font-medium">Microsoft Teams not connected</p>
        <p class="text-fg-subtle text-sm mt-1 max-w-xs">Connect Teams to browse your chats and channels</p>
      </div>
      <button
        class="px-4 py-2 rounded-lg bg-accent hover:bg-accent-light text-white text-sm font-medium transition-colors cursor-pointer"
        @click="router.push('/settings')"
      >
        Connect in Settings
      </button>
    </div>

    <template v-else>
      <!-- Left panel: tab switcher + list -->
      <div class="flex flex-col w-64 shrink-0 border-r border-border overflow-hidden">

        <!-- Tabs -->
        <div class="flex border-b border-border shrink-0">
          <button
            class="flex-1 flex items-center justify-center gap-1.5 py-2.5 text-xs font-medium transition-colors cursor-pointer"
            :class="activeTab === 'chats'
              ? 'text-fg border-b-2 border-accent-light'
              : 'text-fg-subtle hover:text-fg-muted'"
            @click="activeTab = 'chats'"
          >
            <MessageCircle :size="13" /> Chats
          </button>
          <button
            class="flex-1 flex items-center justify-center gap-1.5 py-2.5 text-xs font-medium transition-colors cursor-pointer"
            :class="activeTab === 'channels'
              ? 'text-fg border-b-2 border-accent-light'
              : 'text-fg-subtle hover:text-fg-muted'"
            @click="activeTab = 'channels'"
          >
            <Hash :size="13" /> Channels
          </button>
        </div>

        <!-- ── CHATS LIST ── -->
        <div v-if="activeTab === 'chats'" class="flex-1 overflow-y-auto">
          <div class="flex items-center justify-between px-3 py-2 border-b border-border/60">
            <span class="text-xs text-fg-faint">Recent</span>
            <button
              class="p-1 rounded text-fg-faint hover:text-fg-muted hover:bg-elevated transition-colors cursor-pointer"
              @click="teams.loadChats"
            >
              <RefreshCw :size="11" :class="teams.loadingChats ? 'animate-spin' : ''" />
            </button>
          </div>

          <div v-if="teams.loadingChats" class="flex items-center justify-center py-8">
            <Loader2 :size="18" class="text-fg-subtle animate-spin" />
          </div>
          <div v-else-if="teams.chats.length === 0" class="px-4 py-3 text-xs text-fg-faint">
            No chats found
          </div>

          <button
            v-for="chat in teams.chats"
            :key="chat.id"
            class="flex items-center gap-2.5 w-full px-3 py-2.5 text-left transition-colors cursor-pointer"
            :class="teams.selectedChatId === chat.id
              ? 'bg-elevated/80 text-fg'
              : 'text-fg-muted hover:bg-elevated/50 hover:text-fg'"
            @click="teams.selectChat(chat.id)"
          >
            <TeamsAvatar
              :group="chat.chatType !== 'oneOnOne'"
              :userId="chatAvatarUserId(chat)"
              :name="chatAvatarName(chat)"
              :size="30"
            />
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium truncate leading-tight">{{ chatLabel(chat) }}</p>
              <p class="text-xs text-fg-subtle truncate mt-0.5">
                {{ chat.chatType === 'oneOnOne' ? 'Direct message' : `${chat.members.length} members` }}
              </p>
            </div>
          </button>
        </div>

        <!-- ── CHANNELS LIST ── -->
        <div v-else class="flex-1 overflow-y-auto">
          <div class="flex items-center justify-between px-3 py-2 border-b border-border/60">
            <span class="text-xs text-fg-faint">Teams</span>
            <button
              class="p-1 rounded text-fg-faint hover:text-fg-muted hover:bg-elevated transition-colors cursor-pointer"
              @click="teams.loadTeams"
            >
              <RefreshCw :size="11" :class="teams.loadingTeams ? 'animate-spin' : ''" />
            </button>
          </div>

          <div v-if="teams.loadingTeams" class="flex items-center justify-center py-8">
            <Loader2 :size="18" class="text-fg-subtle animate-spin" />
          </div>
          <div v-else-if="teams.teams.length === 0" class="px-4 py-3 text-xs text-fg-faint">
            No teams found
          </div>

          <div v-for="team in teams.teams" :key="team.id">
            <button
              class="flex items-center gap-2.5 w-full px-3 py-2.5 text-left transition-colors cursor-pointer"
              :class="teams.selectedTeamId === team.id
                ? 'bg-elevated/80 text-fg'
                : 'text-fg-muted hover:bg-elevated/50 hover:text-fg'"
              @click="teams.selectTeam(team.id)"
            >
              <div class="w-7 h-7 rounded-lg bg-accent/25 flex items-center justify-center shrink-0 text-accent-fg text-xs font-bold">
                {{ team.displayName.charAt(0).toUpperCase() }}
              </div>
              <span class="flex-1 truncate text-sm font-medium">{{ team.displayName }}</span>
              <ChevronRight
                :size="12"
                class="shrink-0 transition-transform text-fg-faint"
                :class="teams.selectedTeamId === team.id ? 'rotate-90 text-fg-muted' : ''"
              />
            </button>

            <div v-if="teams.selectedTeamId === team.id">
              <div v-if="teams.loadingChannels" class="flex items-center justify-center py-3">
                <Loader2 :size="14" class="text-fg-subtle animate-spin" />
              </div>
              <button
                v-for="ch in teams.channels"
                :key="ch.id"
                class="flex items-center gap-2 w-full pl-8 pr-3 py-1.5 text-left transition-colors cursor-pointer"
                :class="teams.selectedChannelId === ch.id
                  ? 'bg-border-strong/50 text-fg'
                  : 'text-fg-subtle hover:bg-elevated/40 hover:text-fg-muted'"
                @click="teams.selectChannel(ch.id)"
              >
                <Hash :size="13" class="shrink-0 text-fg-faint" />
                <span class="truncate text-sm">{{ ch.displayName }}</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Footer -->
        <div class="shrink-0 px-3 py-2 border-t border-border flex items-center gap-2">
          <div class="w-6 h-6 rounded-full bg-accent/30 flex items-center justify-center">
            <Users :size="11" class="text-accent-icon" />
          </div>
          <span class="text-xs text-fg-subtle truncate">{{ teams.status.userName ?? 'Connected' }}</span>
        </div>
      </div>

      <!-- Right panel: messages -->
      <div class="flex flex-col flex-1 min-w-0">

        <!-- Nothing selected -->
        <div
          v-if="activeTab === 'chats' && !teams.selectedChatId"
          class="flex flex-col items-center justify-center h-full gap-3 text-center px-6"
        >
          <MessageCircle :size="32" class="text-fg-faint" />
          <p class="text-fg-subtle text-sm">Select a chat to read messages</p>
        </div>

        <div
          v-else-if="activeTab === 'channels' && !teams.selectedChannelId"
          class="flex flex-col items-center justify-center h-full gap-3 text-center px-6"
        >
          <Hash :size="32" class="text-fg-faint" />
          <div>
            <p class="text-fg-subtle text-sm">
              {{ selectedTeam ? `Select a channel in ${selectedTeam.displayName}` : 'Select a team and channel' }}
            </p>
            <p v-if="selectedTeam" class="text-xs text-fg-faint mt-2 max-w-xs">
              Channel messages require admin consent for ChannelMessage.Read.All in your org.
            </p>
          </div>
        </div>

        <!-- Chat messages -->
        <template v-else-if="activeTab === 'chats' && teams.selectedChatId">
          <div class="flex items-center justify-between px-5 py-3.5 border-b border-border shrink-0">
            <div class="flex items-center gap-2">
              <MessageCircle :size="14" class="text-fg-subtle" />
              <span class="font-medium text-fg text-sm">{{ chatLabel(selectedChat) }}</span>
            </div>
            <button
              class="p-1.5 rounded text-fg-subtle hover:text-fg-muted hover:bg-elevated transition-colors cursor-pointer"
              @click="teams.selectChat(teams.selectedChatId!)"
            >
              <RefreshCw :size="13" :class="teams.loadingChatMessages ? 'animate-spin' : ''" />
            </button>
          </div>

          <div v-if="teams.error" class="flex items-start gap-2.5 mx-5 mt-4 px-4 py-3 rounded-lg bg-red-950/40 border border-red-700/50 text-red-400 text-sm">
            <AlertCircle :size="15" class="shrink-0 mt-0.5" />
            <span>{{ teams.error }}</span>
          </div>

          <div v-if="teams.loadingChatMessages" class="flex items-center justify-center flex-1">
            <Loader2 :size="22" class="text-fg-subtle animate-spin" />
          </div>

          <div v-else class="flex-1 overflow-y-auto flex flex-col-reverse px-5 py-4 gap-0.5">
            <div v-if="teams.chatMessages.length === 0" class="flex items-center justify-center py-12 text-fg-faint text-sm">
              No messages
            </div>
            <div
              v-for="msg in teams.chatMessages"
              :key="msg.id"
              class="flex gap-2.5 py-1.5 px-1"
              :class="isMine(msg) ? 'flex-row-reverse' : 'flex-row'"
            >
              <TeamsAvatar v-if="!isMine(msg)" :userId="msg.fromUserId" :name="msg.fromName" :size="32" class="mt-0.5" />
              <div class="flex flex-col max-w-[75%]" :class="isMine(msg) ? 'items-end' : 'items-start'">
                <div v-if="!isMine(msg)" class="flex items-baseline gap-2 mb-0.5 px-1">
                  <span class="text-xs font-semibold text-fg-muted">{{ msg.fromName ?? 'Unknown' }}</span>
                  <span class="text-xs text-fg-subtle">{{ formatDate(msg.createdAt) }}</span>
                </div>
                <!-- Connector / Adaptive cards (preferred over body when present) -->
                <template v-if="cardAttachments(msg).length">
                  <ConnectorCard
                    v-for="att in cardAttachments(msg)"
                    :key="att.id"
                    :attachment="att"
                  />
                </template>
                <!-- Normal message bubble -->
                <div
                  v-else
                  class="rounded-2xl px-3.5 py-2 text-sm leading-relaxed"
                  :class="isMine(msg)
                    ? 'bg-accent text-white rounded-tr-sm'
                    : 'bg-elevated text-fg rounded-tl-sm'"
                >
                  <div v-if="msg.contentType === 'html'" class="teams-message" v-html="bodyHtml(msg)" />
                  <p v-else class="whitespace-pre-wrap m-0">{{ msg.body }}</p>
                </div>
                <!-- File attachments -->
                <div v-if="fileAttachments(msg).length" class="flex flex-wrap gap-1.5 mt-1">
                  <button
                    v-for="att in fileAttachments(msg)" :key="att.id"
                    class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg border border-border bg-elevated hover:bg-border-strong text-fg-muted hover:text-fg text-xs transition-colors cursor-pointer"
                    @click="openAttachment(att)"
                  >
                    <Paperclip :size="11" class="shrink-0 text-fg-subtle" />
                    <span class="truncate max-w-45">{{ att.name }}</span>
                  </button>
                </div>
                <span v-if="isMine(msg)" class="text-xs text-fg-subtle mt-0.5 px-1">{{ formatDate(msg.createdAt) }}</span>
              </div>
            </div>
          </div>

          <!-- Chat input -->
          <div class="shrink-0 px-5 pb-4 pt-2 border-t border-border/60">
            <div v-if="sendError" class="text-xs text-red-400 mb-2">{{ sendError }}</div>
            <div class="flex items-end gap-2 rounded-xl border border-border-strong bg-surface focus-within:border-accent-light/60 transition-colors px-3 py-2">
              <textarea
                v-model="chatInput"
                rows="1"
                class="flex-1 bg-transparent text-sm text-fg placeholder-fg-subtle resize-none outline-none max-h-32"
                placeholder="Message…"
                :disabled="sending"
                @keydown="onChatKeydown"
                @input="(e) => { const t = e.target as HTMLTextAreaElement; t.style.height = 'auto'; t.style.height = Math.min(t.scrollHeight, 128) + 'px' }"
              />
              <button
                class="shrink-0 p-1.5 rounded-lg transition-colors cursor-pointer mb-0.5"
                :class="chatInput.trim() && !sending ? 'text-accent-icon hover:bg-accent/20' : 'text-fg-faint cursor-not-allowed'"
                :disabled="!chatInput.trim() || sending"
                @click="submitChatMessage"
              >
                <Send :size="15" />
              </button>
            </div>
            <p class="text-xs text-fg-faint mt-1.5 px-1">⏎ send · ⇧⏎ newline</p>
          </div>
        </template>

        <!-- Channel messages -->
        <template v-else-if="activeTab === 'channels' && teams.selectedChannelId">
          <div class="flex items-center justify-between px-5 py-3.5 border-b border-border shrink-0">
            <div class="flex items-center gap-2">
              <Hash :size="14" class="text-fg-subtle" />
              <span class="font-medium text-fg text-sm">{{ selectedChannel?.displayName }}</span>
              <span class="text-xs text-fg-subtle">· {{ selectedTeam?.displayName }}</span>
            </div>
            <button
              class="p-1.5 rounded text-fg-subtle hover:text-fg-muted hover:bg-elevated transition-colors cursor-pointer"
              @click="teams.selectChannel(teams.selectedChannelId!)"
            >
              <RefreshCw :size="13" :class="teams.loadingMessages ? 'animate-spin' : ''" />
            </button>
          </div>

          <div v-if="teams.error" class="flex items-start gap-2.5 mx-5 mt-4 px-4 py-3 rounded-lg bg-red-950/40 border border-red-700/50 text-red-400 text-sm">
            <AlertCircle :size="15" class="shrink-0 mt-0.5" />
            <p>{{ teams.error }}</p>
          </div>

          <div v-if="teams.loadingMessages" class="flex items-center justify-center flex-1">
            <Loader2 :size="22" class="text-fg-subtle animate-spin" />
          </div>
          <div v-else class="flex-1 overflow-y-auto flex flex-col-reverse px-5 py-4 gap-0.5">
            <div v-if="teams.messages.length === 0 && !teams.error" class="flex items-center justify-center py-12 text-fg-faint text-sm">
              No messages
            </div>
            <div
              v-for="msg in teams.messages"
              :key="msg.id"
              class="flex gap-2.5 py-1.5 px-1"
              :class="isMine(msg) ? 'flex-row-reverse' : 'flex-row'"
            >
              <TeamsAvatar v-if="!isMine(msg)" :userId="msg.fromUserId" :name="msg.fromName" :size="32" class="mt-0.5" />
              <div class="flex flex-col max-w-[75%]" :class="isMine(msg) ? 'items-end' : 'items-start'">
                <div v-if="!isMine(msg)" class="flex items-baseline gap-2 mb-0.5 px-1">
                  <span class="text-xs font-semibold text-fg-muted">{{ msg.fromName ?? 'Unknown' }}</span>
                  <span class="text-xs text-fg-subtle">{{ formatDate(msg.createdAt) }}</span>
                </div>
                <!-- Connector / Adaptive cards (preferred over body when present) -->
                <template v-if="cardAttachments(msg).length">
                  <ConnectorCard
                    v-for="att in cardAttachments(msg)"
                    :key="att.id"
                    :attachment="att"
                  />
                </template>
                <!-- Normal message bubble -->
                <div
                  v-else
                  class="rounded-2xl px-3.5 py-2 text-sm leading-relaxed"
                  :class="isMine(msg)
                    ? 'bg-accent text-white rounded-tr-sm'
                    : 'bg-elevated text-fg rounded-tl-sm'"
                >
                  <div v-if="msg.contentType === 'html'" class="teams-message" v-html="bodyHtml(msg)" />
                  <p v-else class="whitespace-pre-wrap m-0">{{ msg.body }}</p>
                </div>
                <!-- File attachments -->
                <div v-if="fileAttachments(msg).length" class="flex flex-wrap gap-1.5 mt-1">
                  <button
                    v-for="att in fileAttachments(msg)" :key="att.id"
                    class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg border border-border bg-elevated hover:bg-border-strong text-fg-muted hover:text-fg text-xs transition-colors cursor-pointer"
                    @click="openAttachment(att)"
                  >
                    <Paperclip :size="11" class="shrink-0 text-fg-subtle" />
                    <span class="truncate max-w-45">{{ att.name }}</span>
                  </button>
                </div>
                <span v-if="isMine(msg)" class="text-xs text-fg-subtle mt-0.5 px-1">{{ formatDate(msg.createdAt) }}</span>
              </div>
            </div>
          </div>

          <!-- Channel input -->
          <div class="shrink-0 px-5 pb-4 pt-2 border-t border-border/60">
            <div v-if="sendError" class="text-xs text-red-400 mb-2">{{ sendError }}</div>
            <div class="flex items-end gap-2 rounded-xl border border-border-strong bg-surface focus-within:border-accent-light/60 transition-colors px-3 py-2">
              <textarea
                v-model="channelInput"
                rows="1"
                class="flex-1 bg-transparent text-sm text-fg placeholder-fg-subtle resize-none outline-none max-h-32"
                :placeholder="`Message #${selectedChannel?.displayName ?? ''}…`"
                :disabled="sending"
                @keydown="onChannelKeydown"
                @input="(e) => { const t = e.target as HTMLTextAreaElement; t.style.height = 'auto'; t.style.height = Math.min(t.scrollHeight, 128) + 'px' }"
              />
              <button
                class="shrink-0 p-1.5 rounded-lg transition-colors cursor-pointer mb-0.5"
                :class="channelInput.trim() && !sending ? 'text-accent-icon hover:bg-accent/20' : 'text-fg-faint cursor-not-allowed'"
                :disabled="!channelInput.trim() || sending"
                @click="submitChannelMessage"
              >
                <Send :size="15" />
              </button>
            </div>
            <p class="text-xs text-fg-faint mt-1.5 px-1">⏎ send · ⇧⏎ newline</p>
          </div>
        </template>

      </div>
    </template>
  </div>
</template>

<style scoped>
.teams-message :deep(p) { margin: 0 0 4px; }
.teams-message :deep(p:last-child) { margin-bottom: 0; }
.teams-message :deep(a) { color: var(--color-accent-icon); text-decoration: underline; }
.teams-message :deep(strong) { color: var(--color-fg); }
.teams-message :deep(code) { background: var(--color-elevated); padding: 1px 5px; border-radius: 4px; font-size: 0.875em; color: #1b5e3a; }
.teams-message :deep(pre) { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: 6px; padding: 10px 14px; overflow-x: auto; }
.teams-message :deep(ul), .teams-message :deep(ol) { padding-left: 1.25rem; margin: 4px 0; }
.teams-message :deep(blockquote) { border-left: 3px solid var(--color-border); padding-left: 10px; color: var(--color-fg-muted); }
.teams-message :deep(img) { max-width: 100%; max-height: 400px; border-radius: 6px; object-fit: contain; }
</style>
