// The hidden feature map, rendered to static HTML at build time from
// features.json. Nothing feature-specific lives here.
import type { Audit } from "../../scripts/feature-audit.ts"
import { FEATURES, FLOW, GROUPS, PLATFORMS, STATUS_KINDS, parseStatus, type Feature, type Platform, type Status } from "../features"

const REPO = "https://github.com/kitlangton/scrawl"

const KIND_LABEL: Record<Status["kind"], string> = {
  stable: "Stable",
  main: "On main",
  experiment: "Experiment",
  branch: "Branch only",
  review: "In review",
}

const KIND_MEANS: Record<Status["kind"], string> = {
  stable: "in a release; the version that first had it",
  main: "merged, not in a release yet",
  experiment: "on main, off until its flag is set",
  branch: "not on main",
  review: "an open pull request",
}

const PLATFORM_LABEL: Record<Platform, string> = { mac: "Mac", windows: "Windows", linux: "Linux" }

const statusOf = (f: Feature) => parseStatus(f.status)

function statusDetail(s: Status): string {
  switch (s.kind) {
    case "stable":
      return s.version
    case "main":
      return ""
    case "experiment":
      return s.flag
    case "branch":
      return s.branch
    case "review":
      return `#${s.pr}`
  }
}

function age(added: string | undefined, today: string): string {
  if (!added) return ""
  const d = Math.round((Date.parse(today) - Date.parse(added)) / 86_400_000)
  return d < 14 ? `${d} d` : `${Math.floor(d / 7)} wk`
}

function docHref(doc: string): string {
  const [path, anchor] = doc.split("#")
  return `${REPO}/blob/main/${path}${anchor ? `#${anchor}` : ""}`
}

function StatusBadge({ status }: { status: Status }) {
  const detail = statusDetail(status)
  return (
    <span className={`st st-${status.kind}`}>
      <i />
      {KIND_LABEL[status.kind]}
      {detail && <em>{detail}</em>}
    </span>
  )
}

function Platforms({ on }: { on: Platform[] }) {
  return (
    <span className="pf">
      {PLATFORMS.map((p) => (
        <span key={p} className={on.includes(p) ? "" : "off"} title={on.includes(p) ? undefined : `Not on ${PLATFORM_LABEL[p]}`}>
          {PLATFORM_LABEL[p]}
        </span>
      ))}
    </span>
  )
}

