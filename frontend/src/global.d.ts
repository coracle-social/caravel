import type { IStaticMethods } from "preline"

declare global {
  interface NostrNip07 {
    getPublicKey(): Promise<string>
    signEvent(event: unknown): Promise<unknown>
    nip04?: {
      encrypt(pubkey: string, plaintext: string): Promise<string>
      decrypt(pubkey: string, ciphertext: string): Promise<string>
    }
    nip44?: {
      encrypt(pubkey: string, plaintext: string): Promise<string>
      decrypt(pubkey: string, ciphertext: string): Promise<string>
    }
  }

  interface Window {
    HSStaticMethods: IStaticMethods
    nostr?: NostrNip07
  }
}

interface ImportMetaEnv {
  readonly VITE_API_URL: string
  readonly VITE_PLATFORM_NAME?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

export {}
