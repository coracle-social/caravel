// Dynamic workflow template for the rebuild-skill skill.
//
// Adapt `facets` to the skill being rebuilt (see SKILL.md Step 2), then run with the Workflow
// tool. Keep the script generic and pass per-run values through `args`:
//
//   Workflow({
//     scriptPath: "<this file, or a copy you edited>",
//     args: {
//       skillPath: "/abs/path/to/skills/<name>",
//       skillName: "<name>",
//       facets: [
//         { key: "code-paths",  prompt: "Trace the billing event-replay code this skill describes ..." },
//         { key: "api-docs",    prompt: "Fetch the Lightning/LNbits docs and verify the payment-cascade claims ..." },
//         // 3–6 facets that together cover the skill's claims and its domain
//       ],
//     },
//   })
//
// Phase shape: Investigate (fan-out) -> Synthesize -> Draft -> Verify (fan-out) -> Correct (conditional).
// The point of the structure: gather ground truth before rewriting, then let independent skeptics
// confirm every claim against its source. Nothing unverifiable survives.

export const meta = {
  name: 'rebuild-skill',
  description: 'Investigate a skill\'s domain from real sources, rewrite it from verified ground truth, and adversarially verify every claim',
  phases: [
    { title: 'Investigate', detail: 'one agent per facet gathers ground truth from code/docs/tools' },
    { title: 'Synthesize', detail: 'fold findings + current skill into a grounded rebuild plan' },
    { title: 'Draft', detail: 'rewrite SKILL.md and resources in place' },
    { title: 'Verify', detail: 'one skeptic per claim tries to refute it against its source' },
    { title: 'Correct', detail: 'fix or cut any refuted claim (only if needed)' },
  ],
}

const { skillPath, skillName, facets } = args

const FINDINGS = {
  type: 'object',
  additionalProperties: false,
  required: ['truths', 'divergences'],
  properties: {
    truths: {
      type: 'array',
      description: 'Ground-truth facts established by reading real sources.',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['fact', 'source'],
        properties: {
          fact: { type: 'string' },
          source: { type: 'string', description: 'file path with line, or URL' },
        },
      },
    },
    divergences: {
      type: 'array',
      description: 'Places where the skill\'s current text is stale, vague, wrong, or missing.',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['kind', 'skillSays', 'realitySays', 'source'],
        properties: {
          kind: { type: 'string', enum: ['stale', 'vague', 'wrong', 'missing'] },
          skillSays: { type: 'string' },
          realitySays: { type: 'string' },
          source: { type: 'string' },
        },
      },
    },
  },
}

const PLAN = {
  type: 'object',
  additionalProperties: false,
  required: ['changes', 'descriptionAdvice'],
  properties: {
    changes: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['action', 'what', 'groundedOn'],
        properties: {
          action: { type: 'string', enum: ['fix', 'add', 'cut', 'restructure'] },
          what: { type: 'string' },
          groundedOn: { type: 'string', description: 'the verified fact + source this change rests on' },
        },
      },
    },
    descriptionAdvice: { type: 'string', description: 'how to tune the frontmatter description for correct triggering' },
  },
}

const DRAFT = {
  type: 'object',
  additionalProperties: false,
  required: ['files', 'claims'],
  properties: {
    files: { type: 'array', items: { type: 'string' }, description: 'absolute paths written' },
    claims: {
      type: 'array',
      description: 'Every technical claim in the rebuilt skill a reviewer should check.',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['id', 'claim', 'evidence'],
        properties: {
          id: { type: 'string' },
          claim: { type: 'string' },
          evidence: { type: 'string', description: 'file path with line, or URL, supporting the claim' },
        },
      },
    },
  },
}

const VERDICT = {
  type: 'object',
  additionalProperties: false,
  required: ['refuted', 'reason'],
  properties: {
    refuted: { type: 'boolean', description: 'true if the claim is unsupported, outdated, or contradicted by its source' },
    reason: { type: 'string' },
    shouldSayInstead: { type: 'string', description: 'the correct statement, if refuted' },
  },
}

