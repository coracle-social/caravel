import { createEffect, createSignal, For, on, Show } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import ConfirmDialog from "@/shared/components/ConfirmDialog"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import { callRelayMethod, type MemberSource, type RelayMember, type RelayRole } from "@/shared/lib/api"
import { usePrimeProfiles } from "@/shared/lib/profiles"
import { shortenNpub } from "@/shared/lib/pubkey"
import PubkeyDialog from "@/tenant/components/relay/PubkeyDialog"
import PubkeyLabel from "@/tenant/components/relay/PubkeyLabel"
import RoleChip from "@/tenant/components/relay/RoleChip"
import Section from "@/tenant/components/relay/Section"
import { makeChange } from "@/tenant/lib/changes"

const SOURCE_LABELS: Record<MemberSource, string> = {
  owner: "Owner",
  manual: "Added",
  claim: "Invite code",
  public_join: "Joined",
  payment: "Paid",
  migrated: "Migrated",
}

type MemberListProps = {
  relayId: string
  members: RelayMember[]
  roles: RelayRole[]
  onChanged: () => void
}

// A relay's members (NIP-43): adding and removing them, and the roles they hold.
export default function MemberList(props: MemberListProps) {
  const [adding, setAdding] = createSignal(false)
  const [editing, setEditing] = createSignal<RelayMember>()
  const [removing, setRemoving] = createSignal<RelayMember>()
  const [busy, setBusy] = createSignal(false)

  usePrimeProfiles(() => props.members.map(member => member.pubkey))

  async function remove() {
    const member = removing()
    if (!member) return

    setBusy(true)
    const removed = await makeChange(
      () => callRelayMethod(props.relayId, "unallowpubkey", [member.pubkey]),
      "Failed to remove member",
      "Member removed",
    )
    setBusy(false)
    if (removed) {
      setRemoving(undefined)
      props.onChanged()
    }
  }

  return (
    <Section
      title="Members"
      description="Who belongs to the relay, and the roles they hold."
      action={
        <button type="button" onClick={() => setAdding(true)} class="shrink-0 rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700">
          Add member
        </button>
      }
    >
      <Show when={props.members.length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">No members yet.</p>}>
        <ul class="divide-y divide-gray-100">
          <For each={props.members}>
            {(member) => (
              <li class="flex items-center justify-between gap-3 py-3">
                <PubkeyLabel pubkey={member.pubkey} />
                <div class="flex flex-wrap items-center justify-end gap-2">
                  <For each={member.roles}>{(id) => <RoleChip id={id} roles={props.roles} />}</For>
                  <span class="text-xs text-gray-500">{SOURCE_LABELS[member.source]}</span>
                  <Show when={props.roles.length > 0 || member.source !== "owner"}>
                    <ActionsMenu>
                      {(close) => (
                        <>
                          <Show when={props.roles.length > 0}>
                            <ActionsMenuItem onClick={() => { close(); setEditing(member) }}>Edit roles</ActionsMenuItem>
                          </Show>
                          <Show when={member.source !== "owner"}>
                            <ActionsMenuItem tone="danger" onClick={() => { close(); setRemoving(member) }}>Remove</ActionsMenuItem>
                          </Show>
                        </>
                      )}
                    </ActionsMenu>
                  </Show>
                </div>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <PubkeyDialog
        open={adding()}
        onClose={() => setAdding(false)}
        title="Add member"
        submitLabel="Add member"
        onSubmit={async (pubkey) => {
          await callRelayMethod(props.relayId, "allowpubkey", [pubkey])
          props.onChanged()
        }}
      />

      <MemberRolesDialog
        relayId={props.relayId}
        member={editing()}
        roles={props.roles}
        onClose={() => setEditing(undefined)}
        onChanged={props.onChanged}
      />

      <ConfirmDialog
        confirmation={removing() ? {
          title: "Remove member?",
          description: `${shortenNpub(removing()!.pubkey)} will no longer be a member of the relay.`,
          details: ["What they've already published stays.", "They can be added again later."],
          confirmLabel: "Yes, remove",
          busyLabel: "Removing...",
          tone: "danger",
        } : undefined}
        busy={busy()}
        onConfirm={remove}
        onClose={() => !busy() && setRemoving(undefined)}
      />
    </Section>
  )
}

// Choosing which of the relay's roles a member holds.
function MemberRolesDialog(props: {
  relayId: string
  member?: RelayMember
  roles: RelayRole[]
  onClose: () => void
  onChanged: () => void
}) {
  const [held, setHeld] = createSignal<string[]>([])
  const [saving, setSaving] = createSignal(false)

  createEffect(on(() => props.member, (member) => setHeld(member?.roles ?? [])))

  const toggle = (id: string) =>
    setHeld(current => current.includes(id) ? current.filter(held => held !== id) : [...current, id])

  async function save() {
    const member = props.member
    if (!member) return

    const added = held().filter(id => !member.roles.includes(id))
    const removed = member.roles.filter(id => !held().includes(id))

    setSaving(true)
    const saved = await makeChange(async () => {
      for (const id of added) await callRelayMethod(props.relayId, "assignrole", [member.pubkey, id])
      for (const id of removed) await callRelayMethod(props.relayId, "unassignrole", [member.pubkey, id])
    }, "Failed to update roles", "Roles updated")
    setSaving(false)
    props.onChanged()
    if (saved) props.onClose()
  }

  return (
    <Modal open={props.member !== undefined} onClose={() => !saving() && props.onClose()}>
      <ModalHeader title="Roles" onClose={props.onClose} closeDisabled={saving()} />
      <div class="space-y-2 px-6 py-4">
        <For each={props.roles}>
          {(role) => (
            <label class="flex items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-gray-50">
              <input type="checkbox" checked={held().includes(role.id)} onChange={() => toggle(role.id)} />
              <RoleChip id={role.id} roles={props.roles} />
              <span class="text-sm text-gray-500">{role.description}</span>
            </label>
          )}
        </For>
      </div>
      <div class="flex justify-end gap-2 border-t border-gray-100 px-6 py-4">
        <button type="button" onClick={props.onClose} disabled={saving()} class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50">
          Cancel
        </button>
        <button type="button" onClick={save} disabled={saving()} class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50">
          {saving() ? "Saving..." : "Save roles"}
        </button>
      </div>
    </Modal>
  )
}