function Row({ f, issues }: { f: Feature; issues: string[] }) {
  const status = statusOf(f)
  const links: { href: string; label: string }[] = []
  if (status.kind === "branch") links.push({ href: `${REPO}/tree/${status.branch}`, label: status.branch })
  if (f.pr) links.push({ href: `${REPO}/pull/${f.pr}`, label: `#${f.pr}` })
  for (const doc of f.docs ?? []) links.push({ href: docHref(doc), label: doc.replace(/^docs\//, "") })
  return (
    <li id={f.id} className={issues.length ? "row flagged" : "row"}>
      <div className="row-name">{f.name}</div>
      <div className="row-body">
        <p>{f.says}</p>
        {f.note && <p className="note">{f.note}</p>}
        {(f.shortcuts?.length || f.config?.length || f.flag) && (
          <p className="keys">
            {f.shortcuts?.map((s) => (
              <kbd key={s.keys} title={s.does}>
                {s.keys}
              </kbd>
            ))}
            {f.config?.map((c) => (
              <code key={c.key} title={c.does}>
                {c.key}
              </code>
            ))}
            {f.flag && <code title="Experiment flag on its branch">experiments.{f.flag}</code>}
          </p>
        )}
        {links.length > 0 && (
          <p className="links">
            {links.map((l) => (
              <a key={l.href} href={l.href}>
                {l.label}
              </a>
            ))}
          </p>
        )}
        {issues.map((text) => (
          <p key={text} className="issue">
            {text}
          </p>
        ))}
      </div>
      <div className="row-meta">
        <StatusBadge status={status} />
        <Platforms on={f.platforms} />
      </div>
    </li>
  )
}

// Stable features by the release that first had them, newest first; the newest release is open.
function ByVersion({ list, newest }: { list: Feature[]; newest: string }) {
  const versions = new Map<string, Feature[]>()
  for (const f of list) {
    const s = statusOf(f)
    const v = s.kind === "stable" ? s.version : ""
    versions.set(v, [...(versions.get(v) ?? []), f])
  }
  const order = [...versions.keys()].sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
  return (
    <div className="versions">
      {order.map((v) => (
        <details key={v} open={v === newest}>
          <summary>
            <span>{v}</span>
            <span className="count">{versions.get(v)!.length}</span>
          </summary>
          <ul>
            {versions.get(v)!.map((f) => (
              <li key={f.id}>
                <a href={`#${f.id}`}>{f.name}</a>
              </li>
            ))}
          </ul>
        </details>
      ))}
    </div>
  )
}

function Board({ audit }: { audit: Audit }) {
  const byKind = new Map<Status["kind"], Feature[]>(STATUS_KINDS.map((k) => [k, []]))
  for (const f of FEATURES) byKind.get(statusOf(f).kind)!.push(f)
  const name = (id: string) => FEATURES.find((f) => f.id === id)?.name ?? id
  return (
    <section className="board" id="status">
      <h2>Status</h2>
      <div className="cols">
        {STATUS_KINDS.map((kind) => {
          const list = byKind.get(kind)!
          return (
            <div key={kind} className="col">
              <h3>
                <span className={`st st-${kind}`}>
                  <i />
                  {KIND_LABEL[kind]}
                </span>
                <span className="count">{list.length}</span>
              </h3>
              <p className="means">{KIND_MEANS[kind]}</p>
              {kind === "stable" ? (
                <ByVersion list={list} newest={audit.release.version} />
              ) : (
                <ul>
                  {list.map((f) => {
                    const s = statusOf(f)
                    const extra = [s.kind === "main" ? "" : statusDetail(s), age(f.added, audit.today)].filter(Boolean).join(" · ")
                    return (
                      <li key={f.id}>
                        <a href={`#${f.id}`}>{f.name}</a>
                        {extra && <span>{extra}</span>}
                      </li>
                    )
                  })}
                </ul>
              )}
            </div>
          )
        })}
      </div>
      <div className="look">
        <h3>Needs a look</h3>
        {audit.issues.length ? (
          <ul>
            {audit.issues.map((i) => (
              <li key={`${i.feature}${i.text}`}>
                {i.feature ? <a href={`#${i.feature}`}>{name(i.feature)}</a> : null} {i.text}
              </li>
            ))}
          </ul>
        ) : (
          <p className="means">Nothing.</p>
        )}
      </div>
    </section>
  )
}

function References() {
  const shortcuts = FEATURES.flatMap((f) => (f.shortcuts ?? []).map((s) => ({ ...s, f })))
  const config = FEATURES.flatMap((f) => (f.config ?? []).map((c) => ({ ...c, f }))).sort((a, b) => a.key.localeCompare(b.key))
  const flags = FEATURES.flatMap((f) => {
    const s = statusOf(f)
    return s.kind === "experiment" && !f.config?.some((c) => c.key === s.flag) ? [{ flag: s.flag, f }] : []
  })
  return (
    <>
      <section className="group" id="shortcuts">
        <h2>Shortcuts</h2>
        <table className="ref">
          <tbody>
            {shortcuts.map(({ keys, does, f }) => (
              <tr key={keys + does}>
                <td className="k">
                  <kbd>{keys}</kbd>
                </td>
                <td>{does}</td>
                <td className="m">
                  <a href={`#${f.id}`}>{f.name}</a>
                </td>
                <td className="m">
                  <StatusBadge status={statusOf(f)} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <section className="group" id="config">
        <h2>
          Config keys <code>~/.config/scrawl/config.json</code>
        </h2>
        <table className="ref">
          <tbody>
            {config.map(({ key, value, does, f }) => (
              <tr key={key}>
                <td className="k">
                  <code>{key}</code>
                </td>
                <td>
                  {does} <code className="val">{value}</code>
                </td>
                <td className="m">
                  <a href={`#${f.id}`}>{f.name}</a>
                </td>
                <td className="m">
                  <StatusBadge status={statusOf(f)} />
                </td>
              </tr>
            ))}
            {flags.map(({ flag, f }) => (
              <tr key={flag}>
                <td className="k">
                  <code>experiments.{flag}</code>
                </td>
                <td>
                  Turn the experiment on <code className="val">true</code>
                </td>
                <td className="m">
                  <a href={`#${f.id}`}>{f.name}</a>
                </td>
                <td className="m">
                  <StatusBadge status={statusOf(f)} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </>
  )
}

export function FeatureMap({ audit }: { audit: Audit }) {
  const issuesFor = (id: string) => audit.issues.filter((i) => i.feature === id).map((i) => i.text)
  return (
    <>
      <header className="top">
        <a className="brand" href="/">
          <img src="/app-icon-64.png" width={22} height={22} alt="" />
          Scrawl
        </a>
        <nav>
          <a href="#status">Status</a>
          {GROUPS.map((g) => (
            <a key={g.id} href={`#${g.id}`}>
              {g.title}
            </a>
          ))}
          <a href="#shortcuts">Shortcuts</a>
          <a href="#config">Config</a>
        </nav>
      </header>
      <main className="map">
        <section className="intro">
          <h1>Feature map</h1>
          <p className="meta">
            {FEATURES.length} features · newest release {audit.release.version} ({audit.release.date}) · built {audit.today} from{" "}
            <a href={`${REPO}/blob/main/site/src/features.json`}>site/src/features.json</a>
          </p>
          <ol className="flow">
            {FLOW.map((step) => (
              <li key={step.title}>
                <a href={`#${step.group}`}>
                  <strong>{step.title}</strong>
                  <span>{step.detail}</span>
                </a>
              </li>
            ))}
          </ol>
        </section>
        <Board audit={audit} />
        {GROUPS.map((g) => {
          const list = FEATURES.filter((f) => f.group === g.id)
          if (!list.length) return null
          return (
            <section key={g.id} className="group" id={g.id}>
              <h2>{g.title}</h2>
              <ul className="rows">
                {list.map((f) => (
                  <Row key={f.id} f={f} issues={issuesFor(f.id)} />
                ))}
              </ul>
            </section>
          )
        })}
        <References />
      </main>
    </>
  )
}
