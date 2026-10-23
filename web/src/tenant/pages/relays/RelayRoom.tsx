import { useParams } from "@solidjs/router"
import { createEffect, createMemo, createResource, createSignal, For, on, Show } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import BackLink from "@/shared/components/BackLink"
import ConfirmDialog from "@/shared/components/ConfirmDialog"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import PageContainer from "@/shared/components/PageContainer"
import ResourceState from "@/shared/components/ResourceState"
import { listRelayRooms, moderateRoom, type RoomMember } from "@/shared/lib/api"
import { copyToClipboard } from "@/shared/lib/clipboard"
import { formatError } from "@/shared/lib/format"
import { usePrimeProfiles } from "@/shared/lib/profiles"
import { shortenNpub } from "@/shared/lib/pubkey"
import { loaded } from "@/shared/lib/resource"
import useMinLoading from "@/shared/lib/useMinLoading"
import PubkeyDialog from "@/tenant/components/relay/PubkeyDialog"
import PubkeyLabel from "@/tenant/components/relay/PubkeyLabel"
import Section from "@/tenant/components/relay/Section"
import { makeChange } from "@/tenant/lib/changes"
import {
  ASSIGN_PERMISSION,
  CREATE_INVITE,
  PERMISSIONS,
  permissionLabel,
  PUT_USER,
  REMOVE_USER,
  roomName,
  UNASSIGN_PERMISSION,
} from "@/tenant/lib/rooms"

// Roles as typed: comma-separated labels.
const parseRoles = (text: string) => text.split(",").map(role => role.trim()).filter(Boolean)

