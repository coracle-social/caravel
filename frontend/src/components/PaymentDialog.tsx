import { createEffect, createSignal, Show } from "solid-js"
import QRCode from "qrcode"
import Modal from "@/components/Modal"
import { getInvoice, type Invoice } from "@/lib/api"

type Tab = "bitcoin" | "card"
type PayStatus = "idle" | "loading" | "success" | "error"

type PaymentDialogProps = {
  invoice: Invoice
  open: boolean
  onClose: () => void
}

export default function PaymentDialog(props: PaymentDialogProps) {
  const [tab, setTab] = createSignal<Tab>("bitcoin")
  const [qrDataUrl, setQrDataUrl] = createSignal("")
  const [payStatus, setPayStatus] = createSignal<PayStatus>("idle")
  const [payError, setPayError] = createSignal("")

  createEffect(async () => {
    const bolt11 = props.invoice?.bolt11
    if (!bolt11) return
    setQrDataUrl(await QRCode.toDataURL(bolt11, { width: 256, margin: 2 }))
  })

  function copyBolt11() {
    void navigator.clipboard.writeText(props.invoice.bolt11)
  }

  async function checkPayment() {
    setPayStatus("loading")
    setPayError("")
    try {
      const invoice = await getInvoice(props.invoice.id)
      if (invoice.status === "paid") {
        setPayStatus("success")
      } else {
        setPayStatus("error")
        setPayError("Payment not yet confirmed. Please try again after sending.")
      }
    } catch (e) {
      setPayStatus("error")
      setPayError(e instanceof Error ? e.message : "Failed to check payment status")
    }
  }

  function handleClose() {
    setPayStatus("idle")
    setPayError("")
    props.onClose()
  }

  const periodLabel = () => {
    const start = new Date(props.invoice.period_start * 1000)
    const end = new Date(props.invoice.period_end * 1000)
    return `${start.toLocaleDateString()} – ${end.toLocaleDateString()}`
  }

  return (
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
            <Show when={props.invoice.amount}>
              <p class="text-2xl font-bold text-gray-900 mt-1">
                {props.invoice.amount.toLocaleString()} <span class="text-base font-normal text-gray-500">sats</span>
              </p>
            </Show>
            <Show when={props.invoice.period_start && props.invoice.period_end}>
              <p class="text-xs text-gray-500 mt-0.5">{periodLabel()}</p>
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

      {/* Pay with label + Tabs */}
      <div class="px-6 pt-4">
        <p class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-2">Pay with</p>
        <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${tab() === "bitcoin" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
            onClick={() => setTab("bitcoin")}
          >
            Bitcoin
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

      {/* Tab content */}
      <div class="px-6 py-4 min-h-[240px] flex flex-col items-center justify-center">
        <Show when={tab() === "bitcoin"}>
          <Show
            when={payStatus() === "success"}
            fallback={
              <div class="w-full space-y-3">
                <Show
                  when={qrDataUrl()}
                  fallback={<div class="flex items-center justify-center py-12 text-sm text-gray-400">Generating invoice...</div>}
                >
                  <img src={qrDataUrl()} alt="Lightning invoice QR code" class="mx-auto rounded-lg" />
                </Show>
                <div class="flex rounded-lg border border-gray-300">
                  <input
                    type="text"
                    readOnly
                    value={props.invoice.bolt11}
                    class="min-w-0 flex-1 rounded-l-lg border-0 px-3 py-2 text-xs text-gray-500 bg-transparent focus:outline-none"
                  />
                  <button
                    type="button"
                    class="flex items-center px-3 text-gray-400 hover:text-gray-700"
                    onClick={copyBolt11}
                    title="Copy invoice"
                  >
                    <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <rect x="9" y="9" width="13" height="13" rx="2" />
                      <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
                    </svg>
                  </button>
                </div>
              </div>
            }
          >
            <div class="text-center">
              <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-green-100">
                <svg class="w-6 h-6 text-green-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </div>
              <p class="text-sm font-medium text-gray-900">Payment confirmed!</p>
              <p class="text-xs text-gray-500 mt-1">Thank you. Your relay is now active.</p>
            </div>
          </Show>
        </Show>

        <Show when={tab() === "card"}>
          <div class="text-center">
            <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-gray-100">
              <svg class="w-6 h-6 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <rect x="1" y="4" width="22" height="16" rx="2" ry="2" />
                <line x1="1" y1="10" x2="23" y2="10" />
              </svg>
            </div>
            <p class="text-sm font-medium text-gray-700">Coming soon</p>
            <p class="text-xs text-gray-500 mt-1">Card payments are not yet available.</p>
          </div>
        </Show>
      </div>

      {/* Error */}
      <Show when={payError()}>
        <div class="px-6 pb-2">
          <p class="text-xs text-red-600">{payError()}</p>
        </div>
      </Show>

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
          <button
            type="button"
            onClick={checkPayment}
            disabled={payStatus() === "loading"}
            class="py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
          >
            {payStatus() === "loading" ? "Checking..." : "Complete Payment"}
          </button>
        </Show>
      </div>
    </Modal>
  )
}
