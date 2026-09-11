import { createEffect, createSignal, onCleanup, Show } from "solid-js"
import { useNavigate } from "@solidjs/router"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentDialog from "@/components/PaymentDialog"
import PaymentSetup from "@/components/PaymentSetup"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import { createRelayForActiveTenant, resolvePostPaidFlow } from "@/lib/hooks"
import type { Invoice } from "@/lib/api"
import { FREE } from "@/lib/plans"
import { refetchBilling, setBillingFlowActive } from "@/lib/state"

export default function RelayNew() {
  const navigate = useNavigate()
  const [pendingInvoice, setPendingInvoice] = createSignal<Invoice | undefined>()
  const [paymentSetupOpen, setPaymentSetupOpen] = createSignal(false)
  const [creatingPaid, setCreatingPaid] = createSignal(false)
  let createdRelayId = ""

  // Suppress the shared banner's overlapping pay/setup prompts (it still surfaces
  // churn / method errors) for the whole paid-creation flow: `creatingPaid` covers
  // the async gap between create and a modal opening — without it the invoice that
  // refetchBilling surfaces flashes the banner before the modal appears — and
  // pendingInvoice/paymentSetupOpen cover the time the modals are open.
  createEffect(() => setBillingFlowActive(creatingPaid() || Boolean(pendingInvoice()) || paymentSetupOpen()))
  onCleanup(() => setBillingFlowActive(false))

  async function handleSubmit(values: RelayFormValues) {
    // Paid plans materialize an open invoice on create; mark the flow active
    // before refetchBilling surfaces it so the banner stays suppressed across the
    // whole async gap, not just once a modal opens. The finally releases it on
    // every exit (error, free plan, or after a modal has taken over suppression).
    const paid = values.plan_id !== FREE
    if (paid) setCreatingPaid(true)
    try {
      const relay = await createRelayForActiveTenant(values)
      createdRelayId = relay.id
      void refetchBilling()

      if (paid) {
        // Paid plans materialize an open invoice on create; resolvePostPaidFlow
        // reconciles and decides whether to prompt the tenant to pay it, set up a
        // payment method, or just navigate when autopay is already configured.
        const decision = await resolvePostPaidFlow()
        switch (decision.kind) {
          case "pay_invoice":
            setPendingInvoice(decision.invoice)
            return
          case "setup":
            setPaymentSetupOpen(true)
            return
          case "navigate":
            break
        }
      }

      navigate(`/relays/${relay.id}`)
    } finally {
      setCreatingPaid(false)
    }
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
