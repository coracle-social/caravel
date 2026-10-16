import { createEffect, createResource, createSignal, Show } from "solid-js"
import QRCode from "qrcode"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import PaymentSetup from "@/shared/components/PaymentSetup"
import SegmentedTabs from "@/shared/components/SegmentedTabs"
import InvoiceItemsList from "@/shared/components/payment/InvoiceItemsList"
import LightningPayBody, { type Lightning } from "@/shared/components/payment/LightningPayBody"
import { billingTenant, setToastMessage } from "@/shared/lib/state"
import { copyToClipboard } from "@/shared/lib/clipboard"
import { useRedirect } from "@/shared/lib/useRedirect"
import { createInvoiceCheckout, ensureInvoiceBolt11, listInvoiceItems, reconcileInvoice, type Invoice } from "@/shared/lib/api"
import { autopayConfigured } from "@/shared/lib/paymentMethod"
import { formatError, formatPeriod, formatUsd } from "@/shared/lib/format"

type PayStatus = "idle" | "loading" | "success" | "error"
type PayMethod = "lightning" | "card"

type PaymentInvoice = Pick<Invoice, "id" | "amount"> &
  Partial<Pick<Invoice, "period_start" | "period_end">>

type PaymentDialogProps = {
  invoice: PaymentInvoice
  open: boolean
  onClose: () => void
}

export default function PaymentDialog(props: PaymentDialogProps) {
  const [lightning, setLightning] = createSignal<Lightning>({ status: "loading" })
  const [payStatus, setPayStatus] = createSignal<PayStatus>("idle")
  const [payMethod, setPayMethod] = createSignal<PayMethod>("lightning")
  const [showPaymentSetup, setShowPaymentSetup] = createSignal(false)
  const [setupSaved, setSetupSaved] = createSignal(false)
  const [items] = createResource(
    () => (props.open ? props.invoice.id : undefined),
    listInvoiceItems,
  )

  // Stripe Checkout can clear a 3D Secure challenge that charging a saved card can't.
  const checkout = useRedirect()

  const hasAutopay = () => {
    const t = billingTenant()
    return t ? autopayConfigured(t) : false
  }

  async function loadBolt11() {
    if (!props.invoice.id) return
    setLightning({ status: "loading" })

    try {
      const { lnbc } = await ensureInvoiceBolt11(props.invoice.id)
      setLightning({ status: "ready", lnbc, qr: await QRCode.toDataURL(lnbc, { width: 256, margin: 2 }) })
    } catch (e) {
      setLightning({ status: "error", message: formatError(e, "Failed to generate Lightning invoice") })
    }
  }

  createEffect(() => {
    if (!props.open || !props.invoice.id) return
    void loadBolt11()
  })

  createEffect(() => {
    const err = checkout.error()
    if (err) setToastMessage(err)
  })

  async function checkPayment() {
    setPayStatus("loading")
    try {
      const invoice = await reconcileInvoice(props.invoice.id)
      if (invoice.paid_at != null) {
        setPayStatus("success")
      } else {
        setPayStatus("error")
        setToastMessage("Payment not yet confirmed. Please try again after sending.")
      }
    } catch (e) {
      setPayStatus("error")
      setToastMessage(formatError(e, "Failed to check payment status"))
    }
  }

  function handleClose() {
    setPayStatus("idle")
    setLightning({ status: "loading" })
    setPayMethod("lightning")
    checkout.reset()
    props.onClose()
  }

  const amountLabel = () => formatUsd(props.invoice.amount)

  const periodLabel = () => formatPeriod(props.invoice.period_start, props.invoice.period_end)

  return (
    <>
    <Modal open={props.open} onClose={handleClose}>
      <ModalHeader title="Pay Invoice" onClose={handleClose}>
        <p class="text-2xl font-bold text-gray-900 mt-1">{amountLabel()}</p>
        <Show when={periodLabel()}>
          <p class="text-xs text-gray-500 mt-0.5">Billing period {periodLabel()}</p>
        </Show>
      </ModalHeader>

      <div class="px-6 py-4 min-h-[240px] flex flex-col items-center justify-center">
        <Show
          when={payStatus() === "success"}
          fallback={
            <div class="w-full space-y-4">
              <Show when={(items() ?? []).length > 0}>
                <InvoiceItemsList items={items() ?? []} />
              </Show>

              <SegmentedTabs
                tabs={[{ value: "lightning", label: "Lightning" }, { value: "card", label: "Card" }]}
                selected={payMethod()}
                onSelect={setPayMethod}
              />

              <Show when={payMethod() === "lightning"}>
                <LightningPayBody
                  lightning={lightning()}
                  onRetry={() => void loadBolt11()}
                  onCopy={(lnbc) => void copyToClipboard(lnbc, { successMessage: "Invoice copied" })}
                />
              </Show>

              <Show when={payMethod() === "card"}>
                <div class="text-center space-y-4">
                  <div class="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-gray-100">
                    <svg class="w-6 h-6 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <rect x="1" y="4" width="22" height="16" rx="2" ry="2" />
                      <line x1="1" y1="10" x2="23" y2="10" />
                    </svg>
                  </div>
                  <p class="text-sm text-gray-600">
                    Pay this invoice on Stripe's secure checkout. You'll be redirected and brought back here once it's done.
                  </p>
                  <button
                    type="button"
                    onClick={() => void checkout.go(() => createInvoiceCheckout(props.invoice.id, window.location.href), "Failed to open checkout")}
                    disabled={checkout.redirecting()}
                    class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
                  >
                    {checkout.redirecting() ? "Redirecting..." : `Pay ${amountLabel()} by card`}
                  </button>
                </div>
              </Show>
            </div>
          }
        >
          <div class="text-center space-y-3">
            <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-green-100">
              <svg class="w-6 h-6 text-green-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="20 6 9 17 4 12" />
              </svg>
            </div>
            <p class="text-sm font-medium text-gray-900">Payment confirmed!</p>
            <p class="text-xs text-gray-500">Thank you. Your account is up to date.</p>
            <Show when={!hasAutopay()}>
              <button
                type="button"
                onClick={() => setShowPaymentSetup(true)}
                class="mt-2 inline-flex items-center justify-center rounded-lg border border-blue-200 bg-blue-50 px-4 py-2 text-sm font-medium text-blue-700 hover:bg-blue-100 transition-colors"
              >
                Set up automatic payments
              </button>
            </Show>
          </div>
        </Show>
      </div>

      <div class="px-6 py-4 flex justify-between gap-3 border-t border-gray-100">
        <Show
          when={payStatus() !== "success"}
          fallback={
            <div class="flex justify-end w-full">
              <button
                type="button"
                onClick={handleClose}
                class="py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700"
              >
                Done
              </button>
            </div>
          }
        >
          <button
            type="button"
            onClick={handleClose}
            class="py-2 px-4 border border-gray-300 text-gray-700 text-sm rounded-lg hover:bg-gray-50 transition-colors"
          >
            Pay Later
          </button>
          <Show when={payMethod() === "lightning"}>
            <button
              type="button"
              onClick={checkPayment}
              disabled={payStatus() === "loading" || lightning().status !== "ready"}
              class="py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
            >
              {payStatus() === "loading" ? "Checking..." : "Complete Payment"}
            </button>
          </Show>
        </Show>
      </div>
    </Modal>
    <PaymentSetup
      open={showPaymentSetup()}
      onClose={() => {
        setShowPaymentSetup(false)
        if (setupSaved()) {
          setSetupSaved(false)
          props.onClose()
        }
      }}
      onSaved={() => setSetupSaved(true)}
    />
    </>
  )
}
