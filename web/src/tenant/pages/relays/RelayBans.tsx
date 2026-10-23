import { useParams } from "@solidjs/router"
import { createResource, createSignal, For, Show } from "solid-js"
import PageContainer from "@/shared/components/PageContainer"
import { callRelayMethod, type BannedEvent, type BannedPubkey } from "@/shared/lib/api"
import { loaded } from "@/shared/lib/resource"
import PubkeyDialog from "@/tenant/components/relay/PubkeyDialog"
import PubkeyLabel from "@/tenant/components/relay/PubkeyLabel"
import Section from "@/tenant/components/relay/Section"
import { makeChange } from "@/tenant/lib/changes"

// Who and what a relay refuses: banned users, and events it won't take again.
export default function RelayBans() {
  const params = useParams()
  const [pubkeys, { refetch: refetchPubkeys }] = createResource(() => params.id, (id) => callRelayMethod<BannedPubkey[]>(id, "listbannedpubkeys"))
  const [events, { refetch: refetchEvents }] = createResource(() => params.id, (id) => callRelayMethod<BannedEvent[]>(id, "listbannedevents"))
  const [banning, setBanning] = createSignal(false)

  async function unban(pubkey: string) {
    if (await makeChange(() => callRelayMethod(params.id!, "unbanpubkey", [pubkey]), "Failed to lift the ban", "Ban lifted")) {
      void refetchPubkeys()
    }
  }

  async function allow(id: string) {
    if (await makeChange(() => callRelayMethod(params.id!, "allowevent", [id]), "Failed to allow the event", "Event allowed")) {
      void refetchEvents()
    }
  }

  return (
    <PageContainer>
      <div class="space-y-6">
        <Section
          title="Banned users"
          description="They can't publish to or read the relay, and what they published was deleted."
          action={
            <button type="button" onClick={() => setBanning(true)} class="shrink-0 rounded-lg bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700">
              Ban a user
            </button>
          }
        >
          <Show when={!pubkeys.error} fallback={<p class="text-sm text-red-600">Failed to load banned users.</p>}>
            <Show when={(loaded(pubkeys) ?? []).length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">{pubkeys.loading ? "Loading..." : "Nobody is banned."}</p>}>
              <ul class="divide-y divide-gray-100">
                <For each={loaded(pubkeys)}>
                  {(ban) => (
                    <li class="flex items-center justify-between gap-3 py-3">
                      <div class="min-w-0">
                        <PubkeyLabel pubkey={ban.pubkey} />
                        <Show when={ban.reason}>
                          <p class="mt-1 text-sm text-gray-500">{ban.reason}</p>
                        </Show>
                      </div>
                      <button type="button" onClick={() => void unban(ban.pubkey)} class="shrink-0 rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
                        Lift ban
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </Section>

        <Section title="Banned events" description="Deleted events the relay won't take again.">
          <Show when={!events.error} fallback={<p class="text-sm text-red-600">Failed to load banned events.</p>}>
            <Show when={(loaded(events) ?? []).length > 0} fallback={<p class="py-6 text-center text-sm text-gray-500">{events.loading ? "Loading..." : "No events are banned."}</p>}>
              <ul class="divide-y divide-gray-100">
                <For each={loaded(events)}>
                  {(ban) => (
                    <li class="flex items-center justify-between gap-3 py-3">
                      <div class="min-w-0">
                        <p class="truncate font-mono text-xs text-gray-700">{ban.id}</p>
                        <Show when={ban.reason}>
                          <p class="mt-1 text-sm text-gray-500">{ban.reason}</p>
                        </Show>
                      </div>
                      <button type="button" onClick={() => void allow(ban.id)} class="shrink-0 rounded-lg border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50">
                        Allow
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </Show>
        </Section>
      </div>

      <PubkeyDialog
        open={banning()}
        onClose={() => setBanning(false)}
        title="Ban a user"
        description="They're removed as a member, everything they published to the relay is deleted, and they can't use it again."
        submitLabel="Ban user"
        noteLabel="Reason (optional)"
        onSubmit={async (pubkey, reason) => {
          await callRelayMethod(params.id!, "banpubkey", [pubkey, reason])
          void refetchPubkeys()
        }}
      />
    </PageContainer>
  )
}
