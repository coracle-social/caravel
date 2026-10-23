import type { Room } from "@/shared/lib/api"

// The NIP-29 kinds the dashboard publishes to change a room.
export const PUT_USER = 9000
export const REMOVE_USER = 9001
export const EDIT_METADATA = 9002
export const CREATE_GROUP = 9007
export const DELETE_GROUP = 9008
export const CREATE_INVITE = 9009
export const ASSIGN_PERMISSION = 9011
export const UNASSIGN_PERMISSION = 9012

// What a room member may be permitted, each a moderation kind they may then publish.
export const PERMISSIONS = [
  { kind: 9000, label: "Add members and set their roles" },
  { kind: 9001, label: "Remove members" },
  { kind: 9002, label: "Edit the room" },
  { kind: 9005, label: "Delete messages" },
  { kind: 9008, label: "Delete the room" },
  { kind: 9009, label: "Create invite codes" },
  { kind: 9010, label: "Pin messages" },
  { kind: 9011, label: "Grant permissions they hold" },
  { kind: 9012, label: "Revoke permissions they hold" },
] as const

export const permissionLabel = (kind: number) =>
  PERMISSIONS.find(permission => permission.kind === kind)?.label ?? `Kind ${kind}`

// The flags that limit who can use a room, in the order Flotilla offers them.
export const ROOM_FLAGS = [
  { name: "restricted", label: "Restricted", description: "Only members can send messages." },
  { name: "private", label: "Private", description: "Only members can read messages." },
  { name: "hidden", label: "Hidden", description: "Only members know the room exists." },
  { name: "closed", label: "Closed", description: "Requests to join are ignored without an invite code." },
] as const

export const roomTag = (room: Room, name: string) => room.tags.find(tag => tag[0] === name)?.[1] ?? ""

export const roomHasFlag = (room: Room, name: string) => room.tags.some(tag => tag[0] === name)

export const roomName = (room: Room) => roomTag(room, "name") || room.id

// A new room's id, from the characters NIP-29 allows.
export const newRoomId = () =>
  Array.from(crypto.getRandomValues(new Uint8Array(6)), byte => byte.toString(16).padStart(2, "0")).join("")
