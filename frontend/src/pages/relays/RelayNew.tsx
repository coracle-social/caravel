import { createSignal } from "solid-js"
import { useNavigate } from "@solidjs/router"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentDialog from "@/components/PaymentDialog"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import { checkPendingInvoice, createRelayForActiveTenant, type Invoice } from "@/lib/hooks"

export default function RelayNew() {
  const navigate = useNavigate()
  const [pendingInvoice, setPendingInvoice] = createSignal<Invoice | undefined>()
  let createdRelayId = ""

  async function handleSubmit(values: RelayFormValues) {
    const relay = await createRelayForActiveTenant(values)
    createdRelayId = relay.id

    if (values.plan !== "free") {
      const invoice = await checkPendingInvoice()
      if (invoice) {
        setPendingInvoice(invoice)
        return
      }
    }

    navigate(`/relays/${relay.id}`)
  }

  function handleDialogClose() {
    setPendingInvoice(undefined)
    navigate(`/relays/${createdRelayId}`)
  }

  return (
    <PageContainer size="narrow">
      <BackLink href="/relays" label="Relays" />
      <h1 class="text-2xl font-bold text-gray-900 mb-6 py-2">New Relay</h1>
      <RelayForm
        syncSubdomainWithName
        onSubmit={handleSubmit}
        submitLabel="Create Relay"
        submittingLabel="Creating..."
      />
      {pendingInvoice() && (
        <PaymentDialog
          invoice={pendingInvoice()!}
          open={!!pendingInvoice()}
          onClose={handleDialogClose}
        />
      )}
    </PageContainer>
  )
}
