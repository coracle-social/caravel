import { npubEncode } from "applesauce-core/helpers/pointers"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner } from "applesauce-signers"

// Relays a remote signer and this page talk over.
export const NOSTR_CONNECT_RELAYS = ["wss://relay.nsec.app", "wss://bucket.coracle.social"]

NostrConnectSigner.pool = new RelayPool()

// A short npub for showing which key is signed in.
export function shortNpub(pubkey: string) {
  const npub = npubEncode(pubkey)
  return `${npub.slice(0, 12)}…${npub.slice(-6)}`
}
