import { useNavigate } from "@solidjs/router"
import BackLink from "@/shared/components/BackLink"
import PageContainer from "@/shared/components/PageContainer"
import PostPaidPrompts, { usePostPaidFlow } from "@/shared/components/PostPaidPrompts"
import RelayForm, { type RelayFormValues } from "@/shared/components/RelayForm"
import { FREE } from "@/shared/lib/plans"
import { createRelayForActiveTenant } from "@/shared/lib/relays"
import { refetchBilling } from "@/shared/lib/state"

export default function RelayNew() {
  const navigate = useNavigate()
  const postPaid = usePostPaidFlow()
  let createdRelayId = ""

  async function handleSubmit(values: RelayFormValues) {
    const relay = await createRelayForActiveTenant(values)
    createdRelayId = relay.id
    void refetchBilling()

    if (values.plan_id !== FREE && await postPaid.start()) return

    navigate(`/relays/${relay.id}`)
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
      <PostPaidPrompts flow={postPaid} onDone={() => navigate(`/relays/${createdRelayId}`)} />
    </PageContainer>
  )
}
