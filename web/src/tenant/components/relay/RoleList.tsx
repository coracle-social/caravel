import { createEffect, createSignal, For, on, Show } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import ConfirmDialog from "@/shared/components/ConfirmDialog"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import { callRelayMethod, type RelayRole } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { slugify } from "@/shared/lib/slugify"
import RoleChip from "@/tenant/components/relay/RoleChip"
import Section from "@/tenant/components/relay/Section"
import { makeChange } from "@/tenant/lib/changes"

type RoleListProps = {
  relayId: string
  roles: RelayRole[]
  onChanged: () => void
}

// The roles a relay gives its members (NIP-43), which clients show and which grant nothing.
export default function RoleList(props: RoleListProps) {
  const [editing, setEditing] = createSignal<RelayRole | "new">()
  const [deleting, setDeleting] = createSignal<RelayRole>()
  const [busy, setBusy] = createSignal(false)

  async function remove() {
    const role = deleting()
    if (!role) return

    setBusy(true)
    const removed = await makeChange(
      () => callRelayMethod(props.relayId, "deleterole", [role.id]),
      "Failed to delete role",
      "Role deleted",
    )
    setBusy(false)
    if (removed) {
      setDeleting(undefined)
      props.onChanged()
    }
  }

  return (
    <Section
      title="Roles"
      description="Labels for members, which clients show next to their names."
      action={
        <button type="button" onClick={() => setEditing("new")} class="shrink-0 rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
          New role
        </button>
      }
    >
      <Show when={props.roles.length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">No roles yet.</p>}>
        <ul class="divide-y divide-gray-100">
          <For each={props.roles}>
            {(role) => (
              <li class="flex items-center justify-between gap-3 py-3">
                <div class="min-w-0">
                  <div class="flex items-center gap-2">
                    <RoleChip id={role.id} roles={props.roles} />
                    <span class="font-mono text-xs text-gray-400">{role.id}</span>
                  </div>
                  <Show when={role.description}>
                    <p class="mt-1 text-sm text-gray-500">{role.description}</p>
                  </Show>
                </div>
                <ActionsMenu>
                  {(close) => (
                    <>
                      <ActionsMenuItem onClick={() => { close(); setEditing(role) }}>Edit</ActionsMenuItem>
                      <ActionsMenuItem tone="danger" onClick={() => { close(); setDeleting(role) }}>Delete</ActionsMenuItem>
                    </>
                  )}
                </ActionsMenu>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <RoleDialog
        relayId={props.relayId}
        role={editing()}
        onClose={() => setEditing(undefined)}
        onSaved={props.onChanged}
      />

      <ConfirmDialog
        confirmation={deleting() ? {
          title: "Delete role?",
          description: `"${deleting()!.label || deleting()!.id}" will be deleted and taken from every member who holds it.`,
          confirmLabel: "Yes, delete",
          busyLabel: "Deleting...",
          tone: "danger",
        } : undefined}
        busy={busy()}
        onConfirm={remove}
        onClose={() => !busy() && setDeleting(undefined)}
      />
    </Section>
  )
}

// Creating a role, or editing one; its id can't change once made.
function RoleDialog(props: { relayId: string; role?: RelayRole | "new"; onClose: () => void; onSaved: () => void }) {
  const [id, setId] = createSignal("")
  const [label, setLabel] = createSignal("")
  const [description, setDescription] = createSignal("")
  const [hue, setHue] = createSignal(210)
  const [position, setPosition] = createSignal(0)
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")

  const isNew = () => props.role === "new"

  createEffect(on(() => props.role, (role) => {
    const existing = role === "new" ? undefined : role
    setId(existing?.id ?? "")
    setLabel(existing?.label ?? "")
    setDescription(existing?.description ?? "")
    setHue(existing?.hue || 210)
    setPosition(existing?.position ?? 0)
    setError("")
  }))

  async function save(e: Event) {
    e.preventDefault()
    const roleId = isNew() ? slugify(id() || label()) : id()
    if (!roleId) {
      setError("Give the role a label.")
      return
    }

    setSaving(true)
    setError("")
    try {
      await callRelayMethod(props.relayId, isNew() ? "createrole" : "editrole", [roleId, label().trim(), description().trim(), hue(), position()])
      props.onSaved()
      props.onClose()
    } catch (e) {
      setError(formatError(e, "Failed to save role"))
    } finally {
      setSaving(false)
    }
  }

  const field = "w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"

  return (
    <Modal open={props.role !== undefined} onClose={() => !saving() && props.onClose()}>
      <ModalHeader title={isNew() ? "New role" : "Edit role"} onClose={props.onClose} closeDisabled={saving()} />
      <form onSubmit={save} class="space-y-4 px-6 py-4">
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">Label</span>
          <input type="text" value={label()} onInput={(e) => setLabel(e.currentTarget.value)} placeholder="Moderator" class={field} autofocus />
        </label>
        <Show when={isNew()}>
          <label class="block">
            <span class="mb-1 block text-sm font-medium text-gray-700">Id</span>
            <input type="text" value={id()} onInput={(e) => setId(e.currentTarget.value)} placeholder={slugify(label()) || "moderator"} class={`${field} font-mono`} />
          </label>
        </Show>
        <label class="block">
          <span class="mb-1 block text-sm font-medium text-gray-700">Description</span>
          <input type="text" value={description()} onInput={(e) => setDescription(e.currentTarget.value)} class={field} />
        </label>
        <div class="flex gap-4">
          <label class="block flex-1">
            <span class="mb-1 block text-sm font-medium text-gray-700">Color</span>
            <div class="flex items-center gap-3">
              <input type="range" min="0" max="360" value={hue()} onInput={(e) => setHue(Number(e.currentTarget.value))} class="flex-1" />
              <span class="h-5 w-5 rounded-full" style={{ background: `hsl(${hue()} 65% 50%)` }} />
            </div>
          </label>
          <label class="block w-24">
            <span class="mb-1 block text-sm font-medium text-gray-700">Order</span>
            <input type="number" value={position()} onInput={(e) => setPosition(Math.trunc(Number(e.currentTarget.value) || 0))} class={field} />
          </label>
        </div>
        <Show when={error()}>
          <p class="text-sm text-red-600">{error()}</p>
        </Show>
        <div class="flex justify-end gap-2">
          <button type="button" onClick={props.onClose} disabled={saving()} class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50">
            Cancel
          </button>
          <button type="submit" disabled={saving()} class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50">
            {saving() ? "Saving..." : "Save role"}
          </button>
        </div>
      </form>
    </Modal>
  )
}
