// The core's view of an extension (src-tauri/src/extensions): its manifest (P1)
// and its state in this library.

export interface ExtensionManifest {
  id: string
  version: string
  name: string
  description: string
  publisher?: string | null
  engines: { linen: string }
  main?: string | null
  activation: string[]
  contributes: {
    commands: { id: string; title: string }[]
    selectionActions: { command: string; when?: string | null }[]
    navigatorTabs: { id: string; title: string; page: string }[]
    themes: {
      id: string
      title: string
      scheme: 'light' | 'dark'
      tokens: Record<string, string>
    }[]
    exporters: { id: string; title: string; command: string }[]
  }
  permissions: string[]
}

export interface InstalledExtension {
  manifest: ExtensionManifest
  enabled: boolean
  granted: string[]
  builtin: boolean
  incompatible: string | null
  suspended: boolean
  crashes: number[]
}

export interface Inspection {
  manifest: ExtensionManifest
  permissions: {
    permission: string
    consent: 'atInstall' | 'highlighted' | 'stronglyWarned' | 'eachUse'
  }[]
  update_from: string | null
  new_permissions: string[]
  incompatible: string | null
}
