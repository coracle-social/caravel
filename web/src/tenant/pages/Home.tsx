import { A, useNavigate } from "@solidjs/router"
import { createSignal, For, Show } from "solid-js"
import CheckIcon from "@/shared/components/CheckIcon"
import ExternalLinkIcon from "@/shared/components/ExternalLinkIcon"
import PricingTable from "@/shared/components/PricingTable"
import RelayForm, { type RelayFormValues } from "@/shared/components/RelayForm"
import Modal from "@/shared/components/Modal"
import PostPaidPrompts, { usePostPaidFlow } from "@/shared/components/PostPaidPrompts"
import Login from "@/shared/views/Login"
import { account } from "@/shared/lib/account"
import type { PlanId } from "@/shared/lib/api"
import { PLATFORM_LOGO, PLATFORM_NAME } from "@/shared/lib/config"
import { formatError } from "@/shared/lib/format"
import { FREE } from "@/shared/lib/plans"
import { createRelayForActiveTenant } from "@/shared/lib/relays"
import { refetchBilling, setToastMessage } from "@/shared/lib/state"
import FlotillaLogo from "@/shared/assets/flotilla-logo.svg"
import NostordLogo from "@/shared/assets/nostord-logo.svg"
import ChachiLogo from "@/shared/assets/chachi-logo.svg"

// The Nostr clients a relay's members can use, with the accent each card takes.
const CLIENTS = [
  {
    name: "Flotilla",
    url: "https://flotilla.social",
    host: "flotilla.social",
    logo: FlotillaLogo,
    border: "hover:border-blue-300",
    shadow: "shadow-blue-200",
    title: "group-hover:text-blue-600",
    icon: "group-hover:text-blue-400",
    link: "text-blue-600",
    description: "A community platform built on Nostr. Flotilla gives your members channels, threads, and a rich social experience — all connected to your relay.",
    features: ["Nostr-native channels & threads", "Works directly with your relay", "Open source & self-sovereign"],
  },
  {
    name: "Nostrord",
    url: "https://nostrord.com/",
    host: "nostrord.com",
    logo: NostordLogo,
    border: "hover:border-amber-300",
    shadow: "shadow-amber-200",
    title: "group-hover:text-amber-600",
    icon: "group-hover:text-amber-400",
    link: "text-amber-600",
    description: "A NIP-29 client built for decentralized group chat on Nostr. Create censorship-resistant communities with admin roles, moderation, and access control—all powered by your relay.",
    features: ["Decentralized group chat with NIP-29", "Censorship-resistant communities", "Admin roles & moderation"],
  },
  {
    name: "Chachi",
    url: "https://chachi.chat",
    host: "chachi.chat",
    logo: ChachiLogo,
    border: "hover:border-purple-300",
    shadow: "shadow-purple-200",
    title: "group-hover:text-purple-600",
    icon: "group-hover:text-purple-400",
    link: "text-purple-600",
    description: "A group chat app built on top of Nostr. Chachi makes it easy for your community to have real-time conversations, all flowing through your own relay.",
    features: ["Real-time group messaging", "Bring your own relay", "No accounts — just your Nostr key"],
  },
]

