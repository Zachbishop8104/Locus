<script setup lang="ts">
import { onMounted } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import AppShell from '@/components/layout/AppShell.vue'

const settings = useSettingsStore()
onMounted(() => settings.load())
</script>

<template>
  <AppShell />
</template>
