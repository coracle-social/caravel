import { A } from "@solidjs/router"
import { createEffect, createSignal, onCleanup, Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import type { Relay } from "@/lib/api"
import { eventStore } from "@/lib/state"

type RelayListItemProps = {
  relay: Relay
  href: string
  showTenant?: boolean
}

function shortenPubkey(pubkey: string) {
  return pubkey.length <= 16 ? pubkey : `${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`
}

export default function RelayListItem(props: RelayListItemProps) {
  const [tenantProfile, setTenantProfile] = createSignal<{ name?: string; picture?: string }>({})

  // Resolve the owning tenant's profile from the event store. The list that
  // passes `showTenant` is responsible for priming these profiles in one batch.
  createEffect(() => {
    if (!props.showTenant) return
    const pubkey = props.relay.tenant_pubkey
    if (!pubkey) return

    const sub = eventStore.profile(pubkey).subscribe((metadata) => {
      setTenantProfile({
        name: metadata?.name || metadata?.display_name,
        picture: getProfilePicture(metadata),
      })
    })
    onCleanup(() => sub.unsubscribe())
  })

  return (
    <li>
      <A href={props.href} class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300">
        <div class="flex items-center justify-between gap-3">
          <div class="min-w-0">
            <p class="font-medium text-gray-900">{props.relay.info_name || props.relay.subdomain}</p>
            <p class="text-xs text-gray-500">{props.relay.subdomain}.spaces.coracle.social</p>
            <Show when={props.showTenant}>
              <div class="mt-1.5 flex items-center gap-2">
                <Show
                  when={tenantProfile().picture}
                  fallback={
                    <div class="h-5 w-5 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-[10px] text-gray-500">
                      {(tenantProfile().name || props.relay.tenant_pubkey).slice(0, 1).toUpperCase()}
                    </div>
                  }
                >
                  <img src={tenantProfile().picture} alt="" class="h-5 w-5 flex-shrink-0 rounded-full object-cover" />
                </Show>
                <span class="text-xs text-gray-500 truncate">{tenantProfile().name || shortenPubkey(props.relay.tenant_pubkey)}</span>
              </div>
            </Show>
          </div>
          <Show
            when={props.relay.sync_error}
            fallback={<p class="text-xs uppercase tracking-wide text-gray-500">{props.relay.status}</p>}
          >
            <span
              class="inline-flex items-center rounded-full border border-red-200 bg-red-50 px-2.5 py-0.5 text-xs font-medium text-red-700 max-w-56 truncate"
              title={props.relay.sync_error}
            >
              Failed to sync
            </span>
          </Show>
        </div>
      </A>
    </li>
  )
}
