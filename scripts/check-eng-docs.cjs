/**
 * 工程文档体系合规检查（RFC / ADR / C4 / Specs / Plans / Backlog）
 *
 * 配套：.agents/engineering-docs-agent.ts（工程文档官）
 * 规则来源：eng-rfc-adr-c4-specs 技能 + 各项目 AGENTS.md 的文档约定
 *
 * 用法：
 *   node scripts/check-eng-docs.cjs              # 全量检查，有 ERROR 时 exit 1
 *   node scripts/check-eng-docs.cjs --json       # 机器可读输出
 *   node scripts/check-eng-docs.cjs --root ../x  # 指定项目根（脚本不在项目内时用，如全局技能 assets）
 *
 * 配置：项目根 eng-docs.config.json（可选，字段见 DEFAULT_CONFIG）
 *   未配置时按 .trae/adr → docs/adr → docs/decisions 顺序探测 ADR 目录，
 *   其余目录用通用默认值。因此同一份脚本可直接复制到其他项目使用。
 *
 * 分级：
 *   ERROR —— 新增文档必须修的硬性违规（命名、编号重复、缺头字段、REQ 冲突）
 *   WARN  —— 历史遗留或索引缺失，不阻断，但需在汇报中说明
 *   INFO  —— 统计与下一个可用编号
 *
 * 注意：历史 ADR 可能不是 UTF-8 编码，本脚本只做 ASCII 级匹配，从不重写文档。
 */

const fs = require('fs');
const path = require('path');

const rootArgIndex = process.argv.indexOf('--root');
const rootArg = rootArgIndex >= 0 ? process.argv[rootArgIndex + 1] : null;
const ROOT = rootArg ? path.resolve(process.cwd(), rootArg) : path.resolve(__dirname, '..');

const DEFAULT_CONFIG = {
  adrDir: '.trae/adr',
  rfcDir: 'docs/rfc',
  architectureDir: 'docs/architecture',
  specsDir: 'docs/specs',
  plansDir: 'docs/plans',
  backlogFile: 'docs/backlog.md',
  adrIndex: null, // 默认 <adrDir>/README.md
  gateCommand: 'node scripts/check-eng-docs.cjs',
};

const ADR_DIR_CANDIDATES = ['.trae/adr', 'docs/adr', 'docs/decisions'];

const errors = [];
const warnings = [];
const info = [];

const rel = (p) => path.relative(ROOT, p).split(path.sep).join('/');
const abs = (p) => path.join(ROOT, p);

function listMarkdown(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((f) => f.toLowerCase().endsWith('.md'))
    .sort();
}

function readText(file) {
  return fs.readFileSync(file, 'utf8');
}

function headOf(text, lines = 20) {
  return text.split(/\r?\n/).slice(0, lines).join('\n');
}

const pad3 = (n) => String(n).padStart(3, '0');
const nextNumber = (used) => (used.length === 0 ? 1 : Math.max(...used) + 1);

// ============================================================
// 配置加载（配置 > 探测 > 默认）
// ============================================================

function loadConfig() {
  const config = { ...DEFAULT_CONFIG };
  let source = '默认值';

  const configPath = abs('eng-docs.config.json');
  if (fs.existsSync(configPath)) {
    try {
      Object.assign(config, JSON.parse(readText(configPath)));
      source = 'eng-docs.config.json';
    } catch (error) {
      errors.push(`eng-docs.config.json 解析失败：${error.message}`);
    }
  }

  if (!fs.existsSync(abs(config.adrDir))) {
    const detected = ADR_DIR_CANDIDATES.find((d) => fs.existsSync(abs(d)));
    if (detected && detected !== config.adrDir) {
      config.adrDir = detected;
      source = `${source} + 目录探测`;
    }
  }

  config.adrIndex = config.adrIndex || path.join(config.adrDir, 'README.md');
  config.source = source;
  return config;
}

const CONFIG = loadConfig();

info.push(`配置来源：${CONFIG.source}｜ADR 目录：${CONFIG.adrDir}｜RFC 目录：${CONFIG.rfcDir}`);

// ============================================================
// 一、ADR（<adrDir>/NNN-slug.md）
// ============================================================

const ADR_DIR = abs(CONFIG.adrDir);
const ADR_INDEX = abs(CONFIG.adrIndex);
const ADR_NAME_RE = /^(\d{3})-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$/;
const adrFiles = listMarkdown(ADR_DIR).filter((f) => f.toLowerCase() !== 'readme.md');
const adrByNumber = new Map();

