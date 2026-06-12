import { npubEncode } from "applesauce-core/helpers/pointers"

export function shortenPubkey(pubkey: string) {
  return pubkey.length <= 16 ? pubkey : `${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`
}

// Encode a hex pubkey as an npub for display. Falls back to the raw input when it
// isn't a valid pubkey, since npubEncode throws on malformed input.
export function toNpub(pubkey: string) {
  try {
    return npubEncode(pubkey)
  } catch {
    return pubkey
  }
}

export function shortenNpub(pubkey: string) {
  return shortenPubkey(toNpub(pubkey))
}
