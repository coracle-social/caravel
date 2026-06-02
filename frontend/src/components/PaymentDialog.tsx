import { createEffect, createResource, createSignal, Show } from "solid-js"
import QRCode from "qrcode"
import Modal from "@/components/Modal"
import PaymentSetup from "@/components/PaymentSetup"
import { CardSetupBody } from "@/components/PaymentSetupShell"
import InvoiceItemsList from "@/components/payment/InvoiceItemsList"
import LightningPayBody from "@/components/payment/LightningPayBody"
import { setToastMessage } from "@/lib/state"
import { copyToClipboard } from "@/lib/clipboard"
import { useCardPortal } from "@/lib/usePaymentSetup"
import { ensureInvoiceBolt11, listInvoiceItems, reconcileInvoice, type Invoice } from "@/lib/api"
import { autopayConfigured } from "@/lib/paymentMethod"
import { billingTenant } from "@/lib/state"
import { formatUsd, formatPeriod } from "@/lib/format"

type PayStatus = "idle" | "loading" | "success" | "error"
type Bolt11Status = "idle" | "loading" | "ready" | "error"
type PayMethod = "lightning" | "card"

type PaymentInvoice = Pick<Invoice, "id" | "amount"> &
  Partial<Pick<Invoice, "period_start" | "period_end">>

type PaymentDialogProps = {
  invoice: PaymentInvoice
  open: boolean
  onClose: () => void
}

export default function PaymentDialog(props: PaymentDialogProps) {
  const [bolt11, setBolt11] = createSignal("")
  const [qrDataUrl, setQrDataUrl] = createSignal("")
  const [bolt11Status, setBolt11Status] = createSignal<Bolt11Status>("idle")
  const [bolt11Error, setBolt11Error] = createSignal("")
  const [payStatus, setPayStatus] = createSignal<PayStatus>("idle")
  const [payMethod, setPayMethod] = createSignal<PayMethod>("lightning")
  const [showPaymentSetup, setShowPaymentSetup] = createSignal(false)
  const [setupSaved, setSetupSaved] = createSignal(false)
  const [items] = createResource(
    () => (props.open ? props.invoice.id : undefined),
    listInvoiceItems,
  )

  // Card payment is a redirect to the Stripe billing portal; once a card is on
  // file we retry collection on this invoice automatically.
  const card = useCardPortal()

  const hasAutopay = () => {
    const t = billingTenant()
    return t ? autopayConfigured(t) : false
  }

  async function loadBolt11() {
    if (!props.invoice.id) return
    setBolt11Status("loading")
    setBolt11Error("")
    setBolt11("")
    setQrDataUrl("")

    try {
      const { lnbc } = await ensureInvoiceBolt11(props.invoice.id)
      setBolt11(lnbc)
      setQrDataUrl(await QRCode.toDataURL(lnbc, { width: 256, margin: 2 }))
      setBolt11Status("ready")
    } catch (e) {
      setBolt11Status("error")
      setBolt11Error(e instanceof Error ? e.message : "Failed to generate Lightning invoice")
    }
  }

  createEffect(() => {
    if (!props.open || !props.invoice.id) return
    void loadBolt11()
  })

  // The card portal lives in a shared hook, so surface its failures here by
  // mirroring its error signal into the toast.
  createEffect(() => {
    const err = card.error()
    if (err) setToastMessage(err)
  })

  function copyBolt11() {
    void copyToClipboard(bolt11(), { successMessage: "Invoice copied" })
  }

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
      setToastMessage(e instanceof Error ? e.message : "Failed to check payment status")
    }
  }

  function handleClose() {
    setPayStatus("idle")
    setBolt11Status("idle")
    setBolt11Error("")
    setBolt11("")
    setQrDataUrl("")
    setPayMethod("lightning")
    card.reset()
    props.onClose()
  }

  const amountLabel = () => formatUsd(props.invoice.amount)

  const periodLabel = () => formatPeriod(props.invoice.period_start, props.invoice.period_end)

  return (
    <>
    <Modal
      open={props.open}
      onClose={handleClose}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      panelClass="w-full max-w-md rounded-2xl bg-white shadow-xl overflow-hidden"
    >
      {/* Header */}
      <div class="px-6 pt-6 pb-4 border-b border-gray-100">
        <div class="flex items-start justify-between gap-3">
          <div>
            <h2 class="text-lg font-semibold text-gray-900">Pay Invoice</h2>
            <p class="text-2xl font-bold text-gray-900 mt-1">{amountLabel()}</p>
            <Show when={periodLabel()}>
              <p class="text-xs text-gray-500 mt-0.5">Billing period {periodLabel()}</p>
            </Show>
          </div>
          <button
            type="button"
            onClick={handleClose}
            class="text-gray-400 hover:text-gray-700 rounded p-1 hover:bg-gray-100 flex-shrink-0"
            aria-label="Close"
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>

      {/* Content */}
      <div class="px-6 py-4 min-h-[240px] flex flex-col items-center justify-center">
        <Show
          when={payStatus() === "success"}
          fallback={
            <div class="w-full space-y-4">
              {/* What's being paid for — the invoice's actual line items */}
              <Show when={(items() ?? []).length > 0}>
                <InvoiceItemsList items={items() ?? []} />
              </Show>

              {/* Method switcher */}
              <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
                <button
                  type="button"
                  class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${payMethod() === "lightning" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
                  onClick={() => setPayMethod("lightning")}
                >
                  Lightning
                </button>
                <button
                  type="button"
                  class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${payMethod() === "card" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
                  onClick={() => setPayMethod("card")}
                >
                  Card
                </button>
              </div>

              {/* Lightning: pay this invoice via a bolt11 QR */}
              <Show when={payMethod() === "lightning"}>
                <LightningPayBody
                  bolt11Status={bolt11Status}
                  bolt11={bolt11}
                  qrDataUrl={qrDataUrl}
                  bolt11Error={bolt11Error}
                  onRetry={() => void loadBolt11()}
                  onCopy={copyBolt11}
                />
              </Show>

              {/* Card: redirect to the Stripe billing portal */}
              <Show when={payMethod() === "card"}>
                <CardSetupBody card={card} />
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

      {/* Footer */}
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
              disabled={payStatus() === "loading" || bolt11Status() !== "ready"}
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