for (const file of adrFiles) {
  const match = ADR_NAME_RE.exec(file);
  if (!match) {
    errors.push(`ADR 文件名不合规：${rel(path.join(ADR_DIR, file))}（应为 NNN-kebab-slug.md）`);
    continue;
  }
  const num = Number(match[1]);
  if (adrByNumber.has(num)) {
    errors.push(`ADR 编号重复：${match[1]} → ${adrByNumber.get(num)} 与 ${file}`);
    continue;
  }
  adrByNumber.set(num, file);
}

// 标题与编号一致性（历史遗留仅告警）
for (const [num, file] of adrByNumber) {
  const firstLine = headOf(readText(path.join(ADR_DIR, file)), 1).trim();
  if (!firstLine.startsWith(`# ADR-${pad3(num)}`)) {
    warnings.push(
      `ADR 标题与编号不一致：${rel(path.join(ADR_DIR, file))}（首行非 "# ADR-${pad3(num)}"）`,
    );
  }
}

// 索引覆盖（历史遗留仅告警）
if (fs.existsSync(ADR_INDEX)) {
  const indexText = readText(ADR_INDEX);
  const missing = [];
  for (const [num, file] of adrByNumber) {
    const link = file.replace(/\.md$/, '');
    if (!indexText.includes(link) && !new RegExp(`\\|\\s*${pad3(num)}\\s*\\|`).test(indexText)) {
      missing.push(file);
    }
  }
  if (missing.length > 0) {
    warnings.push(
      `${rel(ADR_INDEX)} 索引未覆盖 ${missing.length} 份 ADR：${missing.slice(0, 5).join(', ')}${missing.length > 5 ? ' …' : ''}`,
    );
  }
} else {
  warnings.push(`缺少 ADR 索引：${rel(ADR_INDEX)}`);
}

const adrNumbers = [...adrByNumber.keys()].sort((a, b) => a - b);
info.push(`ADR：${adrFiles.length} 份文件，占用编号 ${adrNumbers.length} 个`);
info.push(`下一个可用 ADR 编号：ADR-${pad3(nextNumber(adrNumbers))}`);

// ============================================================
// 二、RFC（<rfcDir>/RFC-00N-slug.md）
// ============================================================

const RFC_DIR = abs(CONFIG.rfcDir);
const RFC_NAME_RE = /^RFC-(\d{3})-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$/;
const rfcFiles = listMarkdown(RFC_DIR).filter((f) => f.toLowerCase() !== 'readme.md');
const rfcByNumber = new Map();
const reqOwners = new Map(); // REQ-001 -> Set(rfc file)

for (const file of rfcFiles) {
  const full = path.join(RFC_DIR, file);
  const match = RFC_NAME_RE.exec(file);
  if (!match) {
    errors.push(`RFC 文件名不合规：${rel(full)}（应为 RFC-00N-kebab-slug.md）`);
    continue;
  }
  const num = Number(match[1]);
  if (rfcByNumber.has(num)) {
    errors.push(`RFC 编号重复：${match[1]} → ${rfcByNumber.get(num)} 与 ${file}`);
    continue;
  }
  rfcByNumber.set(num, file);

  const text = readText(full);
  const head = headOf(text, 20);

  if (!/^>\s*Status\s*:/m.test(head)) {
    errors.push(`RFC 头部缺 Status 字段：${rel(full)}`);
  } else if (!/Status\s*:\s*(Draft|In Progress|Implemented)/.test(head)) {
    warnings.push(`RFC Status 取值非约定枚举（Draft|In Progress|Implemented）：${rel(full)}`);
  }
  if (!/Updated\s*:\s*\d{4}-\d{2}-\d{2}/.test(head)) {
    errors.push(`RFC 头部缺 Updated: YYYY-MM-DD：${rel(full)}`);
  }
  if (!/一致性声明/.test(head)) {
    warnings.push(`RFC 头部缺「一致性声明」：${rel(full)}`);
  }
  if (!/^##\s*1\.\s*需求/m.test(text)) {
    errors.push(`RFC 缺 §1 需求（REQ 表）：${rel(full)}`);
  }
  if (!/^##\s*8\./m.test(text)) {
    errors.push(`RFC 缺 §8 追溯章节：${rel(full)}`);
  }

  const reqs = new Set([...text.matchAll(/REQ-(\d{3})/g)].map((m) => `REQ-${m[1]}`));
  for (const req of reqs) {
    if (!reqOwners.has(req)) reqOwners.set(req, new Set());
    reqOwners.get(req).add(file);
  }
}

