import {
  SettingsManager,
  createBashToolDefinition,
  createLocalBashOperations,
  getAgentDir,
  type BashOperations,
  type ExtensionAPI,
  type ToolInfo,
} from "@earendil-works/pi-coding-agent";
import { delimiter, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HOST_EXTENSION_PATH = normalizedPath(fileURLToPath(import.meta.url));

function normalizedPath(path: string): string {
  const normalized = normalize(resolve(path)).replaceAll("\\", "/");
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function isHostRuntimeVariable(name: string): boolean {
  const comparableName = process.platform === "win32" ? name.toUpperCase() : name;
  return comparableName === "PORT"
    || comparableName === "NODE_ENV"
    || comparableName.startsWith("NEXT_");
}

function sanitizedEnvironment(environment: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const sanitized = { ...environment };
  for (const name of Object.keys(sanitized)) {
    if (isHostRuntimeVariable(name)) delete sanitized[name];
  }

  const pathKey = process.platform === "win32"
    ? Object.keys(sanitized).find((name) => name.toUpperCase() === "PATH") ?? "PATH"
    : "PATH";
  const agentBinDir = join(getAgentDir(), "bin");
  const currentPath = sanitized[pathKey] ?? "";
  const pathEntries = currentPath.split(delimiter).filter(Boolean);
  if (!pathEntries.includes(agentBinDir)) {
    sanitized[pathKey] = [agentBinDir, currentPath].filter(Boolean).join(delimiter);
  }
  return sanitized;
}

function projectCommandOperations(shellPath: string | undefined): BashOperations {
  const localOperations = createLocalBashOperations({ shellPath });
  return {
    exec(command, cwd, options) {
      return localOperations.exec(command, cwd, {
        ...options,
        env: sanitizedEnvironment(options.env ?? process.env),
      });
    },
  };
}

function currentBash(pi: ExtensionAPI): ToolInfo | undefined {
  return pi.getAllTools().find((tool) => tool.name === "bash");
}

function builtInOwnsBash(pi: ExtensionAPI): boolean {
  const bash = currentBash(pi);
  return bash?.sourceInfo.source === "builtin"
    || bash?.sourceInfo.path === "<builtin:bash>";
}

function isHostBash(bash: ToolInfo | undefined): boolean {
  const path = bash?.sourceInfo.path;
  return path !== undefined && !path.startsWith("<")
    && normalizedPath(path) === HOST_EXTENSION_PATH;
}

export default function projectCommandEnvironment(pi: ExtensionAPI): void {
  let settings: SettingsManager | undefined;
  let hostRegistered = false;

  pi.on("session_start", (_event, ctx) => {
    if (settings === undefined) {
      const sessionCwd = ctx.cwd;
      settings = SettingsManager.create(sessionCwd, getAgentDir(), {
        projectTrusted: ctx.isProjectTrusted(),
      });
    }
  });

  pi.on("resources_discover", (event) => {
    if (hostRegistered || settings === undefined || !builtInOwnsBash(pi)) return undefined;

    const sessionSettings = settings;
    const displayDefinition = createBashToolDefinition(event.cwd);
    pi.registerTool({
      ...displayDefinition,
      execute(toolCallId, params, signal, onUpdate, executionContext) {
        const executionCwd = executionContext?.cwd ?? event.cwd;
        const executionDefinition = createBashToolDefinition(executionCwd, {
          commandPrefix: sessionSettings.getShellCommandPrefix(),
          operations: projectCommandOperations(sessionSettings.getShellPath()),
        });
        return executionDefinition.execute(
          toolCallId,
          params,
          signal,
          onUpdate,
          executionContext,
        );
      },
    });
    hostRegistered = isHostBash(currentBash(pi));
    return undefined;
  });

  pi.on("user_bash", () => {
    // bash 缺失表示 None/ReadOnly allowlist；host 未确认 owner 时让后续 handler 接管。
    if (!hostRegistered || settings === undefined || !isHostBash(currentBash(pi))) {
      return undefined;
    }
    return { operations: projectCommandOperations(settings.getShellPath()) };
  });
}
