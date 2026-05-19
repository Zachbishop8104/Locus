import { createRouter, createWebHashHistory } from 'vue-router'
import ChatView from '@/views/ChatView.vue'
import SettingsView from '@/views/SettingsView.vue'
import ProjectsView from '@/views/ProjectsView.vue'
import TeamsView from '@/views/TeamsView.vue'

export default createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/chat' },
    { path: '/chat', component: ChatView },
    { path: '/teams', component: TeamsView },
    { path: '/projects', component: ProjectsView },
    { path: '/settings', component: SettingsView },
  ],
})
