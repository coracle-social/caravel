import { useParams, A } from "@solidjs/router"

export default function AdminTenantDetail() {
  const params = useParams()

  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <div class="flex items-center gap-2 mb-6">
        <A href="/admin/tenants" class="text-gray-500 hover:text-gray-700">← Tenants</A>
      </div>
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Tenant {params.id}</h1>
      <div class="space-y-6">
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold mb-4">Relays</h2>
          <p class="text-gray-500">No relays.</p>
        </section>
        <button class="py-2 px-4 border border-red-300 text-red-600 font-medium rounded-lg hover:bg-red-50 transition-colors">
          Deactivate Tenant
        </button>
      </div>
    </div>
  )
}
