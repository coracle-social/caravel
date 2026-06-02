import { createSignal } from "solid-js"
import QRCode from "qrcode"
import { getInvoiceBolt11, invoiceStatus, listInvoiceItems, type Invoice, type InvoiceItem } from "@/lib/api"
import { methodLabel } from "@/lib/paymentMethod"
import { formatUsd } from "@/lib/format"
import { PLATFORM_NAME } from "@/lib/state"

const fmtDate = (seconds: number) => new Date(seconds * 1000).toLocaleDateString()

function escapeHtml(value: string) {
  const map: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }
  return value.replace(/[&<>"']/g, (c) => map[c])
}

// Generates a printable invoice and opens the browser's print/save-as-PDF dialog.
// No PDF dependency: the invoice is rendered as standalone HTML into an off-screen
// iframe so the current page is never disturbed. The bitcoin line is included only
// for Lightning-relevant invoices (never card-paid or void) to avoid spuriously
// minting a bolt11.
export function useInvoicePdf() {
  const [printing, setPrinting] = createSignal(false)

  // `loadItems` overrides how line items are fetched — used by the draft invoice,
  // whose sentinel id has no row in the per-invoice items endpoint.
  async function printInvoice(invoice: Invoice, loadItems?: () => Promise<InvoiceItem[]>) {
    if (printing()) return
    setPrinting(true)
    try {
      const fetchItems = loadItems ?? (() => listInvoiceItems(invoice.id))
      const items = await fetchItems().catch(e => {
        console.error("Failed to load invoice line items", e)
        return [] as InvoiceItem[]
      })

      let sats: number | undefined
      let qrDataUrl: string | undefined
      if (invoice.method !== "stripe" && invoice.voided_at == null) {
        try {
          const bolt11 = await getInvoiceBolt11(invoice.id)
          sats = Math.round(bolt11.msats / 1000)
          if (invoice.paid_at == null) {
            qrDataUrl = await QRCode.toDataURL(bolt11.lnbc, { width: 180, margin: 1 })
          }
        } catch {
          // no bolt11 available — omit the bitcoin line
        }
      }

      printHtml(buildHtml({ invoice, items, sats, qrDataUrl }))
    } finally {
      setPrinting(false)
    }
  }

  return { printInvoice, printing }
}

function buildHtml(opts: { invoice: Invoice; items: InvoiceItem[]; sats?: number; qrDataUrl?: string }): string {
  const { invoice, items, sats, qrDataUrl } = opts
  // The draft invoice carries the sentinel id and no lifecycle timestamps, so
  // invoiceStatus would read it as "open" — label it "draft" explicitly.
  const status = invoice.id === "draft" ? "draft" : invoiceStatus(invoice)

  const rows = items.length
    ? items
        .map((i) => `<tr><td>${escapeHtml(i.description || "Charge")}</td><td class="amt">${formatUsd(i.amount)}</td></tr>`)
        .join("")
    : `<tr><td>Relay subscription</td><td class="amt">${formatUsd(invoice.amount)}</td></tr>`

  const satsRow = sats != null ? `<tr><td>Bitcoin equivalent</td><td class="amt">${sats.toLocaleString()} sats</td></tr>` : ""
  const methodLine = invoice.method ? `<div>Paid via ${escapeHtml(methodLabel(invoice.method))}</div>` : ""
  const qr = qrDataUrl
    ? `<div class="qr"><img src="${qrDataUrl}" alt="Lightning invoice QR"/><div class="muted">Scan to pay by Lightning</div></div>`
    : ""

  return `<!doctype html><html><head><meta charset="utf-8"><title>Invoice ${escapeHtml(invoice.id)}</title>
  <style>
    * { font-family: ui-sans-serif, system-ui, -apple-system, sans-serif; color: #111827; box-sizing: border-box; }
    body { margin: 40px; }
    h1 { font-size: 20px; margin: 0 0 4px; }
    .muted { color: #6b7280; font-size: 12px; }
    .head { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 24px; }
    .badge { text-transform: capitalize; border: 1px solid #d1d5db; border-radius: 9999px; padding: 2px 10px; font-size: 12px; }
    .meta { font-size: 13px; color: #374151; line-height: 1.6; word-break: break-all; }
    table { width: 100%; border-collapse: collapse; margin-top: 20px; }
    th, td { text-align: left; padding: 8px 0; border-bottom: 1px solid #e5e7eb; font-size: 14px; }
    .amt { text-align: right; white-space: nowrap; }
    tfoot td { font-weight: 600; border-bottom: none; border-top: 2px solid #111827; }
    .qr { margin-top: 28px; text-align: center; }
  </style></head>
  <body>
    <div class="head">
      <div><h1>${escapeHtml(PLATFORM_NAME)}</h1><div class="muted">Invoice</div></div>
      <span class="badge">${status}</span>
    </div>
    <div class="meta">
      <div>Invoice ID: ${escapeHtml(invoice.id)}</div>
      <div>Billed to: ${escapeHtml(invoice.tenant_pubkey)}</div>
      <div>Issued: ${fmtDate(invoice.created_at)}</div>
      <div>Period: ${fmtDate(invoice.period_start)} &ndash; ${fmtDate(invoice.period_end)}</div>
      ${methodLine}
    </div>
    <table>
      <thead><tr><th>Description</th><th class="amt">Amount</th></tr></thead>
      <tbody>${rows}${satsRow}</tbody>
      <tfoot><tr><td>Total</td><td class="amt">${formatUsd(invoice.amount)}</td></tr></tfoot>
    </table>
    ${qr}
  </body></html>`
}

function printHtml(html: string) {
  const iframe = document.createElement("iframe")
  iframe.style.position = "fixed"
  iframe.style.right = "0"
  iframe.style.bottom = "0"
  iframe.style.width = "0"
  iframe.style.height = "0"
  iframe.style.border = "0"
  document.body.appendChild(iframe)

  const win = iframe.contentWindow
  const doc = win?.document
  if (!win || !doc) {
    iframe.remove()
    return
  }

  doc.open()
  doc.write(html)
  doc.close()

  const cleanup = () => window.setTimeout(() => iframe.remove(), 1000)
  win.onafterprint = cleanup

  // Let the iframe lay out (and decode the QR image) before printing.
  window.setTimeout(() => {
    win.focus()
    win.print()
    window.setTimeout(cleanup, 60000)
  }, 150)
}