phase('Investigate')
// Fan out: each agent gains the expertise for one facet by reading real sources, never guessing.
const findings = (await parallel(
  facets.map(f => () =>
    agent(
      `You are gaining the expertise needed to judge and rewrite the "${skillName}" skill at ${skillPath}.\n` +
        `Facet: ${f.prompt}\n\n` +
        `Read the ACTUAL code, docs, and tools — do not speculate or rely on memory for third-party behavior. ` +
        `Gather ground truth: what is genuinely true, what the skill currently claims, and every place the two diverge ` +
        `(stale, vague, wrong, or missing). Cite a file path (with line) or URL for every fact and every divergence.`,
      { label: `investigate:${f.key}`, phase: 'Investigate', schema: FINDINGS },
    ).then(r => ({ facet: f.key, ...r })),
  ),
)).filter(Boolean)

log(`Investigated ${findings.length}/${facets.length} facets; ${findings.reduce((n, f) => n + f.divergences.length, 0)} divergences found.`)

phase('Synthesize')
// One agent needs ALL findings + the current skill together to plan a coherent rebuild — a real barrier.
const plan = await agent(
  `Read the current skill at ${skillPath} and these investigation findings:\n${JSON.stringify(findings)}\n\n` +
    `Produce a concrete plan to rebuild the skill: what to fix, add, cut, and restructure, with the verified fact ` +
    `(and its source) each change rests on. Apply skill-writing principles: progressive disclosure (lean SKILL.md, ` +
    `detail in references/), explain the WHY instead of stacking MUSTs, and a description tuned for correct triggering. ` +
    `Preserve the directory and the frontmatter \`name\`.`,
  { phase: 'Synthesize', schema: PLAN },
)

phase('Draft')
// Single writer, so no worktree isolation needed. It enumerates the claims it makes for verification.
const draft = await agent(
  `Rebuild the "${skillName}" skill at ${skillPath} per this plan:\n${JSON.stringify(plan)}\n\n` +
    `Edit SKILL.md and any bundled resources in place; preserve the directory and the frontmatter \`name\`. ` +
    `Every technical claim must trace to ground truth from the findings — cite or cut. ` +
    `Return the files you wrote and the full set of factual claims a reviewer should check, each with its supporting evidence.`,
  { phase: 'Draft', schema: DRAFT },
)

phase('Verify')
// Fan out: each claim gets an independent skeptic prompted to refute it against its own source.
const verdicts = (await parallel(
  draft.claims.map(c => () =>
    agent(
      `Adversarially check this claim from the rebuilt "${skillName}" skill: "${c.claim}".\n` +
        `Evidence cited: ${c.evidence}\n\n` +
        `Go read the cited source yourself and TRY TO REFUTE the claim. Default to refuted=true if you cannot ` +
        `independently confirm it from the source. If it is unsupported, outdated, or contradicted, say what the skill should say instead.`,
      { label: `verify:${c.id}`, phase: 'Verify', schema: VERDICT },
    ).then(v => ({ ...c, ...v })),
  ),
)).filter(Boolean)

const broken = verdicts.filter(v => v.refuted)
log(`Verified ${verdicts.length} claims; ${broken.length} refuted.`)

if (broken.length) {
  phase('Correct')
  await agent(
    `These claims in the rebuilt skill at ${skillPath} were refuted during verification:\n${JSON.stringify(broken)}\n\n` +
      `Fix each one in the skill files: correct the statement to match ground truth (use \`shouldSayInstead\`), or cut it. ` +
      `Do not introduce any new unverified claim.`,
    { phase: 'Correct', schema: { type: 'object', additionalProperties: false, required: ['fixed'], properties: { fixed: { type: 'array', items: { type: 'string' } } } } },
  )
}

return { skillPath, filesWritten: draft.files, claimsChecked: verdicts.length, refuted: broken }
