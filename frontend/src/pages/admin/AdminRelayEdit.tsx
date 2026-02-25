import { useParams, A } from "@solidjs/router"

export default function AdminRelayEdit() {
  const params = useParams()

  return (
    <div class="max-w-2xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href={`/admin/relays/${params.id}`} class="text-gray-500 hover:text-gray-700">← Back</A>
      </div>
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Edit Relay (Admin)</h1>
      <p class="text-gray-500">Edit form coming soon.</p>
    </div>
  )
}
