import { useParams, A } from "@solidjs/router"

export default function RelayDetail() {
  const params = useParams()

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href="/relays" class="text-gray-500 hover:text-gray-700">← Relays</A>
      </div>
      <h1 class="text-2xl font-bold text-gray-900 mb-4">Relay {params.id}</h1>
      <div class="flex gap-3">
        <A
          href={`/relays/${params.id}/edit`}
          class="py-2 px-4 border border-gray-300 text-gray-700 font-medium rounded-lg hover:bg-gray-50 transition-colors"
        >
          Edit
        </A>
        <button class="py-2 px-4 border border-red-300 text-red-600 font-medium rounded-lg hover:bg-red-50 transition-colors">
          Deactivate
        </button>
      </div>
    </div>
  )
}
