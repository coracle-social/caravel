import { A } from "@solidjs/router"
import { Show } from "solid-js"
import type { Relay } from "@/lib/api"

type RelayListItemProps = {
  relay: Relay
  href: string
  showTenant?: boolean
}

export default function RelayListItem(props: RelayListItemProps) {
  return (
    <li>
      <A href={props.href} class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300">
        <div class="flex items-center justify-between gap-3">
          <div>
            <p class="font-medium text-gray-900">{props.relay.info_name || props.relay.subdomain}</p>
            <p class="text-xs text-gray-500">{props.relay.subdomain}.spaces.coracle.social</p>
            {props.showTenant && (
              <p class="text-xs text-gray-500 break-all mt-1">Tenant: {props.relay.tenant}</p>
            )}
          </div>
          <Show
            when={props.relay.sync_error}
            fallback={<p class="text-xs uppercase tracking-wide text-gray-500">{props.relay.status}</p>}
          >
            <span
              class="inline-flex items-center rounded-full border border-red-200 bg-red-50 px-2.5 py-0.5 text-xs font-medium text-red-700 max-w-56 truncate"
              title={props.relay.sync_error}
            >
              {props.relay.sync_error}
            </span>
          </Show>
        </div>
      </A>
    </li>
  )
}