// One room's members, their roles and permissions, and its invite codes.
export default function RelayRoom() {
  const params = useParams()
  const [rooms, { refetch }] = createResource(() => params.id, listRelayRooms)
  const loading = useMinLoading(() => rooms.loading && !rooms())
  const room = createMemo(() => loaded(rooms)?.find(room => room.id === params.room))
  const [adding, setAdding] = createSignal(false)
  const [editingRoles, setEditingRoles] = createSignal<RoomMember>()
  const [editingPermissions, setEditingPermissions] = createSignal<RoomMember>()
  const [removing, setRemoving] = createSignal<RoomMember>()
  const [busy, setBusy] = createSignal(false)

  usePrimeProfiles(() => room()?.members.map(member => member.pubkey) ?? [])

  const moderate = (kind: number, tags: string[][]) => moderateRoom(params.id!, kind, [["h", params.room!], ...tags])

  async function remove() {
    const member = removing()
    if (!member) return

    setBusy(true)
    const removed = await makeChange(() => moderate(REMOVE_USER, [["p", member.pubkey]]), "Failed to remove member", "Member removed")
    setBusy(false)
    if (removed) {
      setRemoving(undefined)
      void refetch()
    }
  }

  async function createInvite() {
    const code = Array.from(crypto.getRandomValues(new Uint8Array(6)), byte => byte.toString(16).padStart(2, "0")).join("")
    if (await makeChange(() => moderate(CREATE_INVITE, [["code", code]]), "Failed to create invite code")) {
      void refetch()
      void copyToClipboard(code, { successMessage: "Invite code copied" })
    }
  }

  return (
    <PageContainer>
      <BackLink href={`/relays/${params.id}/rooms`} label="Rooms" />
      <ResourceState loading={loading()} error={rooms.error} loadingText="Loading room..." errorText="Failed to load room." />
      <Show when={!loading() && loaded(rooms)}>
        <Show when={room()} fallback={<p class="py-20 text-center text-gray-500">This room doesn't exist.</p>}>
          {(current) => (
            <div class="space-y-6">
              <h1 class="text-2xl font-bold text-gray-900">{roomName(current())}</h1>

              <Section
                title="Members"
                description="Roles are labels clients show. Permissions decide what each member may change in the room; the relay's owner may change anything."
                action={
                  <button type="button" onClick={() => setAdding(true)} class="shrink-0 rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700">
                    Add member
                  </button>
                }
              >
                <Show when={current().members.length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">No members yet.</p>}>
                  <ul class="divide-y divide-gray-100">
                    <For each={current().members}>
                      {(member) => (
                        <li class="flex items-center justify-between gap-3 py-3">
                          <PubkeyLabel pubkey={member.pubkey} />
                          <div class="flex flex-wrap items-center justify-end gap-2">
                            <For each={member.roles}>
                              {(role) => <span class="rounded-full border border-gray-200 px-2 py-0.5 text-xs text-gray-700">{role}</span>}
                            </For>
                            <Show when={member.permissions.length > 0}>
                              <span class="rounded-full bg-amber-50 px-2 py-0.5 text-xs text-amber-800" title={member.permissions.map(permissionLabel).join(", ")}>
                                {member.permissions.length} {member.permissions.length === 1 ? "permission" : "permissions"}
                              </span>
                            </Show>
                            <ActionsMenu>
                              {(close) => (
                                <>
                                  <ActionsMenuItem onClick={() => { close(); setEditingRoles(member) }}>Edit roles</ActionsMenuItem>
                                  <ActionsMenuItem onClick={() => { close(); setEditingPermissions(member) }}>Edit permissions</ActionsMenuItem>
                                  <ActionsMenuItem tone="danger" onClick={() => { close(); setRemoving(member) }}>Remove</ActionsMenuItem>
                                </>
                              )}
                            </ActionsMenu>
                          </div>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </Section>

              <Section
                title="Invite codes"
                description="A code lets someone join the room when it's closed."
                action={
                  <button type="button" onClick={createInvite} class="shrink-0 rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
                    New code
                  </button>
                }
              >
                <Show when={current().invites.length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">No invite codes.</p>}>
                  <ul class="divide-y divide-gray-100">
                    <For each={current().invites}>
                      {(code) => (
                        <li class="flex items-center justify-between gap-3 py-3">
                          <span class="font-mono text-sm text-gray-900">{code}</span>
                          <button type="button" onClick={() => void copyToClipboard(code, { successMessage: "Invite code copied" })} class="text-sm text-blue-600 hover:underline">
                            Copy
                          </button>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </Section>
            </div>
          )}
        </Show>
      </Show>

      <PubkeyDialog
        open={adding()}
        onClose={() => setAdding(false)}
        title="Add member"
        submitLabel="Add member"
        noteLabel="Roles (optional, comma-separated)"
        onSubmit={async (pubkey, roles) => {
          await moderate(PUT_USER, [["p", pubkey, ...parseRoles(roles)]])
          void refetch()
        }}
      />

      <RolesDialog
        member={editingRoles()}
        onClose={() => setEditingRoles(undefined)}
        onSave={async (member, roles) => {
          await moderate(PUT_USER, [["p", member.pubkey, ...roles]])
          void refetch()
        }}
      />

      <PermissionsDialog
        member={editingPermissions()}
        onClose={() => setEditingPermissions(undefined)}
        onSave={async (member, permissions) => {
          const added = permissions.filter(kind => !member.permissions.includes(kind))
          const removed = member.permissions.filter(kind => !permissions.includes(kind))
          try {
            if (added.length) await moderate(ASSIGN_PERMISSION, [["p", member.pubkey, ...added.map(String)]])
            if (removed.length) await moderate(UNASSIGN_PERMISSION, [["p", member.pubkey, ...removed.map(String)]])
          } finally {
            void refetch()
          }
        }}
      />

      <ConfirmDialog
        confirmation={removing() ? {
          title: "Remove from room?",
          description: `${shortenNpub(removing()!.pubkey)} will no longer be a member of this room, and loses any permissions in it.`,
          confirmLabel: "Yes, remove",
          busyLabel: "Removing...",
          tone: "danger",
        } : undefined}
        busy={busy()}
        onConfirm={remove}
        onClose={() => !busy() && setRemoving(undefined)}
      />
    </PageContainer>
  )
}

// A dialog's buttons and error, below whatever it asks.
function DialogFooter(props: { saving: boolean; error: string; label: string; onCancel: () => void; onSave: () => void }) {
  return (
    <div class="border-t border-gray-100 px-6 py-4">
      <Show when={props.error}>
        <p class="mb-3 text-sm text-red-600">{props.error}</p>
      </Show>
      <div class="flex justify-end gap-2">
        <button type="button" onClick={props.onCancel} disabled={props.saving} class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50">
          Cancel
        </button>
        <button type="button" onClick={props.onSave} disabled={props.saving} class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50">
          {props.saving ? "Saving..." : props.label}
        </button>
      </div>
    </div>
  )
}

// Saving one of the dialogs below: busy while it runs, and its error if it fails.
function useSave<T>(onSave: (value: T) => Promise<void>, onClose: () => void) {
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")

  async function save(value: T) {
    setSaving(true)
    setError("")
    try {
      await onSave(value)
      onClose()
    } catch (e) {
      setError(formatError(e, "Failed to save"))
    } finally {
      setSaving(false)
    }
  }

  return { saving, error, setError, save }
}

// The labels a room member goes by, which grant nothing.
function RolesDialog(props: { member?: RoomMember; onClose: () => void; onSave: (member: RoomMember, roles: string[]) => Promise<void> }) {
  const [text, setText] = createSignal("")
  const { saving, error, setError, save } = useSave((roles: string[]) => props.onSave(props.member!, roles), () => props.onClose())

  createEffect(on(() => props.member, (member) => {
    setText(member?.roles.join(", ") ?? "")
    setError("")
  }))

  return (
    <Modal open={props.member !== undefined} onClose={() => !saving() && props.onClose()}>
      <ModalHeader title="Roles" onClose={props.onClose} closeDisabled={saving()} />
      <div class="px-6 py-4">
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">Roles, comma-separated</span>
          <input type="text" value={text()} onInput={(e) => setText(e.currentTarget.value)} placeholder="host, regular" class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm" autofocus />
        </label>
      </div>
      <DialogFooter saving={saving()} error={error()} label="Save roles" onCancel={props.onClose} onSave={() => void save(parseRoles(text()))} />
    </Modal>
  )
}

// The moderation kinds a room member may publish in the room.
function PermissionsDialog(props: { member?: RoomMember; onClose: () => void; onSave: (member: RoomMember, permissions: number[]) => Promise<void> }) {
  const [chosen, setChosen] = createSignal<number[]>([])
  const { saving, error, setError, save } = useSave((permissions: number[]) => props.onSave(props.member!, permissions), () => props.onClose())

  createEffect(on(() => props.member, (member) => {
    setChosen(member?.permissions ?? [])
    setError("")
  }))

  const toggle = (kind: number) =>
    setChosen(current => current.includes(kind) ? current.filter(held => held !== kind) : [...current, kind])

  return (
    <Modal open={props.member !== undefined} onClose={() => !saving() && props.onClose()}>
      <ModalHeader title="Permissions" onClose={props.onClose} closeDisabled={saving()} />
      <div class="space-y-1 px-6 py-4">
        <For each={PERMISSIONS}>
          {(permission) => (
            <label class="flex items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-gray-50">
              <input type="checkbox" checked={chosen().includes(permission.kind)} onChange={() => toggle(permission.kind)} />
              <span class="text-sm text-gray-900">{permission.label}</span>
              <span class="ml-auto font-mono text-xs text-gray-400">{permission.kind}</span>
            </label>
          )}
        </For>
      </div>
      <DialogFooter saving={saving()} error={error()} label="Save permissions" onCancel={props.onClose} onSave={() => void save(chosen())} />
    </Modal>
  )
}
