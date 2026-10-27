import { createEffect, createMemo, createResource, createSignal, Show } from "solid-js"
import { useSearchParams } from "@solidjs/router"
import PromptBanner, { type PromptBannerAction } from "@/shared/components/PromptBanner"
import PaymentDialog from "@/shared/components/PaymentDialog"
import PaymentSetup from "@/shared/components/PaymentSetup"
import { getInvoice, type Invoice } from "@/shared/lib/api"
import { activeBillingPrompt, useBillingStatus, type BillingPromptKind } from "@/shared/lib/billing"
import { account } from "@/shared/lib/account"
import { formatError } from "@/shared/lib/format"
import { loaded } from "@/shared/lib/resource"
import { billingFlowActive, setToastMessage } from "@/shared/lib/state"

// The banner asking a tenant to sort out billing, and its dialogs. A
// `?invoice=<id>` link, as billing DMs send, opens that invoice's.
export default function BillingPrompts() {
  const status = useBillingStatus()
  const [dismissed, setDismissed] = createSignal<Set<BillingPromptKind>>(new Set())
  const [payInvoice, setPayInvoice] = createSignal<Invoice | undefined>()
  const [setupOpen, setSetupOpen] = createSignal(false)
  const [setupTab, setSetupTab] = createSignal<"nwc" | "card">("nwc")

  const [searchParams, setSearchParams] = useSearchParams()
  const [deepLinked] = createResource(
    () => searchParams.invoice as string | undefined,
    (id) => getInvoice(id),
  )

  // A resource keeps its value after its source clears, so this reads the param too.
  const deepLinkedInvoice = createMemo(() =>
    searchParams.invoice ? loaded(deepLinked) : undefined,
  )

  createEffect(() => {
    if (deepLinked.error) setToastMessage(formatError(deepLinked.error, "Failed to load the invoice"))
  })

  const prompt = createMemo(() =>
    activeBillingPrompt(
      {
        tenant: status.tenant(),
        openInvoice: status.openInvoice(),
        hasPaidSubscription: status.hasPaidSubscription(),
      },
      { suppressInline: billingFlowActive() },
    ),
  )

  const visiblePrompt = createMemo(() => {
    const p = prompt()
    if (!p || dismissed().has(p.kind)) return undefined
    return p
  })

  function openSetup(tab: "nwc" | "card") {
    setSetupTab(tab)
    setSetupOpen(true)
  }

  const actions = createMemo<PromptBannerAction[]>(() => {
    const p = visiblePrompt()
    if (!p) return []
    const open = status.openInvoice()
    switch (p.kind) {
      case "churned":
        return [{ label: "Update payment method", onClick: () => openSetup("nwc") }]
      case "pay_invoice":
        return open ? [{ label: "Pay now", onClick: () => setPayInvoice(open) }] : []
      case "update_method":
        return [{ label: "Update payment method", onClick: () => openSetup(status.tenant()?.nwc_error ? "nwc" : "card") }]
      case "setup_autopay":
        return [{ label: "Set up autopay", onClick: () => openSetup("nwc") }]
    }
    return []
  })

  const dismissible = () => visiblePrompt()?.kind === "setup_autopay"

  function dismiss() {
    const p = visiblePrompt()
    if (p) setDismissed((prev) => new Set([...prev, p.kind]))
  }

  function clearDeepLink() {
    if (searchParams.invoice) setSearchParams({ invoice: undefined })
  }

  // After paying or saving a method, collect what's owed with it, then refresh.
  function refreshBilling() {
    const pubkey = account()?.pubkey
    if (pubkey) void status.autopay(pubkey)
  }

  return (
    <>
      <Show when={visiblePrompt()}>
        {(p) => (
          <PromptBanner
            severity={p().severity}
            message={p().message}
            actions={actions()}
            onDismiss={dismissible() ? dismiss : undefined}
            class="mx-4 mt-4 md:mx-6"
          />
        )}
      </Show>

      <Show when={payInvoice() ?? deepLinkedInvoice()}>
        {(invoice) => (
          <PaymentDialog
            invoice={invoice()}
            open={true}
            onClose={() => {
              const wasDeepLink = !payInvoice()
              setPayInvoice(undefined)
              if (wasDeepLink) clearDeepLink()
              refreshBilling()
            }}
          />
        )}
      </Show>

      <PaymentSetup
        open={setupOpen()}
        initialTab={setupTab()}
        onClose={() => {
          setSetupOpen(false)
          refreshBilling()
        }}
      />
    </>
  )
}
