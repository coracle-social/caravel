import { createSignal } from "solid-js"
import { ensureInvoiceBolt11, invoiceStatus, listInvoiceItems, type Invoice, type InvoiceItem } from "@/shared/lib/api"
import { methodLabel } from "@/shared/lib/paymentMethod"
import { formatDate, formatUsd } from "@/shared/lib/format"
import { PLATFORM_NAME } from "@/shared/lib/config"

function escapeHtml(value: string) {
  const map: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }
  return value.replace(/[&<>"']/g, (c) => map[c])
}

// Printing an invoice from an off-screen frame, with its price in sats when it could be paid over Lightning.
export function useInvoicePdf() {
  const [printing, setPrinting] = createSignal(false)

  // The draft invoice isn't stored, so it passes `loadItems` to fetch its items.
  async function printInvoice(invoice: Invoice, loadItems?: () => Promise<InvoiceItem[]>) {
    if (printing()) return
    setPrinting(true)
    try {
      const fetchItems = loadItems ?? (() => listInvoiceItems(invoice.id))
      const items = await fetchItems().catch(e => {
        console.error("Failed to load invoice line items", e)
        return [] as InvoiceItem[]
      })

      // The draft isn't payable yet, so it has no Lightning invoice to price it.
      let sats: number | undefined
      if (invoice.id !== "draft" && invoice.method !== "stripe" && invoice.method !== "comp" && invoice.voided_at == null) {
        try {
          const bolt11 = await ensureInvoiceBolt11(invoice.id)
          sats = Math.round(bolt11.msats / 1000)
        } catch {
          // Without a bolt11 there is no sats line.
        }
      }

      printHtml(buildHtml({ invoice, items, sats }))
    } finally {
      setPrinting(false)
    }
  }

  return { printInvoice, printing }
}

function buildHtml(opts: { invoice: Invoice; items: InvoiceItem[]; sats?: number }): string {
  const { invoice, items, sats } = opts
  // The draft has no lifecycle timestamps, which would read as open.
  const status = invoice.id === "draft" ? "draft" : invoiceStatus(invoice)

  const rows = items.length
    ? items
        .map((i) => `<tr><td>${escapeHtml(i.description || "Charge")}</td><td class="amt">${formatUsd(i.amount)}</td></tr>`)
        .join("")
    : `<tr><td>Relay subscription</td><td class="amt">${formatUsd(invoice.amount)}</td></tr>`

  const satsRow = sats != null ? `<tr><td>Bitcoin equivalent</td><td class="amt">${sats.toLocaleString()} sats</td></tr>` : ""
  const methodLine = invoice.method ? `<div>Paid via ${escapeHtml(methodLabel(invoice.method))}</div>` : ""

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
  </style></head>
  <body>
    <div class="head">
      <div><h1>${escapeHtml(PLATFORM_NAME)}</h1><div class="muted">Invoice</div></div>
      <span class="badge">${status}</span>
    </div>
    <div class="meta">
      <div>Invoice ID: ${escapeHtml(invoice.id)}</div>
      <div>Billed to: ${escapeHtml(invoice.tenant_pubkey)}</div>
      <div>Issued: ${formatDate(invoice.created_at)}</div>
      <div>Period: ${formatDate(invoice.period_start)} &ndash; ${formatDate(invoice.period_end)}</div>
      ${methodLine}
    </div>
    <table>
      <thead><tr><th>Description</th><th class="amt">Amount</th></tr></thead>
      <tbody>${rows}${satsRow}</tbody>
      <tfoot><tr><td>Total</td><td class="amt">${formatUsd(invoice.amount)}</td></tr></tfoot>
    </table>
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

  // Let the iframe lay out before printing.
  window.setTimeout(() => {
    win.focus()
    win.print()
    window.setTimeout(cleanup, 60000)
  }, 150)
}
