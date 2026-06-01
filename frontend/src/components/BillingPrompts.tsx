import { createMemo, createResource, createSignal, Show } from "solid-js"
import { useSearchParams } from "@solidjs/router"
import PromptBanner, { type PromptBannerAction } from "@/components/PromptBanner"
import PaymentDialog from "@/components/PaymentDialog"
import PaymentSetup from "@/components/PaymentSetup"
import { getInvoice, type Invoice } from "@/lib/api"
import { activeBillingPrompt, billingFlowActive, useBillingStatus, type BillingPromptKind } from "@/lib/billing"

type BillingPromptsProps = {
  // "banner" sits in the dashboard shell (mounted on every page except the
  // billing page); "inline" is shown contextually on the billing page itself.
  // Only one is ever mounted at a time, so each can own its own modals + deep link.
  variant?: "banner" | "inline"
}

export default function BillingPrompts(props: BillingPromptsProps) {
  const status = useBillingStatus()
  const [dismissed, setDismissed] = createSignal<Set<BillingPromptKind>>(new Set())
  const [payInvoice, setPayInvoice] = createSignal<Invoice | undefined>()
  const [setupOpen, setSetupOpen] = createSignal(false)
  const [setupTab, setSetupTab] = createSignal<"nwc" | "card">("nwc")

  // Deep link: /...?invoice=<id> (e.g. from the billing DM) opens the payment
  // dialog on whatever dashboard page the link lands on.
  const [searchParams, setSearchParams] = useSearchParams()
  const [deepLinked] = createResource(
    () => searchParams.invoice as string | undefined,
    (id) => getInvoice(id),
  )

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
        return open
          ? [
              { label: "Pay now", onClick: () => setPayInvoice(open) },
              { label: "Update payment method", onClick: () => openSetup("nwc") },
            ]
          : [{ label: "Update payment method", onClick: () => openSetup("nwc") }]
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
    if (p) setDismissed((prev) => new Set(prev).add(p.kind))
  }

  function clearDeepLink() {
    if (searchParams.invoice) setSearchParams({ invoice: undefined })
  }

  const outerClass = () => (props.variant === "inline" ? "" : "mx-4 mt-4 md:mx-6")

  return (
    <>
      <Show when={visiblePrompt()}>
        {(p) => (
          <PromptBanner
            severity={p().severity}
            message={p().message}
            actions={actions()}
            onDismiss={dismissible() ? dismiss : undefined}
            class={outerClass()}
          />
        )}
      </Show>

      {/* Pay an invoice — from a prompt action or a deep link. */}
      <Show when={payInvoice() ?? deepLinked()}>
        {(invoice) => (
          <PaymentDialog
            invoice={invoice()}
            open={true}
            onClose={() => {
              const wasDeepLink = !payInvoice()
              setPayInvoice(undefined)
              if (wasDeepLink) clearDeepLink()
              status.refetch()
            }}
          />
        )}
      </Show>

      <PaymentSetup
        open={setupOpen()}
        initialTab={setupTab()}
        onClose={() => {
          setSetupOpen(false)
          status.refetch()
        }}
      />
    </>
  )
}