// The page a tenant lands on: what the platform is, who connects to a relay, and
// the relay's creation flow, which sends someone who isn't signed in to sign in
// first and creates the relay once they are.
export default function Home() {
  const navigate = useNavigate()
  const [showRelayModal, setShowRelayModal] = createSignal(false)
  const [showLoginModal, setShowLoginModal] = createSignal(false)
  const [draftRelay, setDraftRelay] = createSignal<RelayFormValues>()
  const [initialPlanId, setInitialPlanId] = createSignal<PlanId>(FREE)
  const postPaid = usePostPaidFlow()
  let createdRelayId = ""

  function openRelayModal(planId: PlanId = FREE) {
    setInitialPlanId(planId)
    setShowRelayModal(true)
  }

  async function onRelayFormSubmit(values: RelayFormValues) {
    // Someone signed out signs in first, and the relay is created once they have.
    if (!account()) {
      setDraftRelay(values)
      setShowRelayModal(false)
      setShowLoginModal(true)
      return
    }

    const relay = await createRelayForActiveTenant(values)
    createdRelayId = relay.id
    setShowRelayModal(false)

    if (values.plan_id !== FREE) {
      void refetchBilling()
      if (await postPaid.start()) return
    }

    navigate(`/relays/${relay.id}`)
  }

  async function onAuthenticated() {
    setShowLoginModal(false)
    const relay = draftRelay()
    setDraftRelay(undefined)

    if (!relay) {
      navigate("/relays")
      return
    }

    try {
      await onRelayFormSubmit(relay)
    } catch (e) {
      setToastMessage(formatError(e, "Failed to create relay"))
    }
  }

  return (
    <div class="min-h-screen bg-white text-gray-900 overflow-x-hidden">
      <Nav onSignIn={() => setShowLoginModal(true)} />
      <Hero onCreateRelay={() => openRelayModal()} onSignIn={() => setShowLoginModal(true)} />
      <Features />
      <Clients />
      <Pricing onChoose={openRelayModal} />
      <ClosingCta onCreateRelay={() => openRelayModal()} />
      <Footer />

      <Modal
        open={showRelayModal()}
        onClose={() => setShowRelayModal(false)}
        panelClass="w-full max-w-2xl rounded-2xl bg-white p-6 sm:p-8 shadow-2xl"
      >
        <div class="mb-6 flex items-start justify-between gap-4">
          <div>
            <h3 class="text-xl font-bold text-gray-900">Create your relay</h3>
            <p class="mt-1 text-sm text-gray-500">Start with a free relay. You can upgrade later.</p>
          </div>
          <button
            type="button"
            class="rounded-md p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700"
            onClick={() => setShowRelayModal(false)}
            aria-label="Close"
          >
            <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12" /></svg>
          </button>
        </div>

        <RelayForm
          syncSubdomainWithName
          initialValues={{ plan_id: initialPlanId() }}
          onSubmit={onRelayFormSubmit}
          submitLabel="Continue"
          submittingLabel="Creating..."
        />
      </Modal>

      <Modal
        open={showLoginModal()}
        onClose={() => setShowLoginModal(false)}
        wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
        panelClass="w-full max-w-3xl mx-4 rounded-2xl"
      >
        <Login
          inModal
          onClose={() => setShowLoginModal(false)}
          onAuthenticated={onAuthenticated}
        />
      </Modal>

      <PostPaidPrompts flow={postPaid} onDone={() => navigate(`/relays/${createdRelayId}`)} />
    </div>
  )
}

function Nav(props: { onSignIn: () => void }) {
  return (
    <nav class="sticky top-0 z-20 bg-white/80 backdrop-blur border-b border-gray-100">
      <div class="max-w-5xl mx-auto px-6 h-14 flex items-center justify-between">
        <div class="flex items-center gap-2 font-bold text-gray-900">
          <img src={PLATFORM_LOGO} alt={PLATFORM_NAME} class="w-7 h-7 rounded" />
          {PLATFORM_NAME}
        </div>
        <Show
          when={account()}
          fallback={
            <button
              type="button"
              onClick={() => props.onSignIn()}
              class="text-sm font-medium py-1.5 px-4 border border-gray-200 rounded-lg text-gray-600 hover:bg-gray-50 transition-colors"
            >
              Sign in
            </button>
          }
        >
          <A
            href="/relays"
            class="text-sm font-medium py-1.5 px-4 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
          >
            Go to dashboard
          </A>
        </Show>
      </div>
    </nav>
  )
}

