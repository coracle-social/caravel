export default function AdminTenants() {
  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Tenants</h1>
      <input
        type="search"
        placeholder="Search tenants..."
        class="w-full border border-gray-300 rounded-lg px-3 py-2 mb-6 focus:outline-none focus:ring-2 focus:ring-blue-500"
      />
      <p class="text-gray-500">No tenants found.</p>
    </div>
  )
}
