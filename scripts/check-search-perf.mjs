#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const DEFAULT_TIER = 'dev-i9-14900hx'

function parseArgs(argv) {
  const args = {
    tier: DEFAULT_TIER,
    baseline: null,
    results: null,
    median: 1.2,
    p95: 2,
  }
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (key === '--tier') args.tier = argv[++index]
    else if (key === '--baseline') args.baseline = argv[++index]
    else if (key === '--results') args.results = argv[++index]
    else if (key === '--median') args.median = Number(argv[++index])
    else if (key === '--p95') args.p95 = Number(argv[++index])
    else if (key === '--help' || key === '-h') {
      console.log(
        'usage: check-search-perf --results <latest.json> [--baseline <baseline.json>] [--tier <tier>] [--median 1.2] [--p95 2]',
      )
      process.exit(0)
    } else throw new Error(`unknown argument: ${key}`)
  }
  if (!(args.median > 0) || !(args.p95 > 0)) throw new Error('thresholds must be positive')
  return args
}

function latestResults(tier) {
  const dir = path.join(ROOT, 'docs', 'perf', 'results')
  if (!existsSync(dir)) {
    throw new Error('no results directory; run scripts/bench-search.mjs first')
  }
  const files = readdirSync(dir)
    .filter((file) => file.endsWith('.json') && file.includes(tier))
    .sort()
  if (files.length === 0) {
    throw new Error(`no results for tier ${tier}; run scripts/bench-search.mjs first`)
  }
  return path.join('docs', 'perf', 'results', files[files.length - 1])
}

function readJson(file, label) {
  if (!existsSync(file)) throw new Error(`${label} not found: ${file}`)
  try {
    return JSON.parse(readFileSync(file, 'utf8'))
  } catch (error) {
    throw new Error(`${label} is not valid JSON: ${error.message}`)
  }
}

function ratio(current, baseline) {
  return baseline > 0 ? current / baseline : Number.POSITIVE_INFINITY
}

function format(value) {
  return Number.isFinite(value) ? value.toFixed(2) : 'inf'
}

function main() {
  const args = parseArgs(process.argv.slice(2))
  const baselinePath = path.resolve(
    ROOT,
    args.baseline ?? path.join('docs', 'perf', 'baseline', `${args.tier}.json`),
  )
  const resultsPath = path.resolve(ROOT, args.results ?? latestResults(args.tier))

  const baseline = readJson(baselinePath, 'baseline')
  const results = readJson(resultsPath, 'results')

  if (baseline.tier !== results.tier) {
    console.error(
      `tier mismatch: baseline=${baseline.tier} results=${results.tier} (cross-tier comparison is not allowed)`,
    )
    process.exit(1)
  }

  const rows = []
  let violations = 0
  for (const [size, base] of Object.entries(baseline.results ?? {})) {
    const current = results.results?.[size]
    if (!current) {
      rows.push({ size, metric: 'missing', baseline: '-', current: '-', ratio: '-', limit: '-', ok: false })
      violations += 1
      continue
    }

    const medianRatio = ratio(current.warmP50Ms, base.warmP50Ms)
    const p95Ratio = ratio(current.warmP95Ms, base.warmP95Ms)

    rows.push({
      size,
      metric: 'p50',
      baseline: base.warmP50Ms,
      current: current.warmP50Ms,
      ratio: medianRatio,
      limit: args.median,
      ok: medianRatio <= args.median,
    })
    rows.push({
      size,
      metric: 'p95',
      baseline: base.warmP95Ms,
      current: current.warmP95Ms,
      ratio: p95Ratio,
      limit: args.p95,
      ok: p95Ratio <= args.p95,
    })
    if (medianRatio > args.median) violations += 1
    if (p95Ratio > args.p95) violations += 1
  }

  console.log(`baseline: ${path.relative(ROOT, baselinePath)} (${baseline.tier})`)
  console.log(`results:  ${path.relative(ROOT, resultsPath)}`)
  console.log('')
  console.log('size    metric  baseline(ms)  current(ms)  ratio   limit   result')
  for (const row of rows) {
    const baselineCell =
      typeof row.baseline === 'number' ? row.baseline.toFixed(1).padStart(12) : String(row.baseline).padStart(12)
    const currentCell =
      typeof row.current === 'number' ? row.current.toFixed(1).padStart(11) : String(row.current).padStart(11)
    console.log(
      `${String(row.size).padEnd(7)} ${row.metric.padEnd(7)} ${baselineCell}  ${currentCell}  ${format(row.ratio).padStart(6)}  ${String(row.limit).padStart(5)}   ${row.ok ? 'ok' : 'FAIL'}`,
    )
  }
  console.log('')

  if (violations > 0) {
    console.error(`结果：FAIL（${violations} 项超过阈值）`)
    process.exit(1)
  }
  console.log('结果：PASS（中位数 ≤ 1.2×，p95 ≤ 2×）')
}

try {
  main()
} catch (error) {
  console.error(`check-search-perf: ${error.message}`)
  process.exit(2)
}
