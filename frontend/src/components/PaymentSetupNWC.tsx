import { PaymentSetupShell, PaymentSetupBody, SetupFooter, NwcSetupBody } from "@/components/PaymentSetupShell"
import { useNwcSetup } from "@/lib/usePaymentSetup"

type PaymentSetupNWCProps = {
  open: boolean
  onClose: () => void
  onSaved?: () => void
  // The tenant already has a wallet connected, so the copy frames this as
  // replacing it (the stored URL is write-only and never sent back).
  isUpdate?: boolean
}

// Focused Lightning/NWC connect dialog. PaymentSetup offers both methods behind
// tabs for the general setup flow; here the entry point is explicitly "connect a
// Lightning wallet", so there's no method switcher — the card path lives on its
// own row that redirects to Stripe.
export default function PaymentSetupNWC(props: PaymentSetupNWCProps) {
  const nwc = useNwcSetup(() => props.onSaved?.())

  function handleClose() {
    nwc.reset()
    props.onClose()
  }

  return (
    <PaymentSetupShell
      open={props.open}
      onClose={handleClose}
      title={props.isUpdate ? "Update Lightning Wallet" : "Connect Lightning Wallet"}
      description={
        props.isUpdate
          ? "Paste a new Nostr Wallet Connect URL to replace your connected wallet."
          : "Paste your Nostr Wallet Connect URL to pay invoices automatically over Lightning."
      }
      error={nwc.error()}
      footer={<SetupFooter saved={nwc.saved()} cancelLabel="Cancel" onClose={handleClose} />}
    >
      <PaymentSetupBody>
        <NwcSetupBody nwc={nwc} />
      </PaymentSetupBody>
    </PaymentSetupShell>
  )
}
