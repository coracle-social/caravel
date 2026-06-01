import { createEffect, createSignal, onCleanup, Show } from "solid-js"
import { useNavigate } from "@solidjs/router"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentDialog from "@/components/PaymentDialog"
import PaymentSetup from "@/components/PaymentSetup"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import { createRelayForActiveTenant, getLatestOpenInvoice, tenantNeedsPaymentSetup } from "@/lib/hooks"
import type { Invoice } from "@/lib/api"
import { setBillingFlowActive } from "@/lib/billing"
import { refetchBilling } from "@/lib/state"

export default function RelayNew() {
  const navigate = useNavigate()
  const [pendingInvoice, setPendingInvoice] = createSignal<Invoice | undefined>()
  const [paymentSetupOpen, setPaymentSetupOpen] = createSignal(false)
  let createdRelayId = ""

  // While this flow's inline modals are open, suppress the shared banner's
  // overlapping pay/setup prompts (it still surfaces churn / method errors).
  createEffect(() => setBillingFlowActive(Boolean(pendingInvoice()) || paymentSetupOpen()))
  onCleanup(() => setBillingFlowActive(false))

  async function handleSubmit(values: RelayFormValues) {
    const relay = await createRelayForActiveTenant(values)
    createdRelayId = relay.id
    void refetchBilling()

    if (values.plan_id !== "free") {
      const needsSetup = await tenantNeedsPaymentSetup()
      if (needsSetup) {
        // Materialize the invoice for this change (no collection, no DM) so we
        // can prompt the tenant to pay it directly. listTenantInvoices reconciles
        // first, so a just-created invoice is visible here.
        const invoice = await getLatestOpenInvoice()
        if (invoice) {
          setPendingInvoice(invoice)
          return
        }
        setPaymentSetupOpen(true)
        return
      }
    }

    navigate(`/relays/${relay.id}`)
  }

  function handleInvoiceClose() {
    setPendingInvoice(undefined)
    setPaymentSetupOpen(true)
  }

  function handleSetupClose() {
    setPaymentSetupOpen(false)
    void refetchBilling()
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
      <Show when={pendingInvoice()}>
        {(inv) => (
          <PaymentDialog
            invoice={inv()}
            open={true}
            onClose={handleInvoiceClose}
          />
        )}
      </Show>
      <PaymentSetup
        open={paymentSetupOpen()}
        onClose={handleSetupClose}
      />
    </PageContainer>
  )
}
