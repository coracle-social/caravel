import { For, Show, createSignal } from "solid-js"
import Modal from "@/shared/components/Modal"
import { exportRelayData, importRelayData, type ImportResult, type Relay } from "@/shared/lib/api"

type Props = {
  open: boolean
  onClose: () => void
  relay: () => Relay
}

// Export and import a relay's events as JSONL: the same format the hosting
// backend's own dump tools read, so a relay can be backed up, moved between
// hosts, or seeded from another one.
export default function DataTransferModal(props: Props) {
  const [exporting, setExporting] = createSignal(false)
  const [importing, setImporting] = createSignal(false)
  const [file, setFile] = createSignal<File>()
  const [result, setResult] = createSignal<ImportResult>()
  const [error, setError] = createSignal<string>()

  const relayLabel = () => props.relay().info_name || props.relay().subdomain
  const problems = () => result()?.problems ?? []

  function close() {
    setFile(undefined)
    setResult(undefined)
    setError(undefined)
    props.onClose()
  }

  async function handleExport() {
    setError(undefined)
    setExporting(true)

    try {
      const blob = await exportRelayData(props.relay().id)
      const url = URL.createObjectURL(blob)
      const link = document.createElement("a")

      link.href = url
      link.download = `${props.relay().subdomain}.jsonl`
      link.click()

      // Revoking synchronously can cancel the download the click just started.
      window.setTimeout(() => URL.revokeObjectURL(url), 1000)
    } catch (e) {
      setError(e instanceof Error ? e.message : "Export failed")
    } finally {
      setExporting(false)
    }
  }

  async function handleImport() {
    const selected = file()
    if (!selected) return

    setError(undefined)
    setResult(undefined)
    setImporting(true)

    try {
      setResult(await importRelayData(props.relay().id, selected))
    } catch (e) {
      setError(e instanceof Error ? e.message : "Import failed")
    } finally {
      setImporting(false)
    }
  }

  return (
    <Modal
      open={props.open}
      onClose={close}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      panelClass="w-full max-w-lg bg-white rounded-2xl shadow-xl p-6 space-y-5"
    >
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-semibold text-gray-900">Relay data</h2>
        <button
          type="button"
          onClick={close}
          class="text-gray-400 hover:text-gray-600 text-xl leading-none"
          aria-label="Close"
        >
          ×
        </button>
      </div>

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-gray-700">Export</h3>
        <p class="text-xs text-gray-500">
          Downloads every event {relayLabel()} holds as JSONL, one event per line. Uploaded media
          is not included.
        </p>
        <button
          type="button"
          onClick={handleExport}
          disabled={exporting()}
          class="inline-flex items-center rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
        >
          {exporting() ? "Preparing…" : "Download events"}
        </button>
      </div>

      <hr class="border-gray-200" />

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-gray-700">Import</h3>
        <p class="text-xs text-gray-500">
          Adds the events in a JSONL file to {relayLabel()}. Each event keeps its own signature, so
          a tampered or unsigned line is reported and skipped. Events already on the relay are left
          as they are, so importing the same file twice is safe.
        </p>
        <input
          type="file"
          accept=".jsonl,.json,.ndjson,application/x-ndjson,application/json"
          onChange={(e) => setFile(e.currentTarget.files?.[0] ?? undefined)}
          class="block w-full text-sm text-gray-700 file:mr-3 file:rounded-lg file:border file:border-gray-300 file:bg-white file:px-3 file:py-1.5 file:text-sm file:font-medium file:text-gray-700 hover:file:bg-gray-50"
        />
        <button
          type="button"
          onClick={handleImport}
          disabled={importing() || !file()}
          class="inline-flex items-center rounded-lg border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
        >
          {importing() ? "Importing…" : "Import events"}
        </button>
      </div>

      <Show when={result()}>
        {(r) => (
          <div class="rounded-lg border border-gray-200 bg-gray-50 px-4 py-3 space-y-2">
            <p class="text-sm text-gray-800">
              Imported {r().imported} event{r().imported === 1 ? "" : "s"}
              {r().invalid > 0 ? `, skipped ${r().invalid}` : ""}.
            </p>
            <Show when={problems().length}>
              <ul class="space-y-1 font-mono text-xs text-gray-600">
                <For each={problems().slice(0, 5)}>
                  {problem => (
                    <li class="break-all">
                      {problem.line ? `line ${problem.line}: ` : ""}
                      {problem.message}
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </div>
        )}
      </Show>

      <Show when={error()}>
        <p class="text-sm text-red-600">{error()}</p>
      </Show>
    </Modal>
  )
}
