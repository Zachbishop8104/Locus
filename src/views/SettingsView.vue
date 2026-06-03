<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { Eye, EyeOff, Check, Key, Users, Loader2, LogOut, AlertCircle, ExternalLink, Cpu, Server, RefreshCw } from '@lucide/vue'
import { useSettingsStore } from '@/stores/settings'
import { useTeamsStore } from '@/stores/teams'

const settings = useSettingsStore()
const teams = useTeamsStore()

const inputKey = ref(settings.apiKey)
const showKey = ref(false)
const saved = ref(false)

const localModelUrl = ref(settings.localModelUrl)
const localModelName = ref(settings.localModelName)
const savedLocal = ref(false)
const availableModels = ref<string[]>([])
const fetchingModels = ref(false)
const fetchModelsError = ref('')

async function saveLocalModel() {
  await settings.saveLocalModelUrl(localModelUrl.value.trim())
  await settings.saveLocalModelName(localModelName.value.trim())
  savedLocal.value = true
  setTimeout(() => (savedLocal.value = false), 2000)
}

async function fetchModels() {
  fetchingModels.value = true
  fetchModelsError.value = ''
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const models = await invoke<string[]>('fetch_local_models', { baseUrl: localModelUrl.value.trim() })
    availableModels.value = models
    if (models.length > 0 && !models.includes(localModelName.value)) {
      localModelName.value = models[0]
    }
  } catch (e) {
    fetchModelsError.value = String(e)
  } finally {
    fetchingModels.value = false
  }
}

const teamsClientId = ref('')
const teamsTenantId = ref('')
const teamsConnecting = ref(false)
const teamsError = ref('')
const showAzureGuide = ref(false)

onMounted(async () => {
  await teams.loadStatus()
  const [clientId, tenantId] = await teams.getCredentials()
  teamsClientId.value = clientId
  teamsTenantId.value = tenantId
})

async function saveApiKey() {
  await settings.saveApiKey(inputKey.value.trim())
  saved.value = true
  setTimeout(() => (saved.value = false), 2000)
}

async function connectTeams() {
  if (!teamsClientId.value.trim() || !teamsTenantId.value.trim()) {
    teamsError.value = 'Both Client ID and Tenant ID are required.'
    return
  }
  teamsError.value = ''
  teamsConnecting.value = true
  try {
    await teams.connect(teamsClientId.value.trim(), teamsTenantId.value.trim())
  } catch (e) {
    teamsError.value = String(e)
  } finally {
    teamsConnecting.value = false
  }
}

async function disconnectTeams() {
  await teams.disconnect()
}
</script>

