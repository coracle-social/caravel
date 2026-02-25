declare module "nonboard" {
  interface NonboardPayload {
    pubkey: string
    method: string
    events: Array<{ kind: number; content: string; tags: string[][] }>
    nip01?: { secret: string }
    nip46?: { relays: string[]; clientSecret: string; signerPubkey: string }
  }

  interface NonboardOptions {
    appUrl: string
    appName: string
    appImage?: string
    onLogin?: (payload: NonboardPayload) => void
    onSignup?: (payload: NonboardPayload) => void
    onError?: (error: unknown) => void
    onInfo?: (message: unknown) => void
  }

  interface NonboardApp {
    mount(el: HTMLElement): void
    destroy(): void
  }

  export default function nb(options: NonboardOptions): NonboardApp
}