function Hero(props: { onCreateRelay: () => void; onSignIn: () => void }) {
  return (
    <section class="relative overflow-hidden">
      <div class="absolute inset-0 bg-[radial-gradient(ellipse_80%_50%_at_50%_-10%,rgba(59,130,246,0.12),transparent)]" />
      <div class="absolute top-20 left-1/2 -translate-x-1/2 w-[600px] h-[600px] bg-blue-50 rounded-full blur-3xl opacity-60 pointer-events-none" />

      <div class="relative max-w-5xl mx-auto px-6 pt-24 pb-28 text-center">
        <div class="inline-flex items-center gap-2 rounded-full border border-blue-200 bg-blue-50 px-4 py-1.5 text-xs font-semibold text-blue-700 mb-8">
          <span class="w-1.5 h-1.5 bg-blue-500 rounded-full animate-pulse" />
          Nostr-native relay hosting
        </div>

        <h1 class="text-5xl sm:text-6xl font-extrabold tracking-tight text-gray-900 mb-6 leading-tight">
          Your community,<br />
          <span class="text-blue-600">your server.</span>
        </h1>

        <p class="text-xl text-gray-500 mb-10 max-w-2xl mx-auto leading-relaxed">
          Spin up a private, managed Nostr relay for your community in minutes,
          with full control over membership, access, and policies.
        </p>

        <div class="flex flex-col sm:flex-row items-center justify-center gap-4">
          <button
            type="button"
            onClick={() => props.onCreateRelay()}
            class="inline-flex items-center gap-2 py-3 px-8 bg-blue-600 text-white font-semibold rounded-xl hover:bg-blue-700 active:scale-95 transition-all shadow-lg shadow-blue-200"
          >
            Get started free
            <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7" /></svg>
          </button>
          <A
            href={account() ? "/relays" : "#"}
            onClick={(e) => {
              if (!account()) {
                e.preventDefault()
                props.onSignIn()
              }
            }}
            class="inline-flex items-center gap-2 py-3 px-8 border border-gray-200 text-gray-700 font-semibold rounded-xl hover:bg-gray-50 transition-all"
          >
            {account() ? "Go to dashboard" : "Sign in"}
          </A>
        </div>
      </div>
    </section>
  )
}

