import { createSignal } from "solid-js"
import { useNavigate } from "@solidjs/router"
import BackLink from "@/components/BackLink"
import PageContainer from "@/components/PageContainer"
import PaymentSetup from "@/components/PaymentSetup"
import RelayForm, { type RelayFormValues } from "@/components/RelayForm"
import { createRelayForActiveTenant, tenantNeedsPaymentSetup } from "@/lib/hooks"

export default function RelayNew() {
  const navigate = useNavigate()
  const [showPaymentSetup, setShowPaymentSetup] = createSignal(false)
  let createdRelayId = ""

  async function handleSubmit(values: RelayFormValues) {
    const relay = await createRelayForActiveTenant(values)
    createdRelayId = relay.id

    if (values.plan !== "free") {
      const needs = await tenantNeedsPaymentSetup()
      if (needs) {
        setShowPaymentSetup(true)
        return
      }
    }

    navigate(`/relays/${relay.id}`)
  }

  function handleDialogClose() {
    setShowPaymentSetup(false)
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
      <PaymentSetup
        open={showPaymentSetup()}
        onClose={handleDialogClose}
      />
    </PageContainer>
  )
}
