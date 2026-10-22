import { Match, Switch, type JSX } from "solid-js"
import AdminShell, { signOutOfAdmin } from "@/admin/components/AdminShell"
import LoadingState from "@/shared/components/LoadingState"
import Toast from "@/shared/components/Toast"
import Login from "@/shared/views/Login"
import { account } from "@/shared/lib/account"
import { identity } from "@/shared/lib/state"

// The admin dashboard's frame: its own sign-in, then its pages for admins only.
export default function Layout(props: { children?: JSX.Element }) {
  return (
    <>
      <Switch>
        <Match when={identity.loading}>
          <LoadingState message="Loading..." class="min-h-screen bg-zinc-100" />
        </Match>
        <Match when={!account()}>
          <Login />
        </Match>
        <Match when={identity.error}>
          <p class="flex min-h-screen items-center justify-center bg-zinc-100 text-sm text-red-600">
            Couldn't load your account. Try reloading the page.
          </p>
        </Match>
        <Match when={!identity()?.is_admin}>
          <NotAdmin />
        </Match>
        <Match when={true}>
          <AdminShell>{props.children}</AdminShell>
        </Match>
      </Switch>
      <Toast />
    </>
  )
}

function NotAdmin() {
  return (
    <div class="flex min-h-screen items-center justify-center bg-zinc-100 p-6">
      <div class="w-full max-w-sm rounded-2xl border border-zinc-200 bg-white p-6 text-center shadow-sm">
        <h1 class="text-lg font-semibold text-zinc-900">Not an admin</h1>
        <p class="mt-2 text-sm text-zinc-600">The account you're signed in with can't use the admin dashboard.</p>
        <button
          type="button"
          onClick={signOutOfAdmin}
          class="mt-5 rounded-lg border border-zinc-300 px-4 py-2 text-sm text-zinc-700 hover:bg-zinc-50"
        >
          Sign out
        </button>
      </div>
    </div>
  )
}
