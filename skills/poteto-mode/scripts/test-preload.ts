import { existsSync, readFileSync } from "node:fs";
import { installDependencies } from "./bootstrap.ts";

// Bun caches a missing node_modules when the test process starts, so a fresh
// install is only visible to a new process. Inside the test runner Bun also
// rewrites process.argv, so the restart reads the real command line.
function commandLine(): string[] {
  if (existsSync("/proc/self/cmdline")) {
    return readFileSync("/proc/self/cmdline", "utf8").split("\0").filter(Boolean);
  }
  const ps = Bun.spawnSync(["ps", "-ww", "-o", "args=", "-p", String(process.pid)]);
  return ps.stdout.toString().trim().split(/\s+/);
}

if (installDependencies()) {
  const restarted = Bun.spawnSync(commandLine(), {
    cwd: process.cwd(),
    env: process.env,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  process.exit(restarted.exitCode ?? 1);
}
