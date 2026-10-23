// Names for the kinds relays carry most; any other shows its number.
const KIND_LABELS: Record<number, string> = {
  0: "Profile",
  1: "Note",
  7: "Reaction",
  9: "Chat message",
  11: "Thread",
  20: "Picture",
  1063: "File",
  1111: "Comment",
  1984: "Report",
  9802: "Highlight",
  30023: "Article",
}

export const kindLabel = (kind: number) => KIND_LABELS[kind] ?? `Kind ${kind}`
