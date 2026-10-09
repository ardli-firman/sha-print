import { execSync } from "node:child_process";

export function cleanGitEnv(): NodeJS.ProcessEnv {
  const env = { ...process.env };
  delete env.GIT_CONFIG_COUNT;
  delete env.GIT_CONFIG_VALUE_0;
  delete env.GIT_CONFIG_VALUE_1;
  return env;
}

export function execGit(cmd: string, cwd?: string): string {
  return execSync(cmd, {
    cwd: cwd || process.cwd(),
    encoding: "utf-8",
    env: cleanGitEnv(),
    stdio: ["pipe", "pipe", "pipe"],
  }).trim();
}

export function execGh(cmd: string, cwd?: string): string {
  return execSync(`gh ${cmd}`, {
    cwd: cwd || process.cwd(),
    encoding: "utf-8",
    env: cleanGitEnv(),
    stdio: ["pipe", "pipe", "pipe"],
  }).trim();
}
