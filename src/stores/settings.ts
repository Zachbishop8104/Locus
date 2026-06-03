import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ModelId } from '@/types'

export const useSettingsStore = defineStore('settings', () => {
  const apiKey = ref('')
  const model = ref<ModelId>('claude-sonnet-4-6')
  const loaded = ref(false)
  const useLocalClaude = ref(false)
  const autoApproveEdits = ref(false)
  const useLocalModel = ref(false)
  const localModelUrl = ref('http://localhost:11434/v1')
  const localModelName = ref('')

  async function load() {
    apiKey.value = await invoke<string>('get_api_key')
    useLocalClaude.value = await invoke<boolean>('get_use_local_claude')
    useLocalModel.value = await invoke<boolean>('get_use_local_model')
    localModelUrl.value = await invoke<string>('get_local_model_url')
    localModelName.value = await invoke<string>('get_local_model_name')
    loaded.value = true
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
  }

  async function saveLocalModelUrl(url: string) {
    await invoke('set_local_model_url', { url })
    localModelUrl.value = url
  }

  async function saveLocalModelName(name: string) {
    await invoke('set_local_model_name', { name })
    localModelName.value = name
  }


  return {
    apiKey, model, loaded, load, saveApiKey,
    useLocalClaude, setUseLocalClaude,
    autoApproveEdits,
    useLocalModel, setUseLocalModel,
    localModelUrl, saveLocalModelUrl,
    localModelName, saveLocalModelName,
  }
})
