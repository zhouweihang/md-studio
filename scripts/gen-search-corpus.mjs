#!/usr/bin/env node
import { mkdir, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'

const LIMIT = 64
const CHINESE = [
  '本地优先的写作工具应当把文件系统当作唯一事实源。',
  '搜索延迟应当随着输入即时反馈，而不是等待整篇扫描完成。',
  '外部编辑器修改文件后，缓存需要尽快失效。',
  '中文正文用于验证大小写折叠之外的分支。',
  '性能预算必须可测量、可回归，而不是凭感觉判断。',
  '内存快照只服务读路径，保存仍然比对内容 revision。',
]
const ENGLISH = [
  'The quick brown fox jumps over the lazy dog.',
  'Notes stay on disk; the app never owns the canonical copy.',
  'Search should stream partial results while scanning continues.',
  'A stale cache must degrade to a slower path, never a wrong answer.',
  'Benchmarks are only useful when the corpus is reproducible.',
  'Autosave merges edits and flushes before the window closes.',
]

function parseArgs(argv) {
  const args = { notes: 1000, seed: 42, out: '' }
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (key === '--out') args.out = argv[++index]
    else if (key === '--notes') args.notes = Number(argv[++index])
    else if (key === '--seed') args.seed = Number(argv[++index])
    else throw new Error(`unknown argument: ${key}`)
  }
  if (!args.out) throw new Error('--out is required')
  if (!Number.isInteger(args.notes) || args.notes < 3) {
    throw new Error('--notes must be at least 3 (target + boundary + one regular note)')
  }
  return args
}

function mulberry32(seed) {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let value = state
    value = Math.imul(value ^ (value >>> 15), value | 1)
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61)
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296
  }
}

function pick(list, random) {
  return list[Math.floor(random() * list.length)]
}

function targetSize(random) {
  const bucket = random()
  if (bucket < 0.7) return 2048 + Math.floor(random() * 6144)
  if (bucket < 0.9) return 8192 + Math.floor(random() * 24576)
  return 32768 + Math.floor(random() * 229376)
}

function buildContent(index, bytes, random) {
  const header = `# Note ${index}\n\n`
  const parts = [header]
  let size = Buffer.byteLength(header)
  while (size < bytes) {
    const line = random() < 0.3 ? pick(CHINESE, random) : pick(ENGLISH, random)
    parts.push(line, '\n')
    size += Buffer.byteLength(line) + 1
  }
  return parts.join('')
}

async function writeNote(out, index, bytes, random) {
  const bucket = `g-${String(Math.floor(index / 100)).padStart(3, '0')}`
  const name = `note-${String(index).padStart(5, '0')}.md`
  await mkdir(path.join(out, bucket), { recursive: true })
  await writeFile(path.join(out, bucket, name), buildContent(index, bytes, random))
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  const out = path.resolve(args.out)
  const random = mulberry32(args.seed)

  await rm(out, { recursive: true, force: true })
  await mkdir(out, { recursive: true })

  let totalBytes = 0
  let pending = []
  // Reserve two slots for target.md and the 5 MiB boundary file so the corpus
  // never exceeds the workspace note limit it is meant to benchmark.
  for (let index = 0; index < args.notes - 2; index += 1) {
    const bytes = targetSize(random)
    totalBytes += bytes
    pending.push(writeNote(out, index, bytes, random))
    if (pending.length >= LIMIT) {
      await Promise.all(pending)
      pending = []
    }
  }
  await Promise.all(pending)

  const target = '# Target\n\nthe needle lives here\n'
  await writeFile(path.join(out, 'target.md'), target)
  totalBytes += Buffer.byteLength(target)

  // One 5 MiB boundary file: MAX_FILE_BYTES is exactly 5 * 1024 * 1024.
  const hugeSize = 5 * 1024 * 1024 - 1024
  const hugeHeader = '# Huge boundary note\n\n'
  const hugeLine = 'boundary content for the maximum supported note size\n'
  const repeat = Math.ceil((hugeSize - hugeHeader.length) / hugeLine.length)
  const huge = hugeHeader + hugeLine.repeat(repeat)
  await mkdir(path.join(out, 'edge'), { recursive: true })
  await writeFile(path.join(out, 'edge', 'huge-5mb.md'), huge)
  totalBytes += Buffer.byteLength(huge)

  const manifest = {
    notes: args.notes,
    bytes: totalBytes,
    seed: args.seed,
    query: 'needle',
  }
  await writeFile(path.join(out, '.corpus.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(
    `corpus: ${out} | ${manifest.notes} notes | ${(totalBytes / 1024 / 1024).toFixed(1)} MiB | seed ${args.seed}`,
  )
}

main().catch((error) => {
  console.error(`gen-search-corpus: ${error.message}`)
  process.exit(1)
})
