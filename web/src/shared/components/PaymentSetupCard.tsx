import { PaymentSetupShell, PaymentSetupBody, SetupFooter, CardSetupBody } from "@/shared/components/PaymentSetupShell"
import { useRedirect } from "@/shared/lib/useRedirect"

// Adding or managing the card on file. `isUpdate` is for a tenant who already has one.
export default function PaymentSetupCard(props: { open: boolean; onClose: () => void; isUpdate?: boolean }) {
  const billingPortal = useRedirect()

  function handleClose() {
    billingPortal.reset()
    props.onClose()
  }

  return (
    <PaymentSetupShell
      open={props.open}
      onClose={handleClose}
      title={props.isUpdate ? "Manage Card" : "Add a Card"}
      description={
        props.isUpdate
          ? "Manage your saved card in the Stripe billing portal."
          : "Add a card via the Stripe billing portal to pay invoices automatically."
      }
      error={billingPortal.error()}
      footer={<SetupFooter cancelLabel="Cancel" onClose={handleClose} />}
    >
      <PaymentSetupBody>
        <CardSetupBody billingPortal={billingPortal} isUpdate={props.isUpdate} />
      </PaymentSetupBody>
    </PaymentSetupShell>
  )
}
