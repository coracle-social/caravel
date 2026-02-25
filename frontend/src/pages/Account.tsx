export default function Account() {
  return (
    <div class="max-w-4xl mx-auto px-4 py-8">
      <h1 class="text-2xl font-bold text-gray-900 mb-6">Account</h1>

      <div class="space-y-6">
        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Account Status</h2>
          <p class="text-gray-500">Status information coming soon.</p>
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Recurring Billing</h2>
          <p class="text-sm text-gray-600 mb-4">
            Enable automatic payments by providing your Nostr Wallet Connect URL.
          </p>
          <input
            type="text"
            placeholder="nostr+walletconnect://..."
            class="w-full border border-gray-300 rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </section>

        <section class="bg-white border border-gray-200 rounded-xl p-6">
          <h2 class="text-lg font-semibold text-gray-900 mb-4">Invoice History</h2>
          <p class="text-gray-500">No invoices yet.</p>
        </section>
      </div>
    </div>
  )
}
