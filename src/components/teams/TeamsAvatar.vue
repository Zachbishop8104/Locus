<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import { Users } from '@lucide/vue'
import { useTeamsStore } from '@/stores/teams'

const props = defineProps<{ userId?: string; name?: string; size?: number; group?: boolean }>()
const teams = useTeamsStore()
const photoUrl = ref('')
const sz = props.size ?? 32

async function load() {
  if (props.group || !props.userId) return
  photoUrl.value = await teams.getPhoto(props.userId)
}

onMounted(load)
watch(() => props.userId, load)

function initials(name?: string) {
  if (!name) return '?'
  const parts = name.trim().split(/\s+/)
  return parts.length >= 2
    ? (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
    : name[0].toUpperCase()
}
</script>

<template>
  <!-- Group chat: show a people icon with a distinct violet tint -->
  <div
    v-if="group"
    class="rounded-full shrink-0 overflow-hidden flex items-center justify-center select-none"
    :style="{ width: `${sz}px`, height: `${sz}px` }"
    style="background: linear-gradient(135deg, #1e2f26 0%, #162218 100%)"
  >
    <Users :size="Math.round(sz * 0.45)" class="text-[#52B788]" />
  </div>

  <!-- DM / single user -->
  <div
    v-else
    class="rounded-full shrink-0 overflow-hidden flex items-center justify-center bg-accent/25 text-accent-fg font-semibold select-none"
    :style="{ width: `${sz}px`, height: `${sz}px`, fontSize: `${Math.round(sz * 0.35)}px` }"
  >
    <img v-if="photoUrl" :src="photoUrl" :alt="name" class="w-full h-full object-cover" />
    <span v-else>{{ initials(name) }}</span>
  </div>
</template>
