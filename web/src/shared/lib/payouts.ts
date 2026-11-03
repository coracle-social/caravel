import type { PayoutStatus } from "@/shared/lib/api"

// What Stripe still needs before a tenant can be paid, in a sentence.
export function payoutReason(status: PayoutStatus | undefined): string | undefined {
  if (!status || status.ready) return undefined

  switch (status.reason) {
    case "requirements.past_due":
      return "Stripe needs more information from you before it can pay you."
    case "requirements.pending_verification":
      return "Stripe is verifying your information."
    case "under_review":
    case "listed":
      return "Stripe is reviewing your account."
    case "rejected.fraud":
    case "rejected.listed":
    case "rejected.other":
    case "rejected.terms_of_service":
      return "Stripe declined this account, so it can't be paid."
    case "other":
      return "Stripe needs something else from you before it can pay you."
    default:
      return status.reason ? `Stripe reports: ${status.reason}` : undefined
  }
}
