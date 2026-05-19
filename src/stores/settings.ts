import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ModelId } from '@/types'

export const useSettingsStore = defineStore('settings', () => {
  const apiKey = ref('')
  const model = ref<ModelId>('claude-sonnet-4-6')
  const loaded = ref(false)

  async function load() {
    apiKey.value = await invoke<string>('get_api_key')
    loaded.value = true
  }

  async function saveApiKey(key: string) {
    await invoke('set_api_key', { key })
    apiKey.value = key
  }

  return { apiKey, model, loaded, load, saveApiKey }
})
