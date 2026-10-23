import { A, useParams } from "@solidjs/router"
import { createResource, createSignal, For, Show } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import ConfirmDialog from "@/shared/components/ConfirmDialog"
import PageContainer from "@/shared/components/PageContainer"
import ResourceState from "@/shared/components/ResourceState"
import { listRelayRooms, moderateRoom, type Room } from "@/shared/lib/api"
import { loaded } from "@/shared/lib/resource"
import useMinLoading from "@/shared/lib/useMinLoading"
import { useFramedRelay } from "@/tenant/components/RelayFrame"
import RoomDialog from "@/tenant/components/relay/RoomDialog"
import { makeChange } from "@/tenant/lib/changes"
import { DELETE_GROUP, ROOM_FLAGS, roomHasFlag, roomName, roomTag } from "@/tenant/lib/rooms"

// A relay's rooms (NIP-29): creating, editing and deleting them, and a way into each.
export default function RelayRooms() {
  const params = useParams()
  const [relay] = useFramedRelay()
  const [rooms, { refetch }] = createResource(() => params.id, listRelayRooms)
  const loading = useMinLoading(() => rooms.loading && !rooms())
  const [editing, setEditing] = createSignal<Room | "new">()
  const [deleting, setDeleting] = createSignal<Room>()
  const [busy, setBusy] = createSignal(false)

  const enabled = () => loaded(relay)?.groups_enabled === 1
  const calls = () => loaded(relay)?.livekit_enabled === 1

  async function remove() {
    const room = deleting()
    if (!room) return

    setBusy(true)
    const removed = await makeChange(
      () => moderateRoom(params.id!, DELETE_GROUP, [["h", room.id]]),
      "Failed to delete room",
      "Room deleted",
    )
    setBusy(false)
    if (removed) {
      setDeleting(undefined)
      void refetch()
    }
  }

  return (
    <PageContainer>
      <div class="mb-6 flex items-center justify-between gap-4">
        <div>
          <h1 class="text-2xl font-bold text-gray-900">Rooms</h1>
          <p class="mt-1 text-sm text-gray-500">The rooms members talk in, and who may moderate each.</p>
        </div>
        <Show when={enabled()}>
          <button type="button" onClick={() => setEditing("new")} class="shrink-0 rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700">
            New room
          </button>
        </Show>
      </div>

      <Show when={!loaded(relay) || enabled()} fallback={<p class="py-20 text-center text-gray-500">Rooms are turned off for this relay. Turn them on from its overview.</p>}>
        <ResourceState loading={loading()} error={rooms.error} loadingText="Loading rooms..." errorText="Failed to load rooms." />
        <Show when={!loading() && loaded(rooms)}>
          {(list) => (
            <Show when={list().length > 0} fallback={<p class="py-20 text-center text-gray-500">No rooms yet.</p>}>
              <ul class="space-y-3">
                <For each={list()}>
                  {(room) => (
                    <li class="flex items-start justify-between gap-4 rounded-xl border border-gray-200 bg-white p-4">
                      <A href={`/relays/${params.id}/rooms/${room.id}`} class="min-w-0 flex-1">
                        <div class="flex flex-wrap items-center gap-2">
                          <span class="font-medium text-gray-900 hover:underline">{roomName(room)}</span>
                          <Show when={roomHasFlag(room, "livekit")}>
                            <span class="rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-600">Voice</span>
                          </Show>
                          <For each={ROOM_FLAGS.filter(flag => roomHasFlag(room, flag.name))}>
                            {(flag) => <span class="rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-600">{flag.label}</span>}
                          </For>
                        </div>
                        <Show when={roomTag(room, "about")}>
                          <p class="mt-1 line-clamp-2 text-sm text-gray-500">{roomTag(room, "about")}</p>
                        </Show>
                        <p class="mt-1 text-xs text-gray-400">
                          {room.members.length} {room.members.length === 1 ? "member" : "members"}
                        </p>
                      </A>
                      <ActionsMenu>
                        {(close) => (
                          <>
                            <A href={`/relays/${params.id}/rooms/${room.id}`} class="block px-3 py-2 text-sm text-gray-700 hover:bg-gray-50" onClick={close}>
                              Members and invites
                            </A>
                            <ActionsMenuItem onClick={() => { close(); setEditing(room) }}>Edit</ActionsMenuItem>
                            <ActionsMenuItem tone="danger" onClick={() => { close(); setDeleting(room) }}>Delete</ActionsMenuItem>
                          </>
                        )}
                      </ActionsMenu>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          )}
        </Show>
      </Show>

      <RoomDialog
        relayId={params.id!}
        room={editing()}
        calls={calls()}
        onClose={() => setEditing(undefined)}
        onSaved={() => void refetch()}
      />

      <ConfirmDialog
        confirmation={deleting() ? {
          title: "Delete room?",
          description: `${roomName(deleting()!)} and everything posted in it will be deleted.`,
          details: ["Its members stay members of the relay.", "This cannot be undone."],
          confirmLabel: "Yes, delete room",
          busyLabel: "Deleting...",
          tone: "danger",
        } : undefined}
        busy={busy()}
        onConfirm={remove}
        onClose={() => !busy() && setDeleting(undefined)}
      />
    </PageContainer>
  )
}
