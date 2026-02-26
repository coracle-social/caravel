import { Show, createSignal } from "solid-js"
import { useNavigate } from "@solidjs/router"
import { createTenantRelay } from "../../lib/api"

const PLANS = [
  { id: "free", label: "Free", price: 0, members: "Up to 10", blossom: false, livekit: false },
  { id: "basic", label: "Basic", price: 10_000, members: "Up to 100", blossom: true, livekit: true },
  { id: "growth", label: "Growth", price: 50_000, members: "Unlimited", blossom: true, livekit: true },
] as const

type PlanId = (typeof PLANS)[number]["id"]

function slugify(s: string) {
  return s
    .toLowerCase()
    .trim()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
}

export default function RelayNew() {
  const navigate = useNavigate()
  const [name, setName] = createSignal("")
  const [subdomain, setSubdomain] = createSignal("")
  const [icon, setIcon] = createSignal("")
  const [description, setDescription] = createSignal("")
  const [plan, setPlan] = createSignal<PlanId>("free")
  const [submitting, setSubmitting] = createSignal(false)
  const [subdomainError, setSubdomainError] = createSignal("")

  function handleNameInput(value: string) {
    setName(value)
    setSubdomain(slugify(value))
    setSubdomainError("")
  }

  async function handleSubmit(e: Event) {
    e.preventDefault()

    setSubdomainError("")
    setSubmitting(true)

    try {
      const relay = await createTenantRelay({
        name: name().trim(),
        subdomain: slugify(subdomain()),
        icon: icon().trim(),
        description: description().trim(),
        plan: plan(),
      })
      navigate(`/relays/${relay.id}`)
    } catch (e) {
      setSubdomainError(e instanceof Error ? e.message : "Failed to create relay")
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div class="max-w-2xl mx-auto px-4 py-8">
      <h1 class="text-2xl font-bold text-gray-900 mb-6">New Relay</h1>
      <form onSubmit={handleSubmit} class="space-y-6">
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
          <input
            type="text"
            required
            value={name()}
            onInput={e => handleNameInput(e.currentTarget.value)}
            placeholder="My Community"
            class="w-full border border-gray-300 rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>

        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Subdomain</label>
          <div class="relative">
            <div
              class="flex items-center border rounded-lg overflow-hidden focus-within:ring-2"
              classList={{
                "border-gray-300 focus-within:ring-blue-500": !subdomainError(),
                "border-red-400 focus-within:ring-red-400": !!subdomainError(),
              }}
            >
            <input
              type="text"
              required
              value={subdomain()}
              onInput={e => {
                setSubdomain(e.currentTarget.value)
                setSubdomainError("")
              }}
              placeholder="my-community"
              class="flex-1 px-3 py-2 focus:outline-none"
            />
            <span class="px-3 py-2 bg-gray-50 text-gray-500 text-sm border-l border-gray-300">
              .spaces.coracle.social
            </span>
            </div>

            <Show when={subdomainError()}>
              <div class="pointer-events-none absolute right-0 top-full z-10 mt-2 rounded-md bg-red-600 px-3 py-2 text-xs text-white shadow-lg">
                {subdomainError()}
              </div>
            </Show>
          </div>
        </div>

        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Icon URL</label>
          <input
            type="url"
            value={icon()}
            onInput={e => setIcon(e.currentTarget.value)}
            placeholder="https://example.com/icon.png"
            class="w-full border border-gray-300 rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>

        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Description</label>
          <textarea
            value={description()}
            onInput={e => setDescription(e.currentTarget.value)}
            placeholder="A community for..."
            rows={3}
            class="w-full border border-gray-300 rounded-lg px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>

        <div>
          <label class="block text-sm font-medium text-gray-700 mb-3">Plan</label>
          <div class="grid grid-cols-3 gap-3">
            {PLANS.map(p => (
              <button
                type="button"
                onClick={() => setPlan(p.id)}
                class={`border-2 rounded-xl p-4 text-left transition-colors ${
                  plan() === p.id
                    ? "border-blue-600 bg-blue-50"
                    : "border-gray-200 hover:border-gray-300"
                }`}
              >
                <div class="font-bold text-gray-900">{p.label}</div>
                <div class="text-sm text-gray-500 mt-1">
                  {p.price === 0 ? "Free" : `${p.price.toLocaleString()} sats/mo`}
                </div>
                <div class="text-xs text-gray-500 mt-2">{p.members} members</div>
              </button>
            ))}
          </div>
        </div>

        <button
          type="submit"
          disabled={submitting()}
          class="w-full py-3 bg-blue-600 text-white font-semibold rounded-lg hover:bg-blue-700 transition-colors"
        >
          {submitting() ? "Creating..." : "Create Relay"}
        </button>
      </form>
    </div>
  )
}
