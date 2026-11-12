// The feature map's data (features.json) with its types. The map page renders
// only from this; src/feature_map.rs checks it against the config keys and
// experiments the Rust code reads.
import data from "./features.json" with { type: "json" }

export const PLATFORMS = ["mac", "windows", "linux"] as const
export type Platform = (typeof PLATFORMS)[number]

export type Status =
  | { kind: "stable"; version: string }
  | { kind: "main" }
  | { kind: "experiment"; flag: string }
  | { kind: "branch"; branch: string }
  | { kind: "review"; pr: number }

export type Shortcut = { keys: string; does: string }
export type ConfigKey = { key: string; value: string; does: string }

export type Feature = {
  id: string
  group: string
  name: string
  says: string
  status: string
  platforms: Platform[]
  /** When it landed (YYYY-MM-DD); required unless stable. */
  added?: string
  /** Why a platform is missing, or another caveat. */
  note?: string
  /** A branch's experiment flag(s), for features not on main. */
  flag?: string
  pr?: number
  shortcuts?: Shortcut[]
  config?: ConfigKey[]
  /** Repo paths, optionally with a #heading anchor. */
  docs?: string[]
}

export type Group = { id: string; title: string }
export type FlowStep = { title: string; detail: string; group: string }

export const GROUPS: Group[] = data.groups
export const FLOW: FlowStep[] = data.flow
export const FEATURES = data.features as Feature[]

export const STATUS_KINDS = ["stable", "main", "experiment", "branch", "review"] as const

export function parseStatus(status: string): Status {
  if (status === "main") return { kind: "main" }
  const stable = status.match(/^stable@(\d+\.\d+\.\d+)$/)
  if (stable) return { kind: "stable", version: stable[1] }
  const experiment = status.match(/^experiment:([A-Za-z][A-Za-z0-9]*)$/)
  if (experiment) return { kind: "experiment", flag: experiment[1] }
  const branch = status.match(/^branch:([\w./-]+)$/)
  if (branch) return { kind: "branch", branch: branch[1] }
  const review = status.match(/^review:#(\d+)$/)
  if (review) return { kind: "review", pr: Number(review[1]) }
  throw new Error(`unknown status "${status}"`)
}

/** Whether the feature's code is on main (so its config keys and flags must exist there). */
export const onMain = (s: Status) => s.kind === "stable" || s.kind === "main" || s.kind === "experiment"

/** Shape problems that would make the page wrong. The build fails on any. */
export function shapeErrors(features: Feature[] = FEATURES, groups: Group[] = GROUPS): string[] {
  const errors: string[] = []
  const ids = new Set<string>()
  const groupIds = new Set(groups.map((g) => g.id))
  for (const f of features) {
    const at = `features.json "${f.id}"`
    if (!/^[a-z0-9-]+$/.test(f.id ?? "")) errors.push(`${at}: id must be kebab-case`)
    if (ids.has(f.id)) errors.push(`${at}: duplicate id`)
    ids.add(f.id)
    if (!groupIds.has(f.group)) errors.push(`${at}: unknown group "${f.group}"`)
    if (!f.name || !f.says) errors.push(`${at}: needs a name and says`)
    let status: Status | null = null
    try {
      status = parseStatus(f.status)
    } catch (e) {
      errors.push(`${at}: ${(e as Error).message}`)
    }
    if (!f.platforms?.length || f.platforms.some((p) => !PLATFORMS.includes(p))) errors.push(`${at}: platforms must be some of ${PLATFORMS.join(", ")}`)
    if (f.added !== undefined && !/^\d{4}-\d{2}-\d{2}$/.test(f.added)) errors.push(`${at}: added must be YYYY-MM-DD`)
    if (status && status.kind !== "stable" && !f.added) errors.push(`${at}: a ${status.kind} feature needs "added"`)
    if (status?.kind === "review" && f.pr !== status.pr) errors.push(`${at}: pr must match the review status`)
  }
  for (const step of FLOW) if (!groupIds.has(step.group)) errors.push(`features.json flow "${step.title}": unknown group "${step.group}"`)
  return errors
}
