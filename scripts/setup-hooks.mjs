// Point git at the committed .githooks/ directory. Runs automatically via the
// package.json "prepare" script on `pnpm install`, and can be run by hand:
//   node scripts/setup-hooks.mjs
//
// Uses core.hooksPath (a committed, dependency-free approach) instead of husky,
// so there is nothing to install and the hooks are versioned with the repo.
// Skips silently when not inside a git checkout (CI tarballs, vendored copies).
import { execFileSync } from "node:child_process";

function git(args) {
  return execFileSync("git", args, { stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
}

try {
  git(["rev-parse", "--git-dir"]); // throws if not a git repo
  git(["config", "core.hooksPath", ".githooks"]);
  console.log("✓ git hooks enabled (core.hooksPath = .githooks)");
} catch {
  // Not a git checkout — nothing to wire up. Not an error.
}
