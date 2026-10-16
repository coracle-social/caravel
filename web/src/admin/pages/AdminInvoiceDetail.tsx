import { useParams } from "@solidjs/router"
import { createResource, Show } from "solid-js"
import BackLink from "@/shared/components/BackLink"
import InvoiceDetailCard from "@/admin/components/InvoiceDetailCard"
import InvoiceItemsList from "@/shared/components/payment/InvoiceItemsList"
import PageContainer from "@/shared/components/PageContainer"
import ResourceState from "@/shared/components/ResourceState"
import useMinLoading from "@/shared/lib/useMinLoading"
import { listInvoiceItems } from "@/shared/lib/api"
import { useInvoice } from "@/shared/lib/hooks"

export default function AdminInvoiceDetail() {
  const params = useParams()
  const invoiceId = () => params.id ?? ""
  const [invoice] = useInvoice(invoiceId)
  const [items] = createResource(invoiceId, async (id) => {
    if (!id) return []

    try {
      return await listInvoiceItems(id)
    } catch {
      return []
    }
  })
  const loading = useMinLoading(() => invoice.loading && !invoice())

  return (
    <PageContainer>
      <BackLink label="Back" />
      <ResourceState loading={loading()} error={invoice.error} loadingText="Loading invoice..." errorText="Failed to load invoice." class="mb-4" />
      <Show when={!loading() && invoice()}>
        {(i) => (
          <div class="space-y-6 mb-6">
            <InvoiceDetailCard invoice={i()} />
            <Show when={(items()?.length ?? 0) > 0}>
              <InvoiceItemsList items={items() ?? []} />
            </Show>
          </div>
        )}
      </Show>
    </PageContainer>
  )
}
