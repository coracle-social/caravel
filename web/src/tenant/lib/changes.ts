import { formatError } from "@/shared/lib/format"
import { setToastMessage } from "@/shared/lib/state"

// Make a change, toasting `success` once it's made or else what went wrong, and say whether it was made.
export async function makeChange(action: () => Promise<unknown>, failure: string, success?: string) {
  try {
    await action()
    if (success) setToastMessage(success, "success")
    return true
  } catch (e) {
    setToastMessage(formatError(e, failure))
    return false
  }
}
