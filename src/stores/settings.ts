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

  async function load() {
    apiKey.value = await invoke<string>('get_api_key')
    useLocalClaude.value = await invoke<boolean>('get_use_local_claude')
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

  return { apiKey, model, loaded, load, saveApiKey, useLocalClaude, setUseLocalClaude, autoApproveEdits }
})
