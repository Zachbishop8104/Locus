import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Project } from '@/types'
import { PROJECT_COLORS } from '@/types'

export const useProjectsStore = defineStore('projects', () => {
  const projects = ref<Project[]>([])

  function createProject(fields: Omit<Project, 'id' | 'createdAt'>): Project {
    const project: Project = {
      ...fields,
      id: crypto.randomUUID(),
      createdAt: new Date().toISOString(),
    }
    projects.value.push(project)
    return project
  }

  function updateProject(id: string, fields: Partial<Omit<Project, 'id' | 'createdAt'>>) {
    const p = projects.value.find(p => p.id === id)
    if (p) Object.assign(p, fields)
  }

  function deleteProject(id: string) {
    projects.value = projects.value.filter(p => p.id !== id)
  }

  function getById(id: string): Project | undefined {
    return projects.value.find(p => p.id === id)
  }

  function nextColor(): string {
    return PROJECT_COLORS[projects.value.length % PROJECT_COLORS.length]
  }

  return { projects, createProject, updateProject, deleteProject, getById, nextColor }
}, { persist: true })
