import type { IStaticMethods } from "preline"

declare global {
  interface Window {
    HSStaticMethods: IStaticMethods
  }
}

interface ImportMetaEnv {
  readonly VITE_API_URL: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

export {}
