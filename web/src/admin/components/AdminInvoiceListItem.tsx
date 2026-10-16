import { A } from "@solidjs/router"
import { Show } from "solid-js"
import { invoiceStatus, type Invoice } from "@/shared/lib/api"
import Avatar from "@/shared/components/Avatar"
import CopyNpub from "@/shared/components/CopyNpub"
import StatusBadge from "@/shared/components/StatusBadge"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"
import { formatPeriod, formatUsd } from "@/shared/lib/format"

type AdminInvoiceListItemProps = {
  invoice: Invoice
  href: string
  showTenant?: boolean
}

// An invoice in a list. A list that shows tenants fetches their profiles itself.
export default function AdminInvoiceListItem(props: AdminInvoiceListItemProps) {
  const metadata = useProfileMetadata(() => (props.showTenant ? props.invoice.tenant_pubkey : undefined), { prime: false })

  return (
    <li>
      <A href={props.href} class="block rounded-lg border border-gray-200 bg-white p-4 hover:border-gray-300">
        <div class="flex items-center justify-between gap-3">
          <div class="min-w-0">
            <p class="font-medium text-gray-900">{formatUsd(props.invoice.amount)}</p>
            <p class="text-xs text-gray-500">{formatPeriod(props.invoice.period_start, props.invoice.period_end)}</p>
            <Show when={props.showTenant}>
              <div class="mt-1.5 flex items-center gap-2">
                <Avatar profile={metadata()} pubkey={props.invoice.tenant_pubkey} class="h-5 w-5 flex-shrink-0" fallbackClass="bg-gray-200 text-[10px] text-gray-500" />
                <Show
                  when={profileName(metadata())}
                  fallback={<CopyNpub pubkey={props.invoice.tenant_pubkey} class="text-xs text-gray-500" />}
                >
                  {(name) => <span class="text-xs text-gray-500 truncate">{name()}</span>}
                </Show>
              </div>
            </Show>
          </div>
          <StatusBadge status={invoiceStatus(props.invoice)} />
        </div>
      </A>
    </li>
  )
}
