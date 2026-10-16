import { createEffect, createSignal, Show } from "solid-js"
import { PaymentSetupShell, PaymentSetupBody, SetupFooter, NwcSetupBody, CardSetupBody } from "@/shared/components/PaymentSetupShell"
import { useCardPortal, useNwcSetup } from "@/shared/lib/usePaymentSetup"

type Tab = "nwc" | "card"

type PaymentSetupProps = {
  open: boolean
  onClose: () => void
  onSaved?: () => void
  // Which method to show first. Lightning/NWC is the default; pass "card" to land
  // a tenant on the card tab (e.g. when their card is the method that failed).
  initialTab?: Tab
}

export default function PaymentSetup(props: PaymentSetupProps) {
  const [tab, setTab] = createSignal<Tab>(props.initialTab ?? "nwc")

  // Reset to the requested tab each time the dialog opens.
  createEffect(() => {
    if (props.open) setTab(props.initialTab ?? "nwc")
  })

  const nwc = useNwcSetup(() => props.onSaved?.())
  const card = useCardPortal()

  // Surface only the active tab's error so a stale failure on one method doesn't
  // bleed into the other.
  const error = () => (tab() === "nwc" ? nwc.error() : card.error())

  function handleClose() {
    nwc.reset()
    card.reset()
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
        <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${tab() === "nwc" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
            onClick={() => setTab("nwc")}
          >
            Lightning (NWC)
          </button>
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${tab() === "card" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
            onClick={() => setTab("card")}
          >
            Card
          </button>
        </div>
      </div>

      <PaymentSetupBody>
        <Show when={tab() === "nwc"}>
          <NwcSetupBody nwc={nwc} />
        </Show>
        <Show when={tab() === "card"}>
          <CardSetupBody card={card} />
        </Show>
      </PaymentSetupBody>
    </PaymentSetupShell>
  )
}
