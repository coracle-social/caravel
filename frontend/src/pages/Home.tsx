import { A } from "@solidjs/router"

export default function Home() {
  return (
    <div class="min-h-screen bg-white">
      {/* Hero */}
      <div class="max-w-4xl mx-auto px-4 py-24 text-center">
        <h1 class="text-2xl font-bold text-gray-900 mb-4 py-4">
          Host Your Own Nostr Community Relay
        </h1>
        <p class="text-xl text-gray-600 mb-8 max-w-2xl mx-auto">
          Spin up a private, managed Nostr relay for your community in minutes.
          Full control over membership, access, and policies.
        </p>
        <A
          href="/relays/new"
          class="inline-block py-3 px-8 bg-blue-600 text-white font-semibold rounded-lg hover:bg-blue-700 transition-colors"
        >
          Get Started
        </A>
      </div>

      {/* Pricing */}
      <div class="max-w-4xl mx-auto px-4 pb-24">
        <h2 class="text-2xl font-bold text-center text-gray-900 mb-12">Pricing</h2>
        <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
          <div class="border border-gray-200 rounded-xl p-6">
            <h3 class="text-xl font-bold mb-2">Free</h3>
            <p class="text-3xl font-bold mb-4">0 <span class="text-sm font-normal text-gray-500">sats/mo</span></p>
            <ul class="space-y-2 text-gray-600">
              <li>Up to 10 members</li>
              <li class="line-through text-gray-400">Blossom storage</li>
              <li class="line-through text-gray-400">LiveKit video</li>
            </ul>
          </div>
          <div class="border-2 border-blue-600 rounded-xl p-6 relative">
            <span class="absolute -top-3 left-1/2 -translate-x-1/2 bg-blue-600 text-white text-xs font-semibold px-3 py-1 rounded-full">Popular</span>
            <h3 class="text-xl font-bold mb-2">Basic</h3>
            <p class="text-3xl font-bold mb-4">10,000 <span class="text-sm font-normal text-gray-500">sats/mo</span></p>
            <ul class="space-y-2 text-gray-600">
              <li>Up to 100 members</li>
              <li>Blossom storage</li>
              <li>LiveKit video</li>
            </ul>
          </div>
          <div class="border border-gray-200 rounded-xl p-6">
            <h3 class="text-xl font-bold mb-2">Growth</h3>
            <p class="text-3xl font-bold mb-4">50,000 <span class="text-sm font-normal text-gray-500">sats/mo</span></p>
            <ul class="space-y-2 text-gray-600">
              <li>Unlimited members</li>
              <li>Blossom storage</li>
              <li>LiveKit video</li>
            </ul>
          </div>
        </div>
      </div>
    </div>
  )
}
