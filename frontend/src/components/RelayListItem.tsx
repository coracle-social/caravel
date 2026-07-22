import { A } from "@solidjs/router"
import { Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { relayStatus, type Relay } from "@/lib/api"
import { PlanBadge, StatusBadge } from "@/components/relay/RelayCardHeader"
import CopyNpub from "@/components/CopyNpub"
import { useProfileMetadata } from "@/lib/hooks"

type RelayListItemProps = {
  relay: Relay
  href: string
  showTenant?: boolean
}

export default function RelayListItem(props: RelayListItemProps) {
  // Resolve the owning tenant's profile from the event store. The list that
  // passes `showTenant` is responsible for priming these profiles in one batch,
  // so this subscription does not prime on its own.
  const metadata = useProfileMetadata(() => (props.showTenant ? props.relay.tenant_pubkey : undefined), { prime: false })

  return (
    <li>
      <A href={props.href} class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300">
        <div class="flex items-center justify-between gap-3">
          <div class="min-w-0">
            <p class="font-medium text-gray-900">{props.relay.info_name || props.relay.subdomain}</p>
            <p class="text-xs text-gray-500">{props.relay.subdomain}.{props.relay.zooid_domain}</p>
            <Show when={props.showTenant}>
              <div class="mt-1.5 flex items-center gap-2">
                <Show
                  when={getProfilePicture(metadata())}
                  fallback={
                    <div class="h-5 w-5 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-[10px] text-gray-500">
                      {((metadata()?.name || metadata()?.display_name) || props.relay.tenant_pubkey).slice(0, 1).toUpperCase()}
                    </div>
                  }
                >
                  <img src={getProfilePicture(metadata())} alt="" class="h-5 w-5 flex-shrink-0 rounded-full object-cover" />
                </Show>
                <Show
                  when={metadata()?.name || metadata()?.display_name}
                  fallback={<CopyNpub pubkey={props.relay.tenant_pubkey} class="text-xs text-gray-500" />}
                >
                  <span class="text-xs text-gray-500 truncate">{metadata()?.name || metadata()?.display_name}</span>
                </Show>
              </div>
            </Show>
          </div>
          <div class="flex items-center gap-2 shrink-0">
            <Show
              when={props.relay.sync_error}
              fallback={<StatusBadge status={relayStatus(props.relay)} />}
            >
              <span
                class="inline-flex items-center rounded-full border border-red-200 bg-red-50 px-2.5 py-0.5 text-xs font-medium text-red-700 max-w-56 truncate"
                title={props.relay.sync_error}
              >
                Failed to sync
              </span>
            </Show>
            <PlanBadge planId={props.relay.plan_id} />
          </div>
        </div>
      </A>
    </li>
  )
}
