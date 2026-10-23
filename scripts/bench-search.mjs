#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const CORPUS_ROOT = path.join(ROOT, 'target', 'bench-corpus')
const GEN = path.join(ROOT, 'scripts', 'gen-search-corpus.mjs')
const CARGO = process.platform === 'win32' ? 'cargo.exe' : 'cargo'

function parseArgs(argv) {
  const args = {
    sizes: [1000, 10000, 50000],
    query: 'needle',
    repeats: new Map(),
    defaultRepeat: 30,
    warmup: 5,
    tier: process.platform === 'win32' ? 'dev-windows' : 'dev-unix',
    writeBaseline: false,
  }
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (key === '--sizes') {
      args.sizes = argv[++index]
        .split(',')
        .map((value) => Number(value.trim()))
        .filter((value) => Number.isInteger(value) && value > 0)
    } else if (key === '--query') args.query = argv[++index]
    else if (key === '--repeat') args.defaultRepeat = Number(argv[++index])
    else if (key === '--repeat-map') {
      for (const pair of argv[++index].split(',')) {
        const [size, repeat] = pair.split(':').map(Number)
        if (Number.isInteger(size) && Number.isInteger(repeat)) args.repeats.set(size, repeat)
      }
    } else if (key === '--warmup') args.warmup = Number(argv[++index])
    else if (key === '--tier') args.tier = argv[++index]
    else if (key === '--write-baseline') args.writeBaseline = true
    else throw new Error(`unknown argument: ${key}`)
  }
  return args
}

function rustVersion() {
  try {
    return execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim()
  } catch {
    return 'unknown'
  }
}

async function ensureCorpus(size) {
  const dir = path.join(CORPUS_ROOT, String(size))
  const manifest = path.join(dir, '.corpus.json')
  if (!existsSync(manifest)) {
    execFileSync(process.execPath, [GEN, '--out', dir, '--notes', String(size), '--seed', '42'], {
      stdio: 'inherit',
    })
  }
  return { dir, manifest: JSON.parse(await readFile(manifest, 'utf8')) }
}

function runBench(dir, query, repeat, warmup) {
  const output = execFileSync(
    CARGO,
    [
      'run',
      '--release',
      '--quiet',
      '--bin',
      'bench_search',
      '--',
      '--root',
      dir,
      '--query',
      query,
      '--repeat',
      String(repeat),
      '--warmup',
      String(warmup),
      '--json',
    ],
    { cwd: path.join(ROOT, 'src-tauri'), encoding: 'utf8' },
  )
  const lines = output.trim().split(/\r?\n/).filter(Boolean)
  return JSON.parse(lines[lines.length - 1])
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  const results = {}
  for (const size of args.sizes) {
    const repeat = args.repeats.get(size) ?? args.defaultRepeat
    const { dir, manifest } = await ensureCorpus(size)
    console.log(`\n== ${size} notes (${(manifest.bytes / 1024 / 1024).toFixed(1)} MiB), ${repeat} runs ==`)
    const result = runBench(dir, args.query, repeat, args.warmup)
    results[String(size)] = { ...result, notes: manifest.notes, bytes: manifest.bytes }
    console.log(
      `cold ${result.coldMs.toFixed(1)} ms | warm p50 ${result.warmP50Ms.toFixed(1)} ms | p95 ${result.warmP95Ms.toFixed(1)} ms | hits ${result.hits}`,
    )
  }

  const capturedAt = new Date().toISOString().slice(0, 10)
  const report = {
    tier: args.tier,
    capturedAt,
    machine: {
      cpu: os.cpus()[0]?.model?.trim() ?? 'unknown',
      cores: os.cpus().length,
      ramGb: Number((os.totalmem() / 1024 ** 3).toFixed(1)),
      os: `${os.platform()} ${os.release()}`,
    },
    toolchain: { rustc: rustVersion(), profile: 'release' },
    query: args.query,
    results,
  }

  const resultsDir = path.join(ROOT, 'docs', 'perf', 'results')
  await mkdir(resultsDir, { recursive: true })
  const resultsFile = path.join(resultsDir, `${capturedAt}-${args.tier}.json`)
  await writeFile(resultsFile, `${JSON.stringify(report, null, 2)}\n`)
  console.log(`\nwrote ${path.relative(ROOT, resultsFile)}`)

  if (args.writeBaseline) {
    const baselineDir = path.join(ROOT, 'docs', 'perf', 'baseline')
    await mkdir(baselineDir, { recursive: true })
    const baselineFile = path.join(baselineDir, `${args.tier}.json`)
    await writeFile(baselineFile, `${JSON.stringify(report, null, 2)}\n`)
    console.log(`wrote ${path.relative(ROOT, baselineFile)}`)
  }
}

main().catch((error) => {
  console.error(`bench-search: ${error.message}`)
  process.exit(1)
})