for (const [req, owners] of reqOwners) {
  if (owners.size > 1) {
    errors.push(`${req} 编号被多份 RFC 复用：${[...owners].join(', ')}`);
  }
}

info.push(`RFC：${rfcFiles.length} 份，已注册 REQ ${reqOwners.size} 个`);
info.push(`下一个可用 RFC 编号：RFC-${pad3(nextNumber([...rfcByNumber.keys()]))}`);
const usedReqNumbers = [...reqOwners.keys()].map((r) => Number(r.slice(4)));
info.push(`下一个可用 REQ 编号：REQ-${pad3(nextNumber(usedReqNumbers))}`);

// ============================================================
// 三、Spec / Plan 快照
// ============================================================

const SPEC_DIR = abs(CONFIG.specsDir);
const PLAN_DIR = abs(CONFIG.plansDir);
const SNAPSHOT_RE = /^\d{4}-\d{2}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*-design\.md$/;
const PLAN_RE = /^\d{4}-\d{2}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*-plan\.md$/;

for (const file of listMarkdown(SPEC_DIR).filter((f) => f.toLowerCase() !== 'readme.md')) {
  if (!SNAPSHOT_RE.test(file)) {
    errors.push(
      `spec 快照命名不合规：${rel(path.join(SPEC_DIR, file))}（应为 YYYY-MM-DD-slug-design.md）`,
    );
  }
}

for (const file of listMarkdown(PLAN_DIR).filter((f) => f.toLowerCase() !== 'readme.md')) {
  if (!PLAN_RE.test(file)) {
    errors.push(
      `实现计划命名不合规：${rel(path.join(PLAN_DIR, file))}（应为 YYYY-MM-DD-slug-plan.md）`,
    );
  }
}

info.push(
  `spec 快照：${listMarkdown(SPEC_DIR).length} 份；实现计划：${listMarkdown(PLAN_DIR).length} 份`,
);

// ============================================================
// 四、C4 视图（<architectureDir>/C1|C2|C3）
// ============================================================

const ARCH_DIR = abs(CONFIG.architectureDir);
const C4_FILES = ['C1-context.md', 'C2-container.md', 'C3-component.md'];
const missingC4 = C4_FILES.filter((f) => !fs.existsSync(path.join(ARCH_DIR, f)));
if (missingC4.length === C4_FILES.length) {
  info.push(`C4 视图尚未建立（${CONFIG.architectureDir}/ 为空，首次涉及结构变更时创建 C1/C2/C3）`);
} else if (missingC4.length > 0) {
  warnings.push(`C4 视图不完整，缺少：${missingC4.join(', ')}`);
} else {
  info.push('C4 视图：C1/C2/C3 齐全');
}

// ============================================================
// 五、Backlog（条目编号 B<区块>-<序号> 唯一，如 B1-1）
// ============================================================

const BACKLOG = abs(CONFIG.backlogFile);
if (fs.existsSync(BACKLOG)) {
  const text = readText(BACKLOG);
  const ids = [...text.matchAll(/\bB(\d+)-(\d+)\b/g)].map((m) => `B${m[1]}-${m[2]}`);
  const duplicates = [...new Set(ids.filter((id, idx) => ids.indexOf(id) !== idx))];
  if (duplicates.length > 0) {
    warnings.push(`${rel(BACKLOG)} 存在重复条目编号：${duplicates.join(', ')}`);
  }
  info.push(`Backlog：${new Set(ids).size} 个条目`);
} else {
  info.push(`Backlog 尚未建立（${CONFIG.backlogFile} 不存在）`);
}

// ============================================================
// 输出
// ============================================================

const report = { errors, warnings, info, config: CONFIG };

if (process.argv.includes('--json')) {
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
} else {
  console.log('\n=== 工程文档体系体检 ===\n');
  for (const line of info) console.log(`[INFO]  ${line}`);
  for (const line of warnings) console.log(`[WARN]  ${line}`);
  for (const line of errors) console.log(`[ERROR] ${line}`);
  console.log('');
  console.log(
    `结果：${errors.length === 0 ? 'PASS' : 'FAIL'}（ERROR ${errors.length} / WARN ${warnings.length}）`,
  );
  console.log('');
}

process.exit(errors.length === 0 ? 0 : 1);
