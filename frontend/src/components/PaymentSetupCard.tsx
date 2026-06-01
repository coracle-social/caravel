import { PaymentSetupShell, PaymentSetupBody, SetupFooter, CardSetupBody } from "@/components/PaymentSetupShell"
import { useCardPortal } from "@/lib/usePaymentSetup"

type PaymentSetupCardProps = {
  open: boolean
  onClose: () => void
  // The tenant already has a card on file, so the copy frames this as managing
  // the existing one rather than adding a first card.
  isUpdate?: boolean
}

// Focused card dialog. PaymentSetup offers both methods behind tabs for the
// general setup flow; here the entry point is explicitly "manage your card", so
// there's no method switcher — adding/updating a card is a redirect to the
// Stripe billing portal, which returns to wherever it was opened from.
export default function PaymentSetupCard(props: PaymentSetupCardProps) {
  const card = useCardPortal()

  function handleClose() {
    card.reset()
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
      error={card.error()}
      footer={<SetupFooter cancelLabel="Cancel" onClose={handleClose} />}
    >
      <PaymentSetupBody>
        <CardSetupBody card={card} isUpdate={props.isUpdate} />
      </PaymentSetupBody>
    </PaymentSetupShell>
  )
}
