<script setup lang="ts">
import Sidebar from './Sidebar.vue'
import DiffPanel from '@/components/chat/DiffPanel.vue'
</script>

<template>
  <div class="flex h-full bg-bg text-fg">
    <Sidebar />
    <main class="flex flex-col flex-1 min-w-0 relative">
      <router-view />
      <DiffPanel />
    </main>
  </div>
</template>
