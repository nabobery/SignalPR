#!/usr/bin/env node
/**
 * Eval fixture validator + deterministic graders.
 * Runs without any model calls — validates fixture schema and
 * grades any existing run outputs in evals/runs/.
 *
 * Usage:
 *   node evals/validate.mjs           # validate fixtures only
 *   node evals/validate.mjs --grade   # also grade existing run outputs
 */
import { readFileSync, readdirSync, existsSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = join(__dirname, "fixtures");
const RUNS_DIR = join(__dirname, "runs");

const VALID_SEVERITIES = ["blocker", "critical", "warning", "info", "nitpick"];

const REQUIRED_FIXTURE_FIELDS = ["id", "description", "diff", "expected"];
const REQUIRED_EXPECTED_FIELDS = ["min_findings", "max_findings", "must_mention_files"];

let errors = 0;
let warnings = 0;

function error(msg) {
  console.error(`  ERROR: ${msg}`);
  errors++;
}

function warn(msg) {
  console.warn(`  WARN: ${msg}`);
  warnings++;
}

function validateFixtures() {
  console.log("=== Validating fixtures ===\n");

  const files = readdirSync(FIXTURES_DIR).filter((f) => f.endsWith(".json"));
  if (files.length === 0) {
    error("No fixture files found in evals/fixtures/");
    return;
  }

  console.log(`Found ${files.length} fixture(s)\n`);

  for (const file of files) {
    console.log(`  Checking ${file}...`);
    const path = join(FIXTURES_DIR, file);
    let fixture;

    try {
      fixture = JSON.parse(readFileSync(path, "utf-8"));
    } catch (e) {
      error(`${file}: Invalid JSON - ${e.message}`);
      continue;
    }

    for (const field of REQUIRED_FIXTURE_FIELDS) {
      if (!(field in fixture)) {
        error(`${file}: Missing required field '${field}'`);
      }
    }

    if (!fixture.expected) continue;

    for (const field of REQUIRED_EXPECTED_FIELDS) {
      if (!(field in fixture.expected)) {
        error(`${file}: Missing required expected field '${field}'`);
      }
    }

    if (fixture.expected.min_findings > fixture.expected.max_findings) {
      error(`${file}: min_findings > max_findings`);
    }

    if (fixture.expected.severity_range) {
      for (const sev of fixture.expected.severity_range) {
        if (!VALID_SEVERITIES.includes(sev)) {
          error(`${file}: Invalid severity '${sev}' in severity_range`);
        }
      }
    }

    if (typeof fixture.diff !== "string" || fixture.diff.length === 0) {
      error(`${file}: 'diff' must be a non-empty string`);
    }

    if (!Array.isArray(fixture.expected.must_mention_files) || fixture.expected.must_mention_files.length === 0) {
      warn(`${file}: 'must_mention_files' is empty`);
    }

    console.log(`  ✓ ${file} valid`);
  }
}

function gradeOutputs() {
  console.log("\n=== Grading run outputs ===\n");

  if (!existsSync(RUNS_DIR)) {
    console.log("  No runs/ directory — skipping grading.\n");
    return;
  }

  const runFiles = readdirSync(RUNS_DIR).filter((f) => f.endsWith(".json"));
  if (runFiles.length === 0) {
    console.log("  No run output files found.\n");
    return;
  }

  for (const file of runFiles) {
    console.log(`  Grading ${file}...`);
    const path = join(RUNS_DIR, file);
    let output;

    try {
      output = JSON.parse(readFileSync(path, "utf-8"));
    } catch (e) {
      error(`${file}: Invalid JSON - ${e.message}`);
      continue;
    }

    // Schema-valid output rate
    if (!output.findings || !Array.isArray(output.findings)) {
      error(`${file}: Missing or invalid 'findings' array`);
      continue;
    }

    // Anchored-file validity
    const findings = output.findings;
    const filesReferenced = new Set(findings.map((f) => f.file_path).filter(Boolean));
    console.log(`    Findings: ${findings.length}, Files referenced: ${filesReferenced.size}`);

    // Duplicate rate
    const titles = findings.map((f) => f.title);
    const uniqueTitles = new Set(titles);
    const dupeRate = 1 - uniqueTitles.size / Math.max(titles.length, 1);
    if (dupeRate > 0.3) {
      warn(`${file}: High duplicate rate (${(dupeRate * 100).toFixed(1)}%)`);
    }
    console.log(`    Duplicate rate: ${(dupeRate * 100).toFixed(1)}%`);

    // Severity distribution sanity
    const sevCounts = {};
    for (const f of findings) {
      const sev = f.severity || "unknown";
      sevCounts[sev] = (sevCounts[sev] || 0) + 1;
    }
    console.log(`    Severity distribution: ${JSON.stringify(sevCounts)}`);

    if (sevCounts["unknown"]) {
      warn(`${file}: ${sevCounts["unknown"]} findings with unknown severity`);
    }

    // Check all severities are valid
    for (const sev of Object.keys(sevCounts)) {
      if (!VALID_SEVERITIES.includes(sev) && sev !== "unknown") {
        error(`${file}: Invalid severity '${sev}' in output`);
      }
    }

    console.log(`  ✓ ${file} graded`);
  }
}

// Main
validateFixtures();

if (process.argv.includes("--grade")) {
  gradeOutputs();
}

console.log(`\n=== Summary: ${errors} error(s), ${warnings} warning(s) ===`);
if (errors > 0) {
  process.exit(1);
}