<template>
  <div class="flex flex-col h-full">
    <div class="flex items-center px-6 py-4 border-b border-border/60 shrink-0">
      <h1 class="text-sm font-semibold text-fg">Settings</h1>
    </div>

    <div class="flex-1 overflow-y-auto px-6 py-6">
      <div class="max-w-lg flex flex-col gap-8">

        <!-- ─── Claude Connection ─── -->
        <section class="flex flex-col gap-4">
          <div>
            <h2 class="text-base font-semibold text-fg">Claude Connection</h2>
            <p class="text-sm text-fg-subtle mt-1">
              Choose how Locus connects to Claude.
            </p>
          </div>

          <!-- Local subscription toggle -->
          <div class="flex items-center justify-between p-4 rounded-xl border border-border-strong bg-surface/60">
            <div class="flex items-center gap-3">
              <Cpu :size="16" class="text-accent-icon shrink-0" />
              <div>
                <p class="text-sm font-medium text-fg">Use local Claude subscription</p>
                <p class="text-xs text-fg-subtle mt-0.5">
                  Use your Claude Code subscription instead of an API key.
                </p>
              </div>
            </div>
            <button
              class="relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors cursor-pointer"
              :class="settings.useLocalClaude ? 'bg-accent' : 'bg-border-strong'"
              @click="settings.setUseLocalClaude(!settings.useLocalClaude)"
            >
              <span
                class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
                :class="settings.useLocalClaude ? 'translate-x-6' : 'translate-x-1'"
              />
            </button>
          </div>

          <!-- API Key (shown when not using local subscription) -->
          <template v-if="!settings.useLocalClaude">
            <div class="flex flex-col gap-2">
              <label class="flex items-center gap-1.5 text-xs font-medium text-fg-muted">
                <Key :size="12" /> API Key
              </label>
              <div class="relative">
                <input
                  v-model="inputKey"
                  :type="showKey ? 'text' : 'password'"
                  class="w-full bg-surface border border-border-strong rounded-lg px-3 py-2.5 pr-10 text-sm text-fg placeholder-fg-subtle outline-none focus:border-accent/70 transition-colors"
                  placeholder="sk-ant-…"
                  @keydown.enter="saveApiKey"
                />
                <button
                  class="absolute right-3 top-1/2 -translate-y-1/2 text-fg-subtle hover:text-fg-muted transition-colors cursor-pointer"
                  @click="showKey = !showKey"
                >
                  <component :is="showKey ? EyeOff : Eye" :size="15" />
                </button>
              </div>
            </div>

            <button
              class="flex items-center gap-2 w-fit px-4 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer"
              :class="saved ? 'bg-emerald-700 text-white' : 'bg-accent hover:bg-accent-light text-white'"
              @click="saveApiKey"
            >
              <Check v-if="saved" :size="15" />
              {{ saved ? 'Saved!' : 'Save Key' }}
            </button>
          </template>
        </section>

        <div class="border-t border-border" />

        <!-- ─── Local Model ─── -->
        <section class="flex flex-col gap-4">
          <div>
            <h2 class="text-base font-semibold text-fg">Local Model</h2>
            <p class="text-sm text-fg-subtle mt-1">
              Use a local model via any OpenAI-compatible endpoint (Ollama, LM Studio, etc.).
            </p>
          </div>

          <!-- Enable toggle -->
          <div class="flex items-center justify-between p-4 rounded-xl border border-border-strong bg-surface/60">
            <div class="flex items-center gap-3">
              <Server :size="16" class="text-accent-icon shrink-0" />
              <div>
                <p class="text-sm font-medium text-fg">Use local model</p>
                <p class="text-xs text-fg-subtle mt-0.5">
                  Overrides Claude API and local Claude subscription.
                </p>
              </div>
            </div>
            <button
              class="relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors cursor-pointer"
              :class="settings.useLocalModel ? 'bg-accent' : 'bg-border-strong'"
              @click="settings.setUseLocalModel(!settings.useLocalModel)"
            >
              <span
                class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
                :class="settings.useLocalModel ? 'translate-x-6' : 'translate-x-1'"
              />
            </button>
          </div>

          <!-- Config fields (shown when enabled) -->
          <template v-if="settings.useLocalModel">
            <!-- Base URL + fetch button -->
            <div class="flex flex-col gap-2">
              <label class="text-xs font-medium text-fg-muted">Base URL</label>
              <div class="flex gap-2">
                <input
                  v-model="localModelUrl"
                  type="text"
                  class="flex-1 bg-surface border border-border-strong rounded-lg px-3 py-2.5 text-sm text-fg placeholder-fg-subtle outline-none focus:border-accent/70 transition-colors font-mono"
                  placeholder="http://localhost:11434/v1"
                />
                <button
                  class="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm font-medium border border-border-strong text-fg-muted hover:text-fg hover:border-accent/60 transition-colors cursor-pointer shrink-0"
                  :disabled="fetchingModels"
                  @click="fetchModels"
                >
                  <RefreshCw :size="14" :class="fetchingModels ? 'animate-spin' : ''" />
                  {{ fetchingModels ? 'Loading…' : 'Load models' }}
                </button>
              </div>
              <p class="text-xs text-fg-faint">Ollama: http://localhost:11434/v1 · LM Studio: http://localhost:1234/v1</p>
            </div>

            <!-- Error from fetch -->
            <div
              v-if="fetchModelsError"
              class="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-red-950/40 border border-red-700/50 text-red-400 text-xs"
            >
              <AlertCircle :size="13" class="shrink-0 mt-0.5" />
              <span>{{ fetchModelsError }}</span>
            </div>

            <div class="flex flex-col gap-2">
              <label class="text-xs font-medium text-fg-muted">Model</label>
              <select
                v-if="availableModels.length > 0"
                v-model="localModelName"
                class="w-full bg-surface border border-border-strong rounded-lg px-3 py-2.5 text-sm text-fg outline-none focus:border-accent/70 transition-colors cursor-pointer font-mono"
              >
                <option value="">— none —</option>
                <option v-for="m in availableModels" :key="m" :value="m">{{ m }}</option>
              </select>
              <input
                v-else
                v-model="localModelName"
                type="text"
                class="w-full bg-surface border border-border-strong rounded-lg px-3 py-2.5 text-sm text-fg placeholder-fg-subtle outline-none focus:border-accent/70 transition-colors font-mono"
                placeholder="qwen3:14b"
              />
            </div>


            <button
              class="flex items-center gap-2 w-fit px-4 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer"
              :class="savedLocal ? 'bg-emerald-700 text-white' : 'bg-accent hover:bg-accent-light text-white'"
              @click="saveLocalModel"
            >
              <Check v-if="savedLocal" :size="15" />
              {{ savedLocal ? 'Saved!' : 'Save' }}
            </button>
          </template>
        </section>

        <div class="border-t border-border" />

        <!-- ─── Microsoft Teams ─── -->
        <section class="flex flex-col gap-4">
          <div>
            <h2 class="text-base font-semibold text-fg">Microsoft Teams</h2>
            <p class="text-sm text-fg-subtle mt-1">
              Connect your work account to browse teams, channels, and messages.
            </p>
          </div>

          <!-- Connected state -->
          <div
            v-if="teams.status.connected"
            class="flex items-center justify-between p-4 rounded-xl border border-emerald-700/40 bg-emerald-950/30"
          >
            <div class="flex items-center gap-3">
              <div class="w-9 h-9 rounded-full bg-accent/30 flex items-center justify-center">
                <Users :size="16" class="text-accent-icon" />
              </div>
              <div>
                <p class="text-sm font-medium text-fg">{{ teams.status.userName ?? 'Connected' }}</p>
                <p class="text-xs text-emerald-500 mt-0.5">Connected to Microsoft Teams</p>
              </div>
            </div>
            <button
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium text-fg-muted hover:text-red-400 hover:bg-red-950/30 border border-border-strong hover:border-red-700/50 transition-colors cursor-pointer"
              @click="disconnectTeams"
            >
              <LogOut :size="12" />
              Disconnect
            </button>
          </div>

          <!-- Not connected: form -->
          <div v-else class="flex flex-col gap-4">

            <!-- Azure setup guide toggle -->
            <button
              class="flex items-center gap-1.5 text-xs text-accent-icon hover:text-accent-fg transition-colors cursor-pointer w-fit"
              @click="showAzureGuide = !showAzureGuide"
            >
              <ExternalLink :size="12" />
              {{ showAzureGuide ? 'Hide' : 'How to get your Client ID & Tenant ID' }}
            </button>

            <!-- Azure guide -->
            <div
              v-if="showAzureGuide"
              class="p-4 rounded-xl border border-border-strong bg-surface/60 text-xs text-fg-muted space-y-2"
            >
              <p class="font-semibold text-fg-muted text-sm mb-3">Azure App Registration (5 min)</p>
              <ol class="list-decimal list-inside space-y-2 leading-relaxed">
                <li>Go to <span class="text-accent-icon font-mono">portal.azure.com</span> and sign in with your work account</li>
                <li>Search for <span class="text-fg">App registrations</span> → click <span class="text-fg">New registration</span></li>
                <li>Name it anything (e.g. "Locus"), leave defaults → click <span class="text-fg">Register</span></li>
                <li>Copy the <span class="text-fg">Application (client) ID</span> and <span class="text-fg">Directory (tenant) ID</span> from the overview page</li>
                <li>Click <span class="text-fg">Authentication</span> → Add platform → <span class="text-fg">Mobile and desktop applications</span> → enable <span class="font-mono text-fg-muted">http://localhost</span> → Save</li>
                <li>Click <span class="text-fg">API permissions</span> → Add a permission → Microsoft Graph → Delegated → add:
                  <ul class="list-disc list-inside ml-4 mt-1 space-y-0.5 text-fg-subtle">
                    <li><span class="font-mono text-fg-muted">Team.ReadBasic.All</span></li>
                    <li><span class="font-mono text-fg-muted">Channel.ReadBasic.All</span></li>
                    <li><span class="font-mono text-fg-muted">Chat.Read</span> <span class="text-emerald-600">(no admin consent needed)</span></li>
                    <li><span class="font-mono text-fg-muted">Chat.ReadBasic</span></li>
                    <li><span class="font-mono text-fg-muted">ChannelMessage.Read.All</span> <span class="text-amber-600">(optional — requires admin consent)</span></li>
                    <li><span class="font-mono text-fg-muted">offline_access</span></li>
                  </ul>
                </li>
              </ol>
              <p class="mt-3 text-fg-subtle">If you see a yellow warning on ChannelMessage.Read.All, ask your IT admin to grant consent for the app.</p>
            </div>

            <div class="flex flex-col gap-3">
              <div class="flex flex-col gap-1.5">
                <label class="text-xs font-medium text-fg-muted">Application (Client) ID</label>
                <input
                  v-model="teamsClientId"
                  type="text"
                  class="w-full bg-surface border border-border-strong rounded-lg px-3 py-2.5 text-sm text-fg placeholder-fg-subtle outline-none focus:border-accent/70 transition-colors font-mono"
                  placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                />
              </div>

              <div class="flex flex-col gap-1.5">
                <label class="text-xs font-medium text-fg-muted">Directory (Tenant) ID</label>
                <input
                  v-model="teamsTenantId"
                  type="text"
                  class="w-full bg-surface border border-border-strong rounded-lg px-3 py-2.5 text-sm text-fg placeholder-fg-subtle outline-none focus:border-accent/70 transition-colors font-mono"
                  placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                />
              </div>
            </div>

            <!-- Error -->
            <div
              v-if="teamsError"
              class="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-red-950/40 border border-red-700/50 text-red-400 text-xs"
            >
              <AlertCircle :size="13" class="shrink-0 mt-0.5" />
              <span>{{ teamsError }}</span>
            </div>

            <button
              class="flex items-center gap-2 w-fit px-4 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer"
              :class="
                teamsConnecting
                  ? 'bg-elevated text-fg-subtle cursor-not-allowed'
                  : 'bg-accent hover:bg-accent-light text-white'
              "
              :disabled="teamsConnecting"
              @click="connectTeams"
            >
              <Loader2 v-if="teamsConnecting" :size="15" class="animate-spin" />
              <Users v-else :size="15" />
              {{ teamsConnecting ? 'Opening browser…' : 'Connect Teams' }}
            </button>

            <p v-if="teamsConnecting" class="text-xs text-fg-subtle">
              A browser window will open for Microsoft sign-in. Complete it there, then return here.
            </p>
          </div>
        </section>

        <div class="border-t border-border" />

        <!-- About -->
        <section class="flex flex-col gap-3">
          <h2 class="text-base font-semibold text-fg">About</h2>
          <div class="flex flex-col gap-1.5 text-sm text-fg-subtle">
            <p>Locus v0.1.0</p>
            <p class="text-xs text-fg-faint mt-0.5">Built with Tauri · Vue · Anthropic Claude</p>
          </div>
        </section>

      </div>
    </div>
  </div>
</template>
