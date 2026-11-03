import { A } from "@solidjs/router"
import { Show } from "solid-js"
import { canonicalHost, relayName, relayStatus, type Relay } from "@/shared/lib/api"
import { PlanBadge } from "@/shared/components/relay/RelayCardHeader"
import Avatar from "@/shared/components/Avatar"
import CopyNpub from "@/shared/components/CopyNpub"
import StatusBadge from "@/shared/components/StatusBadge"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"

type RelayListItemProps = {
  relay: Relay
  href: string
  showTenant?: boolean
  // Whether the tenant can be paid, for a list that knows it.
  payoutsReady?: boolean
}

// A relay in a list. A list that shows tenants fetches their profiles itself.
export default function RelayListItem(props: RelayListItemProps) {
  const metadata = useProfileMetadata(() => (props.showTenant ? props.relay.tenant_pubkey : undefined), { prime: false })

  return (
    <li>
      <A href={props.href} class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300">
        <div class="flex items-center justify-between gap-3">
          <div class="min-w-0">
            <p class="font-medium text-gray-900">{relayName(props.relay)}</p>
            <p class="text-xs text-gray-500">{canonicalHost(props.relay)}</p>
            <Show when={props.showTenant}>
              <div class="mt-1.5 flex items-center gap-2">
                <Avatar profile={metadata()} pubkey={props.relay.tenant_pubkey} class="h-5 w-5 flex-shrink-0" fallbackClass="bg-gray-200 text-[10px] text-gray-500" />
                <Show
                  when={profileName(metadata())}
                  fallback={<CopyNpub pubkey={props.relay.tenant_pubkey} class="text-xs text-gray-500" />}
                >
                  {(name) => <span class="text-xs text-gray-500 truncate">{name()}</span>}
                </Show>
              </div>
            </Show>
          </div>
          <div class="flex items-center gap-2 shrink-0">
            <Show when={props.relay.member_price > 0 && props.payoutsReady === false}>
              <StatusBadge status="incomplete">Payouts needed</StatusBadge>
            </Show>
            <StatusBadge status={relayStatus(props.relay)} />
            <PlanBadge planId={props.relay.plan_id} />
          </div>
        </div>
      </A>
    </li>
  )
}
