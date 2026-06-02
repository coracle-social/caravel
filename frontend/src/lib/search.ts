import Fuse from "fuse.js"

export const FUSE_THRESHOLD = 0.35

export function fuzzySearch<T>(list: T[], keys: string[], query: string): T[] {
  if (!query) return list
  return new Fuse(list, {keys, threshold: FUSE_THRESHOLD, ignoreLocation: true})
    .search(query)
    .map(result => result.item)
}
