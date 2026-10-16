import { A } from "@solidjs/router"
import { Show } from "solid-js"
import { invoiceStatus, type Invoice } from "@/shared/lib/api"
import Avatar from "@/shared/components/Avatar"
import Field from "@/shared/components/Field"
import CopyNpub from "@/shared/components/CopyNpub"
import StatusBadge from "@/shared/components/StatusBadge"
import { profileName, useProfileMetadata } from "@/shared/lib/profiles"
import { formatDate, formatPeriod, formatUsd } from "@/shared/lib/format"

export default function InvoiceDetailCard(props: { invoice: Invoice }) {
  const metadata = useProfileMetadata(() => props.invoice.tenant_pubkey, { prime: false })

  return (
    <div class="bg-white border border-gray-200 rounded-xl p-6 space-y-6">
      <div class="flex items-center justify-between gap-3">
        <p class="text-2xl font-bold text-gray-900">{formatUsd(props.invoice.amount)}</p>
        <StatusBadge status={invoiceStatus(props.invoice)} />
      </div>

      <hr class="border-gray-200" />

      <dl class="grid gap-x-6 gap-y-4 sm:grid-cols-2">
        <Field label="Billing period">
          <span class="text-gray-900">{formatPeriod(props.invoice.period_start, props.invoice.period_end)}</span>
        </Field>
        <Field label="Created">
          <span class="text-gray-900">{formatDate(props.invoice.created_at)}</span>
        </Field>
        <Show when={props.invoice.method}>
          <Field label="Payment method">
            <span class="uppercase text-gray-900">{props.invoice.method}</span>
          </Field>
        </Show>
        <Field label="Tenant">
          <A href={`/admin/tenants/${props.invoice.tenant_pubkey}`} class="group flex w-fit items-center gap-2 min-w-0">
            <Avatar profile={metadata()} pubkey={props.invoice.tenant_pubkey} class="h-5 w-5 flex-shrink-0" fallbackClass="bg-gray-200 text-[10px] text-gray-500" />
            <Show
              when={profileName(metadata())}
              fallback={<CopyNpub pubkey={props.invoice.tenant_pubkey} class="text-blue-600 group-hover:underline" />}
            >
              {(name) => <span class="truncate text-blue-600 group-hover:underline">{name()}</span>}
            </Show>
          </A>
        </Field>
      </dl>
    </div>
  )
}
