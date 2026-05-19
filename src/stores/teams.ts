import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface TeamInfo {
  id: string
  displayName: string
  description?: string
}

export interface ChannelInfo {
  id: string
  displayName: string
  description?: string
}

export interface TeamsAttachment {
  id: string
  contentType: string
  content?: string
  contentUrl?: string
  name?: string
}

export interface TeamsMessage {
  id: string
  fromName?: string
  fromUserId?: string
  body: string
  contentType: string
  createdAt: string
  attachments: TeamsAttachment[]
}

export interface TeamsStatus {
  connected: boolean
  userName?: string
  userId?: string
}

export interface ChatInfo {
  id: string
  topic?: string
  chatType: string
  members: string[]
  memberIds: string[]
  lastMessagePreview?: string
}

function sortNewestFirst(msgs: TeamsMessage[]): TeamsMessage[] {
  return [...msgs].sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime())
}

export const useTeamsStore = defineStore('teams', () => {
  const status = ref<TeamsStatus>({ connected: false })
  const teams = ref<TeamInfo[]>([])
  const channels = ref<ChannelInfo[]>([])
  const messages = ref<TeamsMessage[]>([])
  const selectedTeamId = ref<string | null>(null)
  const selectedChannelId = ref<string | null>(null)
  const loadingTeams = ref(false)
  const loadingChannels = ref(false)
  const loadingMessages = ref(false)
  const error = ref<string | null>(null)

  // Profile photo cache: userId -> data URL (or '' if no photo)
  const photoCache = ref<Record<string, string>>({})

  async function getPhoto(userId: string): Promise<string> {
    if (userId in photoCache.value) return photoCache.value[userId]
    try {
      const dataUrl = await invoke<string>('get_user_photo', { userId })
      photoCache.value[userId] = dataUrl
      return dataUrl
    } catch {
      photoCache.value[userId] = ''
      return ''
    }
  }

  // Chats (DMs + group chats)
  const chats = ref<ChatInfo[]>([])
  const chatMessages = ref<TeamsMessage[]>([])
  const selectedChatId = ref<string | null>(null)
  const loadingChats = ref(false)
  const loadingChatMessages = ref(false)

  async function loadStatus() {
    status.value = await invoke<TeamsStatus>('get_teams_status')
  }

  async function getCredentials() {
    return invoke<[string, string]>('get_teams_credentials')
  }

  async function connect(clientId: string, tenantId: string) {
    error.value = null
    const unlisten = await listen<string>('teams:connected', (e) => {
      status.value = { connected: true, userName: e.payload }
      unlisten()
    })
    await invoke('start_teams_auth', { clientId, tenantId })
  }

  async function disconnect() {
    await invoke('disconnect_teams')
    status.value = { connected: false }
    teams.value = []
    channels.value = []
    messages.value = []
    selectedTeamId.value = null
    selectedChannelId.value = null
    chats.value = []
    chatMessages.value = []
    selectedChatId.value = null
  }

  async function loadTeams() {
    loadingTeams.value = true
    error.value = null
    try {
      teams.value = await invoke<TeamInfo[]>('get_teams_list')
    } catch (e) {
      error.value = String(e)
    } finally {
      loadingTeams.value = false
    }
  }

  async function selectTeam(teamId: string) {
    selectedTeamId.value = teamId
    selectedChannelId.value = null
    messages.value = []
    channels.value = []
    loadingChannels.value = true
    error.value = null
    try {
      channels.value = await invoke<ChannelInfo[]>('get_team_channels', { teamId })
    } catch (e) {
      error.value = String(e)
    } finally {
      loadingChannels.value = false
    }
  }

  async function selectChannel(channelId: string) {
    if (!selectedTeamId.value) return
    selectedChannelId.value = channelId
    messages.value = []
    loadingMessages.value = true
    error.value = null
    try {
      const raw = await invoke<TeamsMessage[]>('get_channel_messages', {
        teamId: selectedTeamId.value,
        channelId,
      })
      messages.value = sortNewestFirst(raw)
    } catch (e) {
      error.value = String(e)
    } finally {
      loadingMessages.value = false
    }
  }

  async function loadChats() {
    loadingChats.value = true
    error.value = null
    try {
      chats.value = await invoke<ChatInfo[]>('get_chats')
    } catch (e) {
      error.value = String(e)
    } finally {
      loadingChats.value = false
    }
  }

  async function sendChatMessage(chatId: string, content: string) {
    await invoke('send_chat_message', { chatId, content })
    // Optimistic: add to local state immediately
    const now = new Date().toISOString()
    chatMessages.value.unshift({
      id: crypto.randomUUID(),
      fromName: status.value.userName,
      fromUserId: status.value.userId,
      body: content,
      contentType: 'text',
      createdAt: now,
      attachments: [],
    })
  }

  async function sendChannelMessage(teamId: string, channelId: string, content: string) {
    await invoke('send_channel_message', { teamId, channelId, content })
    const now = new Date().toISOString()
    messages.value.unshift({
      id: crypto.randomUUID(),
      fromName: status.value.userName,
      fromUserId: status.value.userId,
      body: content,
      contentType: 'text',
      createdAt: now,
      attachments: [],
    })
  }

  async function selectChat(chatId: string) {
    selectedChatId.value = chatId
    chatMessages.value = []
    loadingChatMessages.value = true
    error.value = null
    try {
      chatMessages.value = sortNewestFirst(await invoke<TeamsMessage[]>('get_chat_messages', { chatId }))
    } catch (e) {
      error.value = String(e)
    } finally {
      loadingChatMessages.value = false
    }
  }

  return {
    status,
    teams,
    channels,
    messages,
    selectedTeamId,
    selectedChannelId,
    loadingTeams,
    loadingChannels,
    loadingMessages,
    error,
    chats,
    chatMessages,
    selectedChatId,
    loadingChats,
    loadingChatMessages,
    loadStatus,
    getCredentials,
    connect,
    disconnect,
    loadTeams,
    selectTeam,
    selectChannel,
    photoCache,
    getPhoto,
    loadChats,
    selectChat,
    sendChatMessage,
    sendChannelMessage,
  }
})
