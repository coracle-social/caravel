import { npubEncode } from "applesauce-core/helpers/pointers"

function shortenPubkey(pubkey: string) {
  return pubkey.length <= 16 ? pubkey : `${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`
}

// A hex pubkey as an npub, or as given when it isn't a valid pubkey.
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
