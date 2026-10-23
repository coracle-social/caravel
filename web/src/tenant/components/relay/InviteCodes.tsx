import { createResource, createSignal, For, Show } from "solid-js"
import ActionsMenu, { ActionsMenuItem } from "@/shared/components/ActionsMenu"
import { callRelayMethod, canonicalHost, type Relay } from "@/shared/lib/api"
import { copyToClipboard } from "@/shared/lib/clipboard"
import { FLOTILLA_URL } from "@/shared/lib/config"
import { loaded } from "@/shared/lib/resource"
import Section from "@/tenant/components/relay/Section"
import { makeChange } from "@/tenant/lib/changes"

// An invite code that's hard to guess and easy to read out.
const newCode = () =>
  Array.from(crypto.getRandomValues(new Uint8Array(5)), byte => byte.toString(36).padStart(2, "0")).join("").slice(0, 8)

// The relay's invite codes (NIP-43 claims): each lets whoever has it join the relay.
export default function InviteCodes(props: { relay: Relay }) {
  const [codes, { refetch }] = createResource(() => props.relay.id, (id) => callRelayMethod<string[]>(id, "listclaims"))
  const [creating, setCreating] = createSignal(false)

  const inviteUrl = (code: string) => `${FLOTILLA_URL}/join?r=${encodeURIComponent(canonicalHost(props.relay))}&c=${code}`

  async function create() {
    setCreating(true)
    const code = newCode()
    const created = await makeChange(() => callRelayMethod(props.relay.id, "createclaim", [code]), "Failed to create invite code")
    setCreating(false)
    if (created) {
      void refetch()
      void copyToClipboard(inviteUrl(code), { successMessage: "Invite link copied" })
    }
  }

  async function revoke(code: string) {
    if (await makeChange(() => callRelayMethod(props.relay.id, "deleteclaim", [code]), "Failed to revoke invite code", "Invite code revoked")) {
      void refetch()
    }
  }

  return (
    <Section
      title="Invite codes"
      description="Each code lets whoever has it join the relay, until it's revoked."
      action={
        <button type="button" onClick={create} disabled={creating()} class="shrink-0 rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50 disabled:opacity-50">
          {creating() ? "Creating..." : "New code"}
        </button>
      }
    >
      <Show when={!codes.error} fallback={<p class="text-sm text-red-600">Failed to load invite codes.</p>}>
        <Show when={(loaded(codes) ?? []).length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">{codes.loading ? "Loading..." : "No invite codes."}</p>}>
          <ul class="divide-y divide-gray-100">
            <For each={loaded(codes)}>
              {(code) => (
                <li class="flex items-center justify-between gap-3 py-3">
                  <span class="font-mono text-sm text-gray-900">{code}</span>
                  <ActionsMenu>
                    {(close) => (
                      <>
                        <ActionsMenuItem onClick={() => { close(); void copyToClipboard(inviteUrl(code), { successMessage: "Invite link copied" }) }}>
                          Copy invite link
                        </ActionsMenuItem>
                        <ActionsMenuItem tone="danger" onClick={() => { close(); void revoke(code) }}>Revoke</ActionsMenuItem>
                      </>
                    )}
                  </ActionsMenu>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </Section>
  )
}
