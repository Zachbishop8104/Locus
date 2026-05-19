<script setup lang="ts">
import Sidebar from './Sidebar.vue'
</script>

<template>
  <div class="flex h-full bg-bg text-fg">
    <Sidebar />
    <main class="flex flex-col flex-1 min-w-0">
      <router-view />
    </main>
  </div>
</template>
