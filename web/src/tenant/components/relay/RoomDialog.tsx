import { createEffect, createSignal, For, on, Show } from "solid-js"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import { moderateRoom, type Room } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { CREATE_GROUP, EDIT_METADATA, newRoomId, ROOM_FLAGS, roomHasFlag, roomTag } from "@/tenant/lib/rooms"

const EDITED = new Set(["name", "about", "picture", "livekit", ...ROOM_FLAGS.map(flag => flag.name)])

type RoomDialogProps = {
  relayId: string
  // A room to edit, or "new" to create one.
  room?: Room | "new"
  calls: boolean
  onClose: () => void
  onSaved: () => void
}

// Creating a room or editing its metadata, keeping any tags this form doesn't know.
export default function RoomDialog(props: RoomDialogProps) {
  const [name, setName] = createSignal("")
  const [about, setAbout] = createSignal("")
  const [picture, setPicture] = createSignal("")
  const [flags, setFlags] = createSignal<string[]>([])
  const [voice, setVoice] = createSignal(false)
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")

  const existing = () => (props.room === "new" ? undefined : props.room)

  createEffect(on(() => props.room, () => {
    const room = existing()
    setName(room ? roomTag(room, "name") : "")
    setAbout(room ? roomTag(room, "about") : "")
    setPicture(room ? roomTag(room, "picture") : "")
    setFlags(room ? ROOM_FLAGS.filter(flag => roomHasFlag(room, flag.name)).map(flag => flag.name) : [])
    setVoice(room ? roomHasFlag(room, "livekit") : false)
    setError("")
  }))

  const toggle = (flag: string) =>
    setFlags(current => current.includes(flag) ? current.filter(held => held !== flag) : [...current, flag])

  async function save(e: Event) {
    e.preventDefault()
    if (!name().trim()) {
      setError("Give the room a name.")
      return
    }

    const room = existing()
    const tags = [
      ["h", room?.id ?? newRoomId()],
      ...(room?.tags.filter(tag => !EDITED.has(tag[0])) ?? []),
      ["name", name().trim()],
      ...(about().trim() ? [["about", about().trim()]] : []),
      ...(picture().trim() ? [["picture", picture().trim()]] : []),
      ...(voice() ? [["livekit"]] : []),
      ...flags().map(flag => [flag]),
    ]

    setSaving(true)
    setError("")
    try {
      await moderateRoom(props.relayId, room ? EDIT_METADATA : CREATE_GROUP, tags)
      props.onSaved()
      props.onClose()
    } catch (e) {
      setError(formatError(e, "Failed to save room"))
    } finally {
      setSaving(false)
    }
  }

  const field = "w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"

  return (
    <Modal open={props.room !== undefined} onClose={() => !saving() && props.onClose()}>
      <ModalHeader title={existing() ? "Edit room" : "New room"} onClose={props.onClose} closeDisabled={saving()} />
      <form onSubmit={save} class="space-y-4 px-6 py-4">
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">Name</span>
          <input type="text" value={name()} onInput={(e) => setName(e.currentTarget.value)} placeholder="General" class={field} autofocus />
        </label>
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">About</span>
          <textarea value={about()} onInput={(e) => setAbout(e.currentTarget.value)} rows={2} class={field} />
        </label>
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">Picture URL</span>
          <input type="url" value={picture()} onInput={(e) => setPicture(e.currentTarget.value)} placeholder="https://" class={field} />
        </label>
        <Show when={props.calls}>
          <label class="block">
            <span class="mb-1 block text-sm font-medium text-gray-700">Room type</span>
            <select value={voice() ? "voice" : "text"} onChange={(e) => setVoice(e.currentTarget.value === "voice")} class={field}>
              <option value="text">Text</option>
              <option value="voice">Voice</option>
            </select>
          </label>
        </Show>
        <div class="space-y-1">
          <span class="block text-sm font-medium text-gray-700">Permissions</span>
          <For each={ROOM_FLAGS}>
            {(flag) => (
              <label class="flex items-start gap-3 rounded-lg px-2 py-1.5 hover:bg-gray-50">
                <input type="checkbox" class="mt-1" checked={flags().includes(flag.name)} onChange={() => toggle(flag.name)} />
                <span>
                  <span class="block text-sm font-medium text-gray-900">{flag.label}</span>
                  <span class="block text-xs text-gray-500">{flag.description}</span>
                </span>
              </label>
            )}
          </For>
        </div>
        <Show when={error()}>
          <p class="text-sm text-red-600">{error()}</p>
        </Show>
        <div class="flex justify-end gap-2">
          <button type="button" onClick={props.onClose} disabled={saving()} class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50">
            Cancel
          </button>
          <button type="submit" disabled={saving()} class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50">
            {saving() ? "Saving..." : existing() ? "Save room" : "Create room"}
          </button>
        </div>
      </form>
    </Modal>
  )
}
