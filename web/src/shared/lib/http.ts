import type { ISigner } from "applesauce-signers"
import { API_URL } from "@/shared/lib/config"

// A response the API refused, carrying its message and status.
export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = "ApiError"
    this.status = status
  }
}

// A session token the API issued to a signer's pubkey.
export type Session = {
  pubkey: string
  token: string
  expires_at: number
}

export function hasStatus(error: unknown, status: number) {
  return error instanceof ApiError && error.status === status
}

const apiUrl = (path: string) => `${API_URL.replace(/\/$/, "")}${path}`

// Call the API, throwing an ApiError with the API's own message when it refuses.
export async function request(path: string, init: RequestInit = {}, failure = "Request failed"): Promise<Response> {
  const response = await fetch(apiUrl(path), init)
  if (response.ok) return response

  const payload = await response.json().catch(() => ({})) as { error?: string }
  throw new ApiError(payload.error || `${failure} (${response.status})`, response.status)
}

// A response's payload, out of its `{ data }` envelope.
export async function unwrap<T>(response: Response): Promise<T> {
  if (response.status === 204) return undefined as T
  return (await response.json() as { data: T }).data
}

// Sign one NIP-98 request for exactly `POST /session`, which buys a session token.
export async function createSession(signer: ISigner): Promise<Session> {
  const event = await signer.signEvent({
    kind: 27235,
    content: "",
    created_at: Math.floor(Date.now() / 1000),
    tags: [["u", apiUrl("/session")], ["method", "POST"]],
  })
  const response = await request("/session", { method: "POST", headers: { Authorization: `Nostr ${btoa(JSON.stringify(event))}` } }, "Failed to sign in")
  return unwrap<Session>(response)
}
