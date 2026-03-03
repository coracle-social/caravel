import { For, Show } from "solid-js"
import type { Relay } from "../lib/api"

function Field(props: { label: string; children: any }) {
  return (
    <div>
      <dt class="text-xs font-medium text-gray-500 uppercase tracking-wide">{props.label}</dt>
      <dd class="mt-0.5 text-sm text-gray-900">{props.children}</dd>
    </div>
  )
}

function Badge(props: { on: boolean; onLabel?: string; offLabel?: string }) {
  const label = () => (props.on ? (props.onLabel ?? "Yes") : (props.offLabel ?? "No"))
  return (
    <span class={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${props.on ? "bg-green-100 text-green-700" : "bg-gray-100 text-gray-500"}`}>
      {label()}
    </span>
  )
}

function Section(props: { title: string; children: any }) {
  return (
    <div>
      <h3 class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">{props.title}</h3>
      <dl class="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-3">
        {props.children}
      </dl>
    </div>
  )
}

type RelayDetailCardProps = {
  relay: Relay
  showTenant?: boolean
}

export default function RelayDetailCard(props: RelayDetailCardProps) {
  const r = () => props.relay
  const cfg = () => r().config

  return (
    <div class="space-y-6">
      {/* Header */}
      <div class="flex items-start gap-4">
        <Show when={r().icon}>
          <img src={r().icon} alt="" class="w-14 h-14 rounded-xl object-cover flex-shrink-0 border border-gray-200" />
        </Show>
        <div class="min-w-0">
          <h1 class="text-2xl font-bold text-gray-900">{r().name}</h1>
          <a
            href={`wss://${r().subdomain}.spaces.coracle.social`}
            class="text-sm text-blue-600 hover:underline break-all"
          >
            wss://{r().subdomain}.spaces.coracle.social
          </a>
          <Show when={r().description.trim()}>
            <p class="mt-2 text-sm text-gray-600">{r().description}</p>
          </Show>
        </div>
      </div>

      <hr class="border-gray-200" />

      {/* Relay info */}
      <Section title="Relay">
        <Field label="Plan">
          <span class="capitalize">{r().plan}</span>
        </Field>
        <Field label="Status">
          <span class={`capitalize font-medium ${r().status === "active" ? "text-green-600" : r().status === "deactivated" ? "text-red-500" : "text-yellow-600"}`}>
            {r().status}
          </span>
        </Field>
        <Field label="Schema">{r().schema}</Field>
        <Show when={props.showTenant}>
          <Field label="Tenant">
            <span class="font-mono text-xs break-all">{r().tenant}</span>
          </Field>
        </Show>
      </Section>

      <hr class="border-gray-200" />

      {/* Config sections */}
      <Show when={cfg()}>
        <Section title="Policy">
          <Field label="Public join">
            <Badge on={cfg().policy.public_join} onLabel="Enabled" offLabel="Disabled" />
          </Field>
          <Field label="Strip signatures">
            <Badge on={cfg().policy.strip_signatures} onLabel="Enabled" offLabel="Disabled" />
          </Field>
        </Section>

        <hr class="border-gray-200" />

        <Section title="Groups (NIP 29)">
          <Field label="Enabled">
            <Badge on={cfg().groups.enabled} />
          </Field>
          <Field label="Auto-join">
            <Badge on={cfg().groups.auto_join} />
          </Field>
        </Section>

        <hr class="border-gray-200" />

        <Section title="Management (NIP 86)">
          <Field label="Enabled">
            <Badge on={cfg().management.enabled} />
          </Field>

        </Section>

        <hr class="border-gray-200" />

        <Section title="Features">
          <Field label="Blossom">
            <Badge on={cfg().blossom.enabled} onLabel="Enabled" offLabel="Disabled" />
          </Field>
          <Field label="Push (NIP 9a)">
            <Badge on={cfg().push.enabled} onLabel="Enabled" offLabel="Disabled" />
          </Field>
        </Section>
      </Show>
    </div>
  )
}
