import { A } from "@solidjs/router"
import { Show } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { invoiceStatus, type Invoice } from "@/shared/lib/api"
import Field from "@/shared/components/Field"
import CopyNpub from "@/shared/components/CopyNpub"
import { useProfileMetadata } from "@/shared/lib/hooks"
import { formatPeriod, formatUsd } from "@/shared/lib/format"

const invoiceStatusStyles: Record<string, string> = {
  open: "bg-yellow-50 text-yellow-700 border-yellow-200",
  paid: "bg-green-50 text-green-700 border-green-200",
  void: "bg-gray-100 text-gray-500 border-gray-200",
}

type InvoiceDetailCardProps = {
  invoice: Invoice
}

export default function InvoiceDetailCard(props: InvoiceDetailCardProps) {
  // Resolve the owning tenant's profile so the Tenant field can show a name and
  // avatar instead of a raw pubkey, like the admin list item.
  const metadata = useProfileMetadata(() => props.invoice.tenant_pubkey, { prime: false })

  const status = () => invoiceStatus(props.invoice)

  return (
    <div class="bg-white border border-gray-200 rounded-xl p-6 space-y-6">
      <div class="flex items-center justify-between gap-3">
        <p class="text-2xl font-bold text-gray-900">{formatUsd(props.invoice.amount)}</p>
        <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${invoiceStatusStyles[status()] ?? "bg-gray-100 text-gray-500 border-gray-200"}`}>
          {status()}
        </span>
      </div>

      <hr class="border-gray-200" />

      <dl class="grid gap-x-6 gap-y-4 sm:grid-cols-2">
        <Field label="Billing period">
          <span class="text-gray-900">{formatPeriod(props.invoice.period_start, props.invoice.period_end)}</span>
        </Field>
        <Field label="Created">
          <span class="text-gray-900">{new Date(props.invoice.created_at * 1000).toLocaleDateString()}</span>
        </Field>
        <Show when={props.invoice.method}>
          <Field label="Payment method">
            <span class="uppercase text-gray-900">{props.invoice.method}</span>
          </Field>
        </Show>
        <Field label="Tenant">
          <A href={`/admin/tenants/${props.invoice.tenant_pubkey}`} class="group flex w-fit items-center gap-2 min-w-0">
            <Show
              when={getProfilePicture(metadata())}
              fallback={
                <div class="h-5 w-5 flex-shrink-0 rounded-full bg-gray-200 flex items-center justify-center text-[10px] text-gray-500">
                  {((metadata()?.name || metadata()?.display_name) || props.invoice.tenant_pubkey).slice(0, 1).toUpperCase()}
                </div>
              }
            >
              <img src={getProfilePicture(metadata())} alt="" class="h-5 w-5 flex-shrink-0 rounded-full object-cover" />
            </Show>
            <Show
              when={metadata()?.name || metadata()?.display_name}
              fallback={<CopyNpub pubkey={props.invoice.tenant_pubkey} class="text-blue-600 group-hover:underline" />}
            >
              <span class="truncate text-blue-600 group-hover:underline">{metadata()?.name || metadata()?.display_name}</span>
            </Show>
          </A>
        </Field>
      </dl>
    </div>
  )
}
