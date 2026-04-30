#!/usr/bin/env node
/**
 * Paid eval runner — executes fixtures against real providers.
 *
 * IMPORTANT: This script makes real API calls that cost money.
 * It should NEVER run in PR CI. Use via:
 *   - Manual: `pnpm eval:run --provider claude --fixture 001-unused-variable`
 *   - GitHub Actions: `.github/workflows/paid-evals.yml` (workflow_dispatch)
 *
 * Outputs are stored in evals/runs/<provider>-<fixture_id>-<timestamp>.json
 *
 * Usage:
 *   node evals/run-paid.mjs --provider <id> [--fixture <id>] [--all] [--force]
 */
import { readFileSync, writeFileSync, readdirSync, mkdirSync, existsSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = join(__dirname, "fixtures");
const RUNS_DIR = join(__dirname, "runs");

const EVAL_ELIGIBLE_PROVIDERS = ["claude"];

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {
    provider: null,
    fixture: null,
    all: false,
    force: false,
    dryRun: false,
  };

  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case "--provider":
        opts.provider = args[++i];
        break;
      case "--fixture":
        opts.fixture = args[++i];
        break;
      case "--all":
        opts.all = true;
        break;
      case "--force":
        opts.force = true;
        break;
      case "--dry-run":
        opts.dryRun = true;
        break;
      case "--help":
        printHelp();
        process.exit(0);
    }
  }

  return opts;
}

function printHelp() {
  console.log(`
Usage: node evals/run-paid.mjs --provider <id> [options]

Options:
  --provider <id>   Provider to use (claude)
  --fixture <id>    Run a specific fixture by ID
  --all             Run all fixtures
  --force           Allow running non-eligible providers
  --dry-run         Validate inputs without making API calls
  --help            Show this help

Examples:
  node evals/run-paid.mjs --provider claude --fixture 001-unused-variable
  node evals/run-paid.mjs --provider claude --all
  node evals/run-paid.mjs --provider claude --all --dry-run
`);
}

function discoverFixtures(fixtureId) {
  const files = readdirSync(FIXTURES_DIR).filter((f) => f.endsWith(".json"));
  const fixtures = [];

  for (const file of files) {
    const content = JSON.parse(readFileSync(join(FIXTURES_DIR, file), "utf-8"));
    if (fixtureId && content.id !== fixtureId) continue;
    fixtures.push(content);
  }

  return fixtures;
}

function generateOutputPath(provider, fixtureId) {
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  return join(RUNS_DIR, `${provider}-${fixtureId}-${timestamp}.json`);
}

async function runFixtureWithClaude(fixture, _opts) {
  const apiKey = process.env.ANTHROPIC_API_KEY;
  if (!apiKey) {
    throw new Error("ANTHROPIC_API_KEY is required for claude provider");
  }

  const systemPrompt = `You are a code reviewer. Analyze the following diff and return findings as JSON.
Return a JSON object with a "findings" array. Each finding should have:
- title: string
- body: string
- file_path: string
- line_start: number
- severity: one of "blocker", "critical", "warning", "info", "nitpick"
- confidence: number between 0 and 1
- agent_type: string (e.g. "security", "architecture", "performance")`;

  const response = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-api-key": apiKey,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify({
      model: "claude-sonnet-4-20250514",
      max_tokens: 4096,
      system: systemPrompt,
      messages: [
        {
          role: "user",
          content: `Review this diff:\n\n${fixture.diff}`,
        },
      ],
    }),
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`Claude API error (${response.status}): ${errorText}`);
  }

  const result = await response.json();
  const textContent = result.content?.find((c) => c.type === "text")?.text;
  if (!textContent) {
    throw new Error("No text content in Claude response");
  }

  let parsed;
  try {
    const jsonMatch = textContent.match(/\{[\s\S]*\}/);
    parsed = jsonMatch ? JSON.parse(jsonMatch[0]) : { findings: [] };
  } catch {
    parsed = { findings: [], raw_text: textContent };
  }

  return {
    ...parsed,
    _meta: {
      model: result.model,
      input_tokens: result.usage?.input_tokens,
      output_tokens: result.usage?.output_tokens,
      stop_reason: result.stop_reason,
    },
  };
}

async function runFixture(provider, fixture, opts) {
  console.log(`  Running fixture '${fixture.id}' with provider '${provider}'...`);

  if (opts.dryRun) {
    console.log(`    [DRY RUN] Would call ${provider} API`);
    return { findings: [], _meta: { dry_run: true } };
  }

  switch (provider) {
    case "claude":
      return runFixtureWithClaude(fixture, opts);
    default:
      throw new Error(
        `Provider '${provider}' eval runner not yet implemented. ` +
          "Extend evals/run-paid.mjs to support this provider.",
      );
  }
}

async function main() {
  const opts = parseArgs();

  if (!opts.provider) {
    console.error("ERROR: --provider is required");
    printHelp();
    process.exit(1);
  }

  if (!opts.force && !EVAL_ELIGIBLE_PROVIDERS.includes(opts.provider)) {
    console.error(
      `ERROR: Provider '${opts.provider}' is not eval-eligible. ` +
        `Eligible providers: ${EVAL_ELIGIBLE_PROVIDERS.join(", ")}. ` +
        "Use --force to override.",
    );
    process.exit(1);
  }

  if (!opts.fixture && !opts.all) {
    console.error("ERROR: Specify --fixture <id> or --all");
    printHelp();
    process.exit(1);
  }

  const fixtures = discoverFixtures(opts.fixture);
  if (fixtures.length === 0) {
    console.error(
      opts.fixture
        ? `ERROR: Fixture '${opts.fixture}' not found in evals/fixtures/`
        : "ERROR: No fixtures found in evals/fixtures/",
    );
    process.exit(1);
  }

  if (!existsSync(RUNS_DIR)) {
    mkdirSync(RUNS_DIR, { recursive: true });
  }

  console.log(`\n=== Paid Eval Run ===`);
  console.log(`Provider: ${opts.provider}`);
  console.log(`Fixtures: ${fixtures.length}`);
  console.log(`Dry run: ${opts.dryRun}\n`);

  const results = [];
  for (const fixture of fixtures) {
    try {
      const output = await runFixture(opts.provider, fixture, opts);
      const outputPath = generateOutputPath(opts.provider, fixture.id);
      const record = {
        fixture_id: fixture.id,
        provider: opts.provider,
        timestamp: new Date().toISOString(),
        ...output,
      };
      writeFileSync(outputPath, JSON.stringify(record, null, 2));
      console.log(`    ✓ Written to ${outputPath}`);
      results.push({ fixture_id: fixture.id, status: "success", path: outputPath });
    } catch (err) {
      console.error(`    ✗ Failed: ${err.message}`);
      results.push({ fixture_id: fixture.id, status: "error", error: err.message });
    }
  }

  console.log(`\n=== Summary ===`);
  const successes = results.filter((r) => r.status === "success").length;
  const failures = results.filter((r) => r.status === "error").length;
  console.log(`  Success: ${successes}, Failed: ${failures}`);

  if (failures > 0) {
    process.exit(1);
  }
}

main().catch((err) => {
  console.error(`Fatal error: ${err.message}`);
  process.exit(1);
});
