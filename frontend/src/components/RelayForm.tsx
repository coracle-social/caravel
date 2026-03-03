import type { RelayConfig } from "../lib/api"
import Checkbox from "./Checkbox"

type RelayFormProps = {
  name: string
  setName: (value: string) => void
  subdomain: string
  setSubdomain: (value: string) => void
  icon: string
  setIcon: (value: string) => void
  description: string
  setDescription: (value: string) => void
  plan: string
  setPlan: (value: string) => void
  plans: readonly string[]
  config: RelayConfig
  setConfig: (value: RelayConfig) => void
  onSubmit: (e: Event) => void
  submitting: boolean
  error?: string
  submitLabel: string
  submittingLabel: string
}

export default function RelayForm(props: RelayFormProps) {
  function patchConfig(patch: Partial<RelayConfig>) {
    props.setConfig({ ...props.config, ...patch })
  }

  return (
    <form onSubmit={props.onSubmit} class="space-y-6">
      {/* Basic info */}
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Relay Name</label>
        <input
          value={props.name}
          onInput={e => props.setName(e.currentTarget.value)}
          required
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Subdomain</label>
        <div class="flex items-center border border-gray-300 rounded-lg overflow-hidden">
          <input
            value={props.subdomain}
            onInput={e => props.setSubdomain(e.currentTarget.value)}
            required
            class="flex-1 px-3 py-2"
          />
          <span class="px-3 py-2 bg-gray-50 text-gray-500 text-sm border-l border-gray-300">.spaces.coracle.social</span>
        </div>
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Icon URL</label>
        <input
          type="url"
          value={props.icon}
          onInput={e => props.setIcon(e.currentTarget.value)}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Description</label>
        <textarea
          value={props.description}
          onInput={e => props.setDescription(e.currentTarget.value)}
          rows={3}
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-2">Plan</label>
        <div class="flex gap-2">
          {props.plans.map(p => (
            <button
              type="button"
              class={`rounded-lg border px-3 py-2 text-sm capitalize ${props.plan === p ? "border-blue-600 text-blue-700" : "border-gray-300 text-gray-700"}`}
              onClick={() => props.setPlan(p)}
            >
              {p}
            </button>
          ))}
        </div>
      </div>

      {/* Policy */}
      <fieldset class="border border-gray-200 rounded-lg p-4 space-y-3">
        <legend class="text-sm font-semibold text-gray-700 px-1">Policy</legend>
        <Checkbox
          label="Allow public join (anyone can join without an invite)"
          checked={props.config.policy.public_join}
          onChange={v => patchConfig({ policy: { ...props.config.policy, public_join: v } })}
        />
        <Checkbox
          label="Strip signatures when serving events to non-admins"
          checked={props.config.policy.strip_signatures}
          onChange={v => patchConfig({ policy: { ...props.config.policy, strip_signatures: v } })}
        />
      </fieldset>

      {/* Groups (NIP 29) */}
      <fieldset class="border border-gray-200 rounded-lg p-4 space-y-3">
        <legend class="text-sm font-semibold text-gray-700 px-1">Groups (NIP 29)</legend>
        <Checkbox
          label="Enable NIP 29 groups"
          checked={props.config.groups.enabled}
          onChange={v => patchConfig({ groups: { ...props.config.groups, enabled: v } })}
        />
        <Checkbox
          label="Allow members to join groups without approval"
          checked={props.config.groups.auto_join}
          onChange={v => patchConfig({ groups: { ...props.config.groups, auto_join: v } })}
        />
      </fieldset>

      {/* Management (NIP 86) */}
      <fieldset class="border border-gray-200 rounded-lg p-4 space-y-3">
        <legend class="text-sm font-semibold text-gray-700 px-1">Management (NIP 86)</legend>
        <Checkbox
          label="Enable NIP 86 relay management"
          checked={props.config.management.enabled}
          onChange={v => patchConfig({ management: { enabled: v } })}
        />
      </fieldset>

      {/* Blossom */}
      <fieldset class="border border-gray-200 rounded-lg p-4">
        <legend class="text-sm font-semibold text-gray-700 px-1">Blossom</legend>
        <Checkbox
          label="Enable Blossom media storage"
          checked={props.config.blossom.enabled}
          onChange={v => patchConfig({ blossom: { enabled: v } })}
        />
      </fieldset>

      {/* Push (NIP 9a) */}
      <fieldset class="border border-gray-200 rounded-lg p-4">
        <legend class="text-sm font-semibold text-gray-700 px-1">Push (NIP 9a)</legend>
        <Checkbox
          label="Enable NIP 9a push notifications"
          checked={props.config.push.enabled}
          onChange={v => patchConfig({ push: { enabled: v } })}
        />
      </fieldset>

      {props.error && <p class="text-sm text-red-600">{props.error}</p>}
      <button
        type="submit"
        disabled={props.submitting}
        class="w-full py-2 px-4 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50"
      >
        {props.submitting ? props.submittingLabel : props.submitLabel}
      </button>
    </form>
  )
}
