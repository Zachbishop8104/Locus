import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { MODELS, DEFAULT_MODEL } from '@/types'
import type { ModelId, Effort, ProviderKind, BackendProvider, PermissionMode } from '@/types'

export const useSettingsStore = defineStore('settings', () => {
  // ── Connections (stored in the backend config file) ──
  const apiKey = ref('')
  const loaded = ref(false)
  /** Claude is reached through the local Claude Code CLI instead of an API key. */
  const useLocalClaude = ref(false)
  /** Local OpenAI-compatible models are enabled. */
  const useLocalModel = ref(false)
  const localModelUrl = ref('http://localhost:11434/v1')
  const localModelName = ref('')
  const localModels = ref<string[]>([])
  const localModelsError = ref('')
  const fetchingLocalModels = ref(false)

  // ── Picker selection (persisted in local storage) ──
  const provider = ref<ProviderKind>('claude')
  const model = ref<ModelId>(DEFAULT_MODEL)
  const effort = ref<Effort>('medium')
  const mode = ref<PermissionMode>('ask')

  const claudeReady = computed(() => useLocalClaude.value || !!apiKey.value)
  const localReady = computed(() => useLocalModel.value && !!localModelName.value)

  /** Falls back to whichever provider is configured if the selected one isn't. */
  const activeProvider = computed<ProviderKind | null>(() => {
    if (provider.value === 'local' && localReady.value) return 'local'
    if (provider.value === 'claude' && claudeReady.value) return 'claude'
    if (claudeReady.value) return 'claude'
    if (localReady.value) return 'local'
    return null
  })

  const canChat = computed(() => activeProvider.value !== null)

  const backendProvider = computed<BackendProvider>(() => {
    if (activeProvider.value === 'local') return 'local'
    return useLocalClaude.value ? 'claude_cli' : 'claude_api'
  })

  const claudeModel = computed(() => MODELS.find((m) => m.id === model.value) ?? MODELS.find((m) => m.id === DEFAULT_MODEL)!)

  const activeModelId = computed(() =>
    activeProvider.value === 'local' ? localModelName.value : claudeModel.value.id,
  )

  const activeModelLabel = computed(() =>
    activeProvider.value === 'local' ? localModelName.value || 'Local model' : claudeModel.value.label,
  )

  /** Effort is sent only for Claude models that support it. */
  const activeEffort = computed<Effort | null>(() =>
    activeProvider.value === 'claude' && claudeModel.value.supportsEffort ? effort.value : null,
  )

  async function load() {
    apiKey.value = await invoke<string>('get_api_key')
    useLocalClaude.value = await invoke<boolean>('get_use_local_claude')
    useLocalModel.value = await invoke<boolean>('get_use_local_model')
    localModelUrl.value = await invoke<string>('get_local_model_url')
    localModelName.value = await invoke<string>('get_local_model_name')
    // Drop a persisted model id that no longer exists (e.g. after a model list update).
    if (!MODELS.some((m) => m.id === model.value)) model.value = DEFAULT_MODEL
    loaded.value = true
    if (useLocalModel.value) refreshLocalModels()
  }

  async function saveApiKey(key: string) {
    await invoke('set_api_key', { key })
    apiKey.value = key
  }

  async function setUseLocalClaude(enabled: boolean) {
    await invoke('set_use_local_claude', { enabled })
    useLocalClaude.value = enabled
  }

  async function setUseLocalModel(enabled: boolean) {
    await invoke('set_use_local_model', { enabled })
    useLocalModel.value = enabled
    if (enabled) refreshLocalModels()
  }

  async function saveLocalModelUrl(url: string) {
    await invoke('set_local_model_url', { url })
    localModelUrl.value = url
  }

  async function saveLocalModelName(name: string) {
    await invoke('set_local_model_name', { name })
    localModelName.value = name
  }

  async function refreshLocalModels() {
    fetchingLocalModels.value = true
    localModelsError.value = ''
    try {
      localModels.value = await invoke<string[]>('fetch_local_models', { baseUrl: localModelUrl.value })
    } catch (e) {
      localModels.value = []
      localModelsError.value = String(e)
    } finally {
      fetchingLocalModels.value = false
    }
  }

  async function selectClaudeModel(id: ModelId) {
    provider.value = 'claude'
    model.value = id
  }

  async function selectLocalModel(name: string) {
    provider.value = 'local'
    if (name !== localModelName.value) await saveLocalModelName(name)
  }

  return {
    apiKey, loaded, load, saveApiKey,
    useLocalClaude, setUseLocalClaude,
    useLocalModel, setUseLocalModel,
    localModelUrl, saveLocalModelUrl,
    localModelName, saveLocalModelName,
    localModels, localModelsError, fetchingLocalModels, refreshLocalModels,
    provider, model, effort, mode,
    claudeReady, localReady, activeProvider, canChat, backendProvider,
    claudeModel, activeModelId, activeModelLabel, activeEffort,
    selectClaudeModel, selectLocalModel,
  }
}, {
  persist: { pick: ['provider', 'model', 'effort', 'mode'] },
})
