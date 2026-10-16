import type { JSX } from "solid-js"

// The frame every page sits in: one centered column with room to breathe.
export default function Page(props: { children: JSX.Element }) {
  return (
    <div class="min-h-screen bg-zinc-50 text-zinc-900">
      <main class="mx-auto max-w-md px-4 py-12 sm:py-20">
        {props.children}
        <p class="mt-8 text-center text-xs text-zinc-400">Payments are processed securely by Stripe.</p>
      </main>
    </div>
  )
}
