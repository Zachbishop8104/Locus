<script setup lang="ts">
import { useRoute } from 'vue-router'
import Sidebar from './Sidebar.vue'
import PullRequestsPanel from './PullRequestsPanel.vue'
import DiffPanel from '@/components/chat/DiffPanel.vue'

const route = useRoute()
</script>

<template>
  <div class="flex h-full bg-bg text-fg">
    <Sidebar />
    <main class="flex flex-col flex-1 min-w-0 relative">
      <router-view />
      <DiffPanel />
    </main>
    <!-- The board has its own PR column. -->
    <PullRequestsPanel v-if="route.path !== '/board'" />
  </div>
</template>
