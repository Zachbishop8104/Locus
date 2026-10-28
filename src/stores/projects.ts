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
}, {
  persist: {
    // Migrate older saved shapes to the current one:
    //   - `gitRepos: string[]` (short-lived multi-repo version) → `gitRepo: string` (first non-blank entry)
    //   - `localPath: string` → `localPaths: string[]` (one entry per local clone)
    //   - `dbType` / `dbConnectionString` removed (connection strings live in each project's own code)
    afterHydrate: (ctx) => {
      const list = ctx.store.projects as (Project & {
        gitRepos?: unknown
        localPath?: unknown
        dbType?: unknown
        dbConnectionString?: unknown
      })[]
      for (const p of list) {
        delete p.dbType
        delete p.dbConnectionString

        if (typeof p.gitRepo !== 'string') {
          const repos = Array.isArray(p.gitRepos) ? p.gitRepos : []
          const first = repos.find((r): r is string => typeof r === 'string' && r.trim() !== '')
          p.gitRepo = first?.trim() ?? ''
        }
        delete p.gitRepos

        if (!Array.isArray(p.localPaths)) {
          const old = typeof p.localPath === 'string' ? p.localPath.trim() : ''
          p.localPaths = old ? [old] : []
        }
        delete p.localPath
      }
    },
  },
})