function Features() {
  return (
    <section class="bg-gray-50 border-y border-gray-100">
      <div class="max-w-5xl mx-auto px-6 py-20">
        <h2 class="text-3xl font-bold text-center text-gray-900 mb-4">Everything you need</h2>
        <p class="text-center text-gray-500 mb-14 max-w-xl mx-auto">
          {PLATFORM_NAME} takes care of the infrastructure so you can focus on building your community.
        </p>
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
          {[
            {
              icon: (
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="2" y="3" width="20" height="14" rx="2" />
                  <path d="M8 21h8M12 17v4" />
                </svg>
              ),
              title: "Managed hosting",
              body: "We handle uptime, backups, and updates. Your relay stays online so your community never misses a beat.",
            },
            {
              icon: (
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                  <circle cx="12" cy="8" r="4" />
                  <path d="M6 20v-2a6 6 0 0112 0v2" />
                </svg>
              ),
              title: "Membership control",
              body: "Approve members with Nostr pubkeys. Keep your space invite-only or open — you decide.",
            },
            {
              icon: (
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                </svg>
              ),
              title: "Access policies",
              body: "Fine-grained write and read permissions. Moderate content without touching any server config.",
            },
            {
              icon: (
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                  <ellipse cx="12" cy="5" rx="9" ry="3" />
                  <path d="M21 12c0 1.66-4.03 3-9 3S3 13.66 3 12" />
                  <path d="M3 5v14c0 1.66 4.03 3 9 3s9-1.34 9-3V5" />
                </svg>
              ),
              title: "Blossom storage",
              body: "Attach media files to your relay with integrated Blossom server support on paid plans.",
            },
            {
              icon: (
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                  <polygon points="23 7 16 12 23 17 23 7" />
                  <rect x="1" y="5" width="15" height="14" rx="2" />
                </svg>
              ),
              title: "LiveKit video",
              body: "Built-in video room support via LiveKit. Host voice and video calls directly within your community.",
            },
            {
              icon: (
                <svg class="w-6 h-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z" />
                </svg>
              ),
              title: "Flexible payments",
              body: "Pay with Bitcoin/Lightning or with a card.",
            },
          ].map(({ icon, title, body }) => (
            <div class="bg-white rounded-2xl border border-gray-200 p-6 hover:border-blue-200 hover:shadow-sm transition-all">
              <div class="w-10 h-10 rounded-xl bg-blue-50 text-blue-600 flex items-center justify-center mb-4">
                {icon}
              </div>
              <h3 class="font-semibold text-gray-900 mb-1">{title}</h3>
              <p class="text-sm text-gray-500 leading-relaxed">{body}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

function Clients() {
  return (
    <section class="max-w-5xl mx-auto px-6 py-20">
      <div class="text-center mb-14">
        <h2 class="text-3xl font-bold text-gray-900 mb-4">Connect with your community</h2>
        <p class="text-gray-500 max-w-xl mx-auto">
          Once your relay is live, these Nostr-native platforms let your members connect,
          chat, and collaborate — all powered by your relay.
        </p>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
        <For each={CLIENTS}>
          {(client) => (
            <a
              href={client.url}
              target="_blank"
              rel="noopener noreferrer"
              class={`group flex flex-col gap-5 rounded-2xl border border-gray-200 bg-white p-8 ${client.border} hover:shadow-md transition-all`}
            >
              <div class="flex items-start justify-between gap-4">
                <div class="flex items-center gap-3">
                  <img src={client.logo} alt={client.name} class={`w-12 h-12 rounded-2xl shadow-md ${client.shadow}`} />
                  <div>
                    <h3 class={`text-lg font-bold text-gray-900 ${client.title} transition-colors`}>{client.name}</h3>
                    <p class="text-xs text-gray-400">{client.host}</p>
                  </div>
                </div>
                <span class={`text-gray-300 ${client.icon} transition-colors mt-1`}>
                  <ExternalLinkIcon />
                </span>
              </div>
              <p class="text-sm text-gray-600 leading-relaxed">{client.description}</p>
              <div class="space-y-2">
                <For each={client.features}>
                  {(feature) => (
                    <div class="flex items-start gap-2 text-sm text-gray-600">
                      <CheckIcon />
                      {feature}
                    </div>
                  )}
                </For>
              </div>
              <div class="mt-auto pt-2">
                <span class={`inline-flex items-center gap-1.5 text-sm font-semibold ${client.link}`}>
                  Visit {client.host} <ExternalLinkIcon />
                </span>
              </div>
            </a>
          )}
        </For>
      </div>
    </section>
  )
}

function Pricing(props: { onChoose: (planId: PlanId) => void }) {
  return (
    <section class="bg-gray-50 border-y border-gray-100">
      <div class="max-w-5xl mx-auto px-6 py-20">
        <h2 class="text-3xl font-bold text-center text-gray-900 mb-4">Simple pricing</h2>
        <p class="text-center text-gray-500 mb-14 max-w-lg mx-auto">
          Upgrade or cancel any time.
        </p>

        <PricingTable onCta={props.onChoose} />
      </div>
    </section>
  )
}

function ClosingCta(props: { onCreateRelay: () => void }) {
  return (
    <section class="relative overflow-hidden">
      <div class="absolute inset-0 bg-[radial-gradient(ellipse_80%_80%_at_50%_120%,rgba(59,130,246,0.10),transparent)]" />
      <div class="relative max-w-5xl mx-auto px-6 py-28 text-center">
        <h2 class="text-4xl font-extrabold text-gray-900 mb-4">Ready to launch your relay?</h2>
        <p class="text-gray-500 mb-10 max-w-lg mx-auto text-lg">
          Join communities already running on {PLATFORM_NAME}. Set up in minutes.
        </p>
        <button
          type="button"
          onClick={() => props.onCreateRelay()}
          class="inline-flex items-center gap-2 py-3 px-10 bg-blue-600 text-white font-semibold rounded-xl hover:bg-blue-700 active:scale-95 transition-all shadow-lg shadow-blue-200 text-lg"
        >
          Create your relay
          <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M12 5l7 7-7 7" /></svg>
        </button>
      </div>
    </section>
  )
}

function Footer() {
  return (
    <footer class="border-t border-gray-100 bg-gray-50">
      <div class="max-w-5xl mx-auto px-6 py-8 flex flex-col sm:flex-row items-center justify-between gap-4 text-sm text-gray-400">
        <div class="flex items-center gap-2 font-semibold text-gray-500">
          <img src={PLATFORM_LOGO} alt={PLATFORM_NAME} class="w-5 h-5 rounded" />
          {PLATFORM_NAME}
        </div>
        <p>© {new Date().getFullYear()} {PLATFORM_NAME}. Built on Nostr.</p>
        <div class="flex gap-4">
          <For each={CLIENTS}>
            {(client) => <a href={client.url} target="_blank" rel="noopener noreferrer" class="hover:text-gray-600 transition-colors">{client.name}</a>}
          </For>
        </div>
      </div>
    </footer>
  )
}
