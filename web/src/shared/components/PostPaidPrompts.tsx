import { createEffect, createSignal, onCleanup, Show } from "solid-js"
import { first, sortBy } from "@welshman/lib"
import PaymentDialog from "@/shared/components/PaymentDialog"
import PaymentSetup from "@/shared/components/PaymentSetup"
import { account } from "@/shared/lib/account"
import { invoiceStatus, listTenantInvoices, reconcileTenant, type Invoice } from "@/shared/lib/api"
import { autopayConfigured } from "@/shared/lib/paymentMethod"
import { refetchBilling, setBillingFlowActive } from "@/shared/lib/state"

type Step =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "pay"; invoice: Invoice }
  | { kind: "setup" }

// After a paid plan is picked, a tenant without automatic payments pays the
// invoice it opened and then sets them up, while the banner stays quiet.
export function usePostPaidFlow() {
  const [step, setStep] = createSignal<Step>({ kind: "idle" })

  createEffect(() => setBillingFlowActive(step().kind !== "idle"))
  onCleanup(() => setBillingFlowActive(false))

  // Whether the tenant is now being prompted, rather than paying automatically.
  async function start(): Promise<boolean> {
    setStep({ kind: "starting" })
    try {
      const pubkey = account()!.pubkey
      const tenant = await reconcileTenant(pubkey)
      if (autopayConfigured(tenant)) {
        setStep({ kind: "idle" })
        return false
      }

      const open = (await listTenantInvoices(pubkey)).filter(inv => invoiceStatus(inv) === "open" && inv.amount > 0)
      const invoice = first(sortBy(inv => -inv.period_start, open))
      setStep(invoice ? { kind: "pay", invoice } : { kind: "setup" })
      return true
    } catch (e) {
      setStep({ kind: "idle" })
      throw e
    }
  }

  return { step, setStep, start }
}

type PostPaidFlow = ReturnType<typeof usePostPaidFlow>

export default function PostPaidPrompts(props: { flow: PostPaidFlow; onDone?: () => void }) {
  const invoice = () => {
    const step = props.flow.step()
    return step.kind === "pay" ? step.invoice : undefined
  }

  return (
    <>
      <Show when={invoice()}>
        {(inv) => (
          <PaymentDialog
            invoice={inv()}
            open={true}
            onClose={() => {
              refetchBilling()
              props.flow.setStep({ kind: "setup" })
            }}
          />
        )}
      </Show>
      <PaymentSetup
        open={props.flow.step().kind === "setup"}
        onClose={() => {
          props.flow.setStep({ kind: "idle" })
          refetchBilling()
          props.onDone?.()
        }}
      />
    </>
  )
}
