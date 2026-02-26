import { accounts, API_URL } from "./nostr"

type NostrTag = string[]

type UnsignedEvent = {
  kind: number
  content: string
  created_at: number
  tags: NostrTag[]
}

type SignedEvent = UnsignedEvent & {
  id: string
  pubkey: string
  sig: string
}

type EventSigner = {
  signEvent(event: UnsignedEvent): Promise<SignedEvent>
}

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = "ApiError"
    this.status = status
  }
}

function getActiveSigner(): EventSigner {
  const account = accounts.getActive() as { signer?: EventSigner } | undefined
  if (!account?.signer) throw new Error("Not logged in")
  return account.signer
}

async function createNip98Header(url: string, method: string): Promise<string> {
  const signer = getActiveSigner()
  const event = await signer.signEvent({
    kind: 27235,
    content: "",
    created_at: Math.floor(Date.now() / 1000),
    tags: [
      ["u", url],
      ["method", method.toUpperCase()],
    ],
  })

  const encoded = btoa(JSON.stringify(event))
  return `Nostr ${encoded}`
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const method = (init?.method ?? "GET").toUpperCase()
  const url = new URL(path, API_URL).toString()
  const auth = await createNip98Header(url, method)

  const response = await fetch(url, {
    ...init,
    method,
    headers: {
      ...(init?.headers ?? {}),
      Authorization: auth,
      "Content-Type": "application/json",
    },
  })

  if (!response.ok) {
    let message = `Request failed (${response.status})`
    try {
      const body = await response.json() as { error?: string }
      if (body.error) message = body.error
    } catch {
      // ignored
    }
    throw new ApiError(message, response.status)
  }

  if (response.status === 204) {
    return undefined as T
  }

  return response.json() as Promise<T>
}

export type Relay = {
  id: string
  tenant: string
  name: string
  subdomain: string
  schema: string
  icon: string
  description: string
  plan: string
  status: string
}

export function listTenantRelays() {
  return request<Relay[]>("/tenant/relays")
}

export type CreateRelayInput = {
  name: string
  subdomain: string
  icon: string
  description: string
  plan: string
}

export function createTenantRelay(input: CreateRelayInput) {
  return request<Relay>("/tenant/relays", {
    method: "POST",
    body: JSON.stringify(input),
  })
}

export function getTenantRelay(id: string) {
  return request<Relay>(`/tenant/relays/${id}`)
}
