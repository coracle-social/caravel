import { npubEncode } from "applesauce-core/helpers/pointers"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner, type ISigner } from "applesauce-signers"
import { API_URL } from "@/shared/lib/config"
import { createSession } from "@/portal/lib/api"

// Relays a remote signer and this page talk over.
export const NOSTR_CONNECT_RELAYS = ["wss://relay.nsec.app", "wss://bucket.coracle.social"]

NostrConnectSigner.pool = new RelayPool()

// Sign in by having the member sign one NIP-98 event for the session request,
// and return the session it buys.
export async function signInWith(signer: ISigner) {
  const url = `${API_URL}/session`
  const event = await signer.signEvent({
    kind: 27235,
    content: "",
    created_at: Math.floor(Date.now() / 1000),
    tags: [["u", url], ["method", "POST"]],
  })

  return createSession(`Nostr ${btoa(JSON.stringify(event))}`)
}

// A short npub for showing which key is signed in.
export function shortNpub(pubkey: string) {
  const npub = npubEncode(pubkey)
  return `${npub.slice(0, 12)}…${npub.slice(-6)}`
}
