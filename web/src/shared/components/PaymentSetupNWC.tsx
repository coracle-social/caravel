import { PaymentSetupShell, PaymentSetupBody, SetupFooter, NwcSetupBody } from "@/shared/components/PaymentSetupShell"
import { useNwcSetup } from "@/shared/lib/useNwcSetup"

// Connecting a Lightning wallet. `isUpdate` replaces one, whose url is never sent back.
export default function PaymentSetupNWC(props: { open: boolean; onClose: () => void; onSaved?: () => void; isUpdate?: boolean }) {
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
