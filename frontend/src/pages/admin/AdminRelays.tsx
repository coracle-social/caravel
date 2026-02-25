export default function AdminRelays() {
  return (
    <div class="max-w-5xl mx-auto px-4 py-8">
      <h1 class="text-2xl font-bold text-gray-900 mb-6">All Relays</h1>
      <input
        type="search"
        placeholder="Search relays..."
        class="w-full border border-gray-300 rounded-lg px-3 py-2 mb-6 focus:outline-none focus:ring-2 focus:ring-blue-500"
      />
      <p class="text-gray-500">No relays found.</p>
    </div>
  )
}
