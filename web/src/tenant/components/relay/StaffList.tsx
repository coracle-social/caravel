import { createEffect, createResource, createSignal, For, on, Show } from "solid-js"
import { normalizeToPubkey } from "applesauce-core/helpers/pointers"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import { callRelayMethod, type MethodAssignee, type Relay } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { loaded } from "@/shared/lib/resource"
import { shortenNpub } from "@/shared/lib/pubkey"
import PubkeyLabel from "@/tenant/components/relay/PubkeyLabel"
import Section from "@/tenant/components/relay/Section"
import { makeChange } from "@/tenant/lib/changes"

// Who may call which of the relay's management methods (NIP-86), besides its owner and itself.
export default function StaffList(props: { relay: Relay }) {
  const [assignees, { refetch }] = createResource(() => props.relay.id, (id) => callRelayMethod<MethodAssignee[]>(id, "listmethodassignees"))
  const [methods] = createResource(() => props.relay.id, (id) => callRelayMethod<string[]>(id, "supportedmethods"))
  const [editing, setEditing] = createSignal<MethodAssignee | "new">()

  // The owner and the relay hold every method, and can't be changed.
  const isManager = (pubkey: string) => pubkey === props.relay.tenant_pubkey || pubkey === props.relay.pubkey

  async function remove(assignee: MethodAssignee) {
    await makeChange(async () => {
      for (const method of assignee.methods) {
        await callRelayMethod(props.relay.id, "unassignmethod", [assignee.pubkey, method])
      }
    }, "Failed to remove staff", "Staff removed")
    void refetch()
  }

  return (
    <Section
      title="Staff"
      description="Pubkeys that may use some of the relay's management methods from clients such as Flotilla."
      action={
        <button type="button" onClick={() => setEditing("new")} class="shrink-0 rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
          Add staff
        </button>
      }
    >
      <Show when={!assignees.error} fallback={<p class="text-sm text-red-600">Failed to load staff.</p>}>
        <ul class="divide-y divide-gray-100">
          <For each={loaded(assignees) ?? []}>
            {(assignee) => (
              <li class="flex items-center justify-between gap-3 py-3">
                <PubkeyLabel pubkey={assignee.pubkey} />
                <Show
                  when={!isManager(assignee.pubkey)}
                  fallback={<span class="text-xs text-gray-500">{assignee.pubkey === props.relay.pubkey ? "This relay" : "Owner"}, every method</span>}
                >
                  <div class="flex items-center gap-2">
                    <span class="text-xs text-gray-500">{assignee.methods.length} {assignee.methods.length === 1 ? "method" : "methods"}</span>
                    <ActionsMenu>
                      {(close) => (
                        <>
                          <ActionsMenuItem onClick={() => { close(); setEditing(assignee) }}>Edit methods</ActionsMenuItem>
                          <ActionsMenuItem tone="danger" onClick={() => { close(); void remove(assignee) }}>Remove</ActionsMenuItem>
                        </>
                      )}
                    </ActionsMenu>
                  </div>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <StaffDialog
        relayId={props.relay.id}
        assignee={editing()}
        methods={loaded(methods) ?? []}
        onClose={() => setEditing(undefined)}
        onSaved={() => void refetch()}
      />
    </Section>
  )
}

// Choosing which methods a pubkey may call, and for a new one, whose they are.
function StaffDialog(props: {
  relayId: string
  assignee?: MethodAssignee | "new"
  methods: string[]
  onClose: () => void
  onSaved: () => void
}) {
  const [input, setInput] = createSignal("")
  const [chosen, setChosen] = createSignal<string[]>([])
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")

  const existing = () => (props.assignee === "new" ? undefined : props.assignee)

  createEffect(on(() => props.assignee, () => {
    setInput("")
    setChosen(existing()?.methods ?? [])
    setError("")
  }))

  const toggle = (method: string) =>
    setChosen(current => current.includes(method) ? current.filter(held => held !== method) : [...current, method])

  async function save() {
    const pubkey = existing()?.pubkey ?? normalizeToPubkey(input().trim())
    if (!pubkey) {
      setError("Enter an npub or a hex pubkey.")
      return
    }

    const held = existing()?.methods ?? []
    setSaving(true)
    setError("")
    try {
      for (const method of chosen().filter(method => !held.includes(method))) {
        await callRelayMethod(props.relayId, "assignmethod", [pubkey, method])
      }
      for (const method of held.filter(method => !chosen().includes(method))) {
        await callRelayMethod(props.relayId, "unassignmethod", [pubkey, method])
      }
      props.onSaved()
      props.onClose()
    } catch (e) {
      setError(formatError(e, "Failed to save methods"))
      props.onSaved()
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal open={props.assignee !== undefined} onClose={() => !saving() && props.onClose()}>
      <ModalHeader
        title={existing() ? `Methods for ${shortenNpub(existing()!.pubkey)}` : "Add staff"}
        onClose={props.onClose}
        closeDisabled={saving()}
      />
      <div class="space-y-4 px-6 py-4">
        <Show when={!existing()}>
          <label class="block">
            <span class="mb-1 block text-sm font-medium text-gray-700">Pubkey</span>
            <input
              type="text"
              value={input()}
              onInput={(e) => setInput(e.currentTarget.value)}
              placeholder="npub1..."
              class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm"
              autofocus
            />
          </label>
        </Show>
        <div class="grid grid-cols-2 gap-1">
          <For each={props.methods}>
            {(method) => (
              <label class="flex items-center gap-2 rounded px-2 py-1 font-mono text-xs text-gray-700 hover:bg-gray-50">
                <input type="checkbox" checked={chosen().includes(method)} onChange={() => toggle(method)} />
                {method}
              </label>
            )}
          </For>
        </div>
        <Show when={error()}>
          <p class="text-sm text-red-600">{error()}</p>
        </Show>
      </div>
      <div class="flex justify-end gap-2 border-t border-gray-100 px-6 py-4">
        <button type="button" onClick={props.onClose} disabled={saving()} class="rounded-lg border border-gray-300 px-4 py-2 text-sm text-gray-700 hover:bg-gray-50">
          Cancel
        </button>
        <button type="button" onClick={save} disabled={saving()} class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50">
          {saving() ? "Saving..." : "Save methods"}
        </button>
      </div>
    </Modal>
  )
}
