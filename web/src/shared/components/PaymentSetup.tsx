import { createEffect, createSignal, Show } from "solid-js"
import { PaymentSetupShell, PaymentSetupBody, SetupFooter, NwcSetupBody, CardSetupBody } from "@/shared/components/PaymentSetupShell"
import SegmentedTabs from "@/shared/components/SegmentedTabs"
import { useNwcSetup } from "@/shared/lib/useNwcSetup"
import { useRedirect } from "@/shared/lib/useRedirect"

type Method = "nwc" | "card"

type PaymentSetupProps = {
  open: boolean
  onClose: () => void
  onSaved?: () => void
  // The method shown first, such as the card when it is the one that failed.
  initialTab?: Method
}

// Setting up automatic payments by either method.
export default function PaymentSetup(props: PaymentSetupProps) {
  const [tab, setTab] = createSignal<Method>(props.initialTab ?? "nwc")

  createEffect(() => {
    if (props.open) setTab(props.initialTab ?? "nwc")
  })

  const nwc = useNwcSetup(() => props.onSaved?.())
  const billingPortal = useRedirect()

  const error = () => (tab() === "nwc" ? nwc.error() : billingPortal.error())

  function handleClose() {
    nwc.reset()
    billingPortal.reset()
    props.onClose()
  }

  return (
    <PaymentSetupShell
      open={props.open}
      onClose={handleClose}
      title="Set Up Payments"
      description="Choose how you'd like to pay once invoices are issued for your relay."
      error={error()}
      footer={<SetupFooter saved={nwc.saved()} cancelLabel="Set up later" onClose={handleClose} />}
    >
      <div class="px-6 pt-4">
        <p class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-2">Pay with</p>
        <SegmentedTabs
          tabs={[{ value: "nwc", label: "Lightning (NWC)" }, { value: "card", label: "Card" }]}
          selected={tab()}
          onSelect={setTab}
        />
      </div>

      <PaymentSetupBody>
        <Show when={tab() === "nwc"}>
          <NwcSetupBody nwc={nwc} />
        </Show>
        <Show when={tab() === "card"}>
          <CardSetupBody billingPortal={billingPortal} />
        </Show>
      </PaymentSetupBody>
    </PaymentSetupShell>
  )
}
