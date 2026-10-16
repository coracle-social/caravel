import { setToastMessage } from "@/shared/lib/state"

export async function copyToClipboard(
  text: string,
  opts?: { successMessage?: string; errorMessage?: string },
): Promise<boolean> {
  if (!text) return false
  try {
    await navigator.clipboard.writeText(text)
    setToastMessage(opts?.successMessage ?? "Copied to clipboard", "success")
    return true
  } catch {
    setToastMessage(opts?.errorMessage ?? "Couldn't copy to clipboard")
    return false
  }
}
