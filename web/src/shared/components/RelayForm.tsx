import { createEffect, createMemo, createSignal, For, Show } from "solid-js"
import type { Relay } from "@/shared/lib/api"
import { slugify } from "@/shared/lib/slugify"
import { FREE, memberLabel, priceLabel } from "@/shared/lib/plans"
import { RELAY_DOMAIN } from "@/shared/lib/config"
import { formatError } from "@/shared/lib/format"
import { validateSubdomainLabel } from "@/shared/lib/subdomain"
import { plans, setToastMessage } from "@/shared/lib/state"

export type RelayFormValues = Pick<Relay, "info_name" | "subdomain" | "info_icon" | "info_description" | "plan_id">

type RelayFormProps = {
  initialValues?: Partial<RelayFormValues>
  // The domain after the subdomain: the relay's own, or for a new relay, this dashboard's.
  domain?: string
  syncSubdomainWithName?: boolean
  showPlanSelector?: boolean
  onSubmit: (values: RelayFormValues) => Promise<void> | void
  submitLabel: string
  submittingLabel: string
}

export default function RelayForm(props: RelayFormProps) {
  const showPlanSelector = () => props.showPlanSelector ?? true
  const domain = () => props.domain ?? RELAY_DOMAIN
  const defaultPlanId = createMemo(() => props.initialValues?.plan_id ?? plans()[0]?.id ?? FREE)
  const [planId, setPlanId] = createSignal(defaultPlanId())
  const [name, setName] = createSignal(props.initialValues?.info_name ?? "")
  const [subdomain, setSubdomain] = createSignal(props.initialValues?.subdomain ?? "")
  const [icon, setIcon] = createSignal(props.initialValues?.info_icon ?? "")
  const [description, setDescription] = createSignal(props.initialValues?.info_description ?? "")
  const [submitting, setSubmitting] = createSignal(false)

  async function handleSubmit(e: Event) {
    e.preventDefault()

    if (showPlanSelector() && !planId()) {
      setToastMessage("Please select a plan")
      return
    }

    const subdomainError = validateSubdomainLabel(subdomain())
    if (subdomainError) {
      setToastMessage(subdomainError)
      return
    }

    setToastMessage("")
    setSubmitting(true)

    try {
      await props.onSubmit({
        plan_id: planId(),
        info_name: name(),
        subdomain: subdomain(),
        info_icon: icon(),
        info_description: description(),
      })
    } catch (e) {
      setToastMessage(formatError(e, "Failed to save relay"))
    } finally {
      setSubmitting(false)
    }
  }

  createEffect(() => setPlanId(defaultPlanId()))

  createEffect(() => {
    if (props.syncSubdomainWithName) {
      setSubdomain(slugify(name()))
    }
  })

  return (
    <form onSubmit={handleSubmit} class="space-y-6">
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
        <input
          required
          value={name()}
          onInput={e => setName(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Subdomain</label>
        <div class="flex items-center border border-gray-300 rounded-lg overflow-hidden">
          <input
            value={subdomain()}
            onInput={e => setSubdomain(e.currentTarget.value)}
            required
            class="flex-1 px-3 py-2"
          />
          <span class="px-3 py-2 bg-gray-50 text-gray-500 text-sm border-l border-gray-300">.{domain()}</span>
        </div>
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Icon URL</label>
        <input
          type="url"
          value={icon()}
          onInput={e => setIcon(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Description</label>
        <textarea
          rows={3}
          value={description()}
          onInput={e => setDescription(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <Show when={showPlanSelector()}>
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-3">Plan</label>
          <div class="grid grid-cols-3 gap-3">
            <For each={plans()}>
              {(p) => (
                <button
                  type="button"
                  onClick={() => setPlanId(p.id)}
                  class={`border-2 rounded-xl p-4 text-left transition-colors ${planId() === p.id ? "border-blue-600 bg-blue-50" : "border-gray-200 hover:border-gray-300"}`}
                >
                  <div class="font-bold text-gray-900">{p.name}</div>
                  <div class="text-sm text-gray-500 mt-1">
                    {priceLabel(p.amount)}{p.amount === 0 ? "" : "/mo"}
                  </div>
                  <div class="text-xs text-gray-500 mt-2">
                    {memberLabel(p.members)}
                  </div>
                  <Show when={p.blossom}>
                    <div class="text-xs text-gray-500 mt-1">
                      {p.storage_gb} GB storage, {p.av_hours} A/V hrs
                    </div>
                  </Show>
                </button>
              )}
            </For>
          </div>
        </div>
      </Show>
      <button
        type="submit"
        disabled={submitting()}
        class="w-full py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50"
      >
        {submitting() ? props.submittingLabel : props.submitLabel}
      </button>
    </form>
  )
}
