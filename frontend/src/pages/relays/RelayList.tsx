import { A } from "@solidjs/router"

export default function RelayList() {
  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center justify-between mb-6">
        <h1 class="text-2xl font-bold text-gray-900">My Relays</h1>
        <A
          href="/relays/new"
          class="py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 transition-colors"
        >
          Add Relay
        </A>
      </div>
      <p class="text-gray-500">No relays yet. Create one to get started.</p>
    </div>
  )
}
